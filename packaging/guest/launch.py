#!/usr/bin/env python3
"""In-guest launcher for a user-opted-in project snapshot.

Runs inside the guest OS (Linux, Windows or macOS) using only the Python standard
library. It is never executed by the Dial host. Usage:

    python3 launch.py SNAPSHOT_ROOT

SNAPSHOT_ROOT is the mounted project snapshot. It must contain ``.dial/app.json``:

    {
      "version": 1,
      "targets": {
        "linux": {
          "cwd": "app",
          "setup": [["python3", "-m", "venv", ".venv"], [".venv/bin/pip", "install", "--no-index", "--find-links", "vendor", "-r", "requirements.txt"]],
          "run": [".venv/bin/python", "main.py"]
        }
      }
    }

Rules enforced here:
- Only the target whose key exactly matches the guest ``sys.platform`` runs
  (``linux``, ``win32`` -> ``windows``, ``darwin`` -> ``macos``). There is no fallback.
- The snapshot is copied into a private, writable temporary directory first. Symlinks,
  special files, unsafe relative paths and oversized trees are refused. Everything
  validated and executed comes from that copy, not from the mounted source.
- Commands are argv lists executed without a shell. Relative executable paths must stay
  inside the copy.
- ``setup`` steps run in order. The first failure stops the launch before ``run``.
  Setup may only use what is already present in the guest (preinstalled toolchains and
  vendored or offline dependencies); this helper never downloads or installs anything.
- Logs name the step and executable basename only. Arguments are never printed, because
  they may contain tokens.

Exit codes: 2 for configuration or usage errors, 3 when a setup step fails, otherwise the
exit status of ``run`` (128 + signal number when it is killed by a signal).
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import PurePosixPath, PureWindowsPath
from typing import Callable, Dict, List, Optional, Sequence, Tuple

CONFIG_RELATIVE = (".dial", "app.json")
SCHEMA_VERSION = 1
TARGET_KEYS = ("linux", "windows", "macos")
# Exact sys.platform value -> target key. Anything else is refused.
PLATFORM_TARGETS: Dict[str, str] = {"linux": "linux", "win32": "windows", "darwin": "macos"}
TOP_LEVEL_KEYS = frozenset({"version", "targets"})
TARGET_FIELDS = frozenset({"cwd", "setup", "run"})


@dataclass(frozen=True)
class Limits:
    max_files: int = 50_000
    max_bytes: int = 2 * 1024 ** 3
    max_depth: int = 32
    max_config_bytes: int = 1024 * 1024
    max_setup_steps: int = 32
    max_argv: int = 256
    max_arg_bytes: int = 4096
    chunk_bytes: int = 1024 * 1024


DEFAULT_LIMITS = Limits()


class LaunchError(Exception):
    """A refusal or failure that should be shown to the user as one concise line."""

    def __init__(self, message: str, exit_code: int = 2):
        super().__init__(message)
        self.exit_code = exit_code


@dataclass(frozen=True)
class Target:
    cwd: str
    setup: Tuple[Tuple[str, ...], ...]
    run: Tuple[str, ...]


@dataclass(frozen=True)
class Config:
    version: int
    targets: Dict[str, Target] = field(default_factory=dict)


Runner = Callable[[List[str], str], int]


def _default_runner(argv: List[str], cwd: str) -> int:
    return subprocess.call(argv, cwd=cwd)


# ---------------------------------------------------------------- config validation


def _expect_object(value: object, where: str, allowed: frozenset, required: Sequence[str]) -> dict:
    if not isinstance(value, dict):
        raise LaunchError(f"{where} must be a JSON object")
    unknown = sorted(str(k) for k in value if k not in allowed)
    if unknown:
        raise LaunchError(f"{where} has unknown key(s): {', '.join(unknown)}")
    missing = [k for k in required if k not in value]
    if missing:
        raise LaunchError(f"{where} is missing required key(s): {', '.join(missing)}")
    return value


def _check_argv(value: object, where: str, limits: Limits) -> Tuple[str, ...]:
    if not isinstance(value, list):
        raise LaunchError(f"{where} must be a list of strings")
    if not value:
        raise LaunchError(f"{where} must not be empty")
    if len(value) > limits.max_argv:
        raise LaunchError(f"{where} has more than {limits.max_argv} entries")
    for index, item in enumerate(value):
        if not isinstance(item, str):
            raise LaunchError(f"{where}[{index}] must be a string")
        if "\x00" in item:
            raise LaunchError(f"{where}[{index}] contains a NUL byte")
        if len(item.encode("utf-8", "surrogatepass")) > limits.max_arg_bytes:
            raise LaunchError(f"{where}[{index}] is longer than {limits.max_arg_bytes} bytes")
    if not value[0]:
        raise LaunchError(f"{where}[0] (the executable) must not be empty")
    return tuple(value)


def safe_relative_dir(value: object, where: str) -> str:
    """Return a normalized relative path that cannot leave its base directory."""
    if not isinstance(value, str) or not value:
        raise LaunchError(f"{where} must be a non-empty relative path string")
    if "\x00" in value:
        raise LaunchError(f"{where} contains a NUL byte")
    if PurePosixPath(value).is_absolute() or PureWindowsPath(value).anchor:
        raise LaunchError(f"{where} must be relative to the snapshot, not absolute: {value!r}")
    parts = [part for part in re.split(r"[\\/]+", value) if part not in ("", ".")]
    if any(part == ".." for part in parts):
        raise LaunchError(f"{where} must not contain '..': {value!r}")
    return os.path.join(*parts) if parts else "."


def parse_config(data: object, limits: Limits = DEFAULT_LIMITS) -> Config:
    top = _expect_object(data, "app.json", TOP_LEVEL_KEYS, ("version", "targets"))
    version = top["version"]
    if type(version) is not int or version != SCHEMA_VERSION:
        raise LaunchError(f"app.json version must be {SCHEMA_VERSION}")
    raw_targets = top["targets"]
    if not isinstance(raw_targets, dict) or not raw_targets:
        raise LaunchError("app.json targets must be a non-empty object")
    unknown = sorted(str(k) for k in raw_targets if k not in TARGET_KEYS)
    if unknown:
        raise LaunchError(f"app.json targets has unknown platform key(s): {', '.join(unknown)}")

    targets: Dict[str, Target] = {}
    for name, raw in raw_targets.items():
        where = f"targets.{name}"
        entry = _expect_object(raw, where, TARGET_FIELDS, ("run",))
        cwd = safe_relative_dir(entry["cwd"], f"{where}.cwd") if "cwd" in entry else "."
        raw_setup = entry.get("setup", [])
        if not isinstance(raw_setup, list):
            raise LaunchError(f"{where}.setup must be a list of commands")
        if len(raw_setup) > limits.max_setup_steps:
            raise LaunchError(f"{where}.setup has more than {limits.max_setup_steps} steps")
        setup = tuple(_check_argv(step, f"{where}.setup[{i}]", limits) for i, step in enumerate(raw_setup))
        run = _check_argv(entry["run"], f"{where}.run", limits)
        targets[name] = Target(cwd=cwd, setup=setup, run=run)
    return Config(version=SCHEMA_VERSION, targets=targets)


def load_config(path: str, limits: Limits = DEFAULT_LIMITS) -> Config:
    try:
        size = os.path.getsize(path)
    except OSError:
        raise LaunchError("snapshot has no .dial/app.json") from None
    if size > limits.max_config_bytes:
        raise LaunchError(f"app.json is larger than {limits.max_config_bytes} bytes")

    def _no_duplicates(pairs: List[Tuple[str, object]]) -> dict:
        result: dict = {}
        for key, value in pairs:
            if key in result:
                raise LaunchError(f"app.json repeats key {key!r}")
            result[key] = value
        return result

    try:
        with open(path, "r", encoding="utf-8") as handle:
            data = json.load(handle, object_pairs_hook=_no_duplicates)
    except LaunchError:
        raise
    except (OSError, UnicodeDecodeError, ValueError) as error:
        raise LaunchError(f"app.json is not valid UTF-8 JSON: {type(error).__name__}") from None
    return parse_config(data, limits)


def select_target(config: Config, platform_name: str) -> Tuple[str, Target]:
    key = PLATFORM_TARGETS.get(platform_name)
    if key is None:
        raise LaunchError(f"unsupported guest platform {platform_name!r}; expected linux, win32 or darwin")
    target = config.targets.get(key)
    if target is None:
        declared = ", ".join(sorted(config.targets)) or "none"
        raise LaunchError(f"app.json has no target for {key} (declares: {declared})")
    return key, target


# ---------------------------------------------------------------- bounded copy


def copy_snapshot(source: str, destination: str, limits: Limits = DEFAULT_LIMITS) -> None:
    """Copy a regular-file/directory tree. Links and special files are refused, not skipped."""
    os.mkdir(destination)
    file_count = 0
    byte_total = 0
    pending: List[Tuple[str, str, str, int]] = [(source, destination, "", 0)]
    no_follow = getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_BINARY", 0)

    while pending:
        src_dir, dst_dir, rel_dir, depth = pending.pop()
        with os.scandir(src_dir) as scanner:
            entries = sorted(scanner, key=lambda entry: entry.name)
        for entry in entries:
            rel = os.path.join(rel_dir, entry.name) if rel_dir else entry.name
            src_path = os.path.join(src_dir, entry.name)
            dst_path = os.path.join(dst_dir, entry.name)
            if entry.is_symlink():
                raise LaunchError(f"snapshot contains a symbolic link, which is refused: {rel}")
            if entry.is_dir(follow_symlinks=False):
                if depth + 1 > limits.max_depth:
                    raise LaunchError(f"snapshot is nested deeper than {limits.max_depth} levels")
                os.mkdir(dst_path)
                pending.append((src_path, dst_path, rel, depth + 1))
                continue
            if not entry.is_file(follow_symlinks=False):
                raise LaunchError(f"snapshot contains a special file, which is refused: {rel}")

            file_count += 1
            if file_count > limits.max_files:
                raise LaunchError(f"snapshot has more than {limits.max_files} files")
            byte_total = _copy_file(src_path, dst_path, rel, byte_total, limits, no_follow)


def _copy_file(src: str, dst: str, rel: str, byte_total: int, limits: Limits, no_follow: int) -> int:
    try:
        fd = os.open(src, os.O_RDONLY | no_follow)
    except OSError as error:
        raise LaunchError(f"cannot read snapshot file {rel}: {error.strerror or type(error).__name__}") from None
    with os.fdopen(fd, "rb") as source:
        info = os.fstat(source.fileno())
        if not stat.S_ISREG(info.st_mode):
            raise LaunchError(f"snapshot contains a special file, which is refused: {rel}")
        with open(dst, "xb") as target:
            while True:
                chunk = source.read(limits.chunk_bytes)
                if not chunk:
                    break
                byte_total += len(chunk)
                if byte_total > limits.max_bytes:
                    raise LaunchError(f"snapshot is larger than {limits.max_bytes} bytes")
                target.write(chunk)
    # Keep the executable bit (scripts, binaries) and make sure the owner can rewrite the copy.
    os.chmod(dst, stat.S_IMODE(info.st_mode) | stat.S_IRUSR | stat.S_IWUSR)
    return byte_total


# ---------------------------------------------------------------- execution


def _is_absolute(path: str) -> bool:
    return PurePosixPath(path).is_absolute() or bool(PureWindowsPath(path).anchor)


def _inside(base: str, candidate: str) -> bool:
    try:
        return os.path.commonpath([base, candidate]) == base
    except ValueError:
        return False


def resolve_directory(work_root: str, relative: str) -> str:
    full = os.path.normpath(os.path.join(work_root, relative))
    if not _inside(work_root, full) or not os.path.isdir(full):
        raise LaunchError(f"target cwd is not a directory in the snapshot: {relative!r}")
    return full


def resolve_argv(argv: Tuple[str, ...], cwd: str, work_root: str) -> List[str]:
    """Pin executable paths that contain separators so they cannot escape the copy."""
    exe, rest = argv[0], list(argv[1:])
    if not re.search(r"[\\/]", exe) or _is_absolute(exe):
        # Bare names go through PATH lookup; absolute paths are the guest's own choice.
        return [exe] + rest
    safe = safe_relative_dir(exe, "executable")
    full = os.path.normpath(os.path.join(cwd, safe))
    if not _inside(work_root, full):
        raise LaunchError(f"executable path leaves the snapshot: {exe!r}")
    return [full] + rest


def _run_step(runner: Runner, argv: List[str], cwd: str, label: str) -> int:
    name = os.path.basename(argv[0]) or argv[0]
    print(f"dial-launch: {label}: {name}", file=sys.stderr)
    try:
        return runner(argv, cwd)
    except OSError as error:
        reason = error.strerror or type(error).__name__
        code = 3 if label.startswith("setup") else 2
        raise LaunchError(f"{label}: could not start {name}: {reason}", exit_code=code) from None


def _exit_status(code: int) -> int:
    return 128 - code if code < 0 else code


def _rmtree(path: str) -> None:
    def _make_writable_and_retry(function, target, _excinfo):
        try:
            os.chmod(target, stat.S_IRWXU)
        except OSError:
            return
        function(target)

    shutil.rmtree(path, onerror=_make_writable_and_retry)


def launch(
    snapshot_root: str,
    *,
    platform_name: str = sys.platform,
    runner: Runner = _default_runner,
    limits: Limits = DEFAULT_LIMITS,
) -> int:
    """Validate, copy, run setup, then run. Returns the process exit status."""
    root = os.path.abspath(snapshot_root)
    if os.path.islink(root) or not os.path.isdir(root):
        raise LaunchError(f"snapshot root is not a directory: {snapshot_root}")
    # Refuse an unknown platform before copying anything large.
    if platform_name not in PLATFORM_TARGETS:
        raise LaunchError(f"unsupported guest platform {platform_name!r}; expected linux, win32 or darwin")

    temp_root = tempfile.mkdtemp(prefix="dial-app-")
    try:
        work_root = os.path.join(temp_root, "app")
        copy_snapshot(root, work_root, limits)
        config = load_config(os.path.join(work_root, *CONFIG_RELATIVE), limits)
        key, target = select_target(config, platform_name)
        cwd = resolve_directory(work_root, target.cwd)

        for index, step in enumerate(target.setup, start=1):
            argv = resolve_argv(step, cwd, work_root)
            label = f"setup {index}/{len(target.setup)}"
            code = _run_step(runner, argv, cwd, label)
            if code != 0:
                raise LaunchError(
                    f"{label} failed with status {_exit_status(code)}; not starting the app",
                    exit_code=3,
                )

        argv = resolve_argv(target.run, cwd, work_root)
        print(f"dial-launch: starting {key} target", file=sys.stderr)
        return _exit_status(_run_step(runner, argv, cwd, "run"))
    finally:
        _rmtree(temp_root)


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser(
        prog="launch.py",
        description="Run the target in SNAPSHOT_ROOT/.dial/app.json for this guest OS.",
    )
    parser.add_argument("snapshot_root", help="mounted project snapshot containing .dial/app.json")
    args = parser.parse_args(argv)
    try:
        return launch(args.snapshot_root)
    except LaunchError as error:
        print(f"dial-launch: error: {error}", file=sys.stderr)
        return error.exit_code


if __name__ == "__main__":
    sys.exit(main())
