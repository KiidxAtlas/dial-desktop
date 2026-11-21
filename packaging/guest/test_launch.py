"""Host-side tests for the in-guest launcher. Commands are fake; nothing is executed.

Run: python3 -m unittest packaging/guest/test_launch.py   (from the repository root)
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import shutil
import sys
import tempfile
import unittest
from typing import List, Tuple

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import launch  # noqa: E402
from launch import Limits, LaunchError  # noqa: E402

SMALL = Limits(max_files=50, max_bytes=64 * 1024, max_depth=4, max_config_bytes=4096)
HAS_SYMLINK = hasattr(os, "symlink")
HAS_FIFO = hasattr(os, "mkfifo")


def write_app(root: str, targets: dict, version: object = 1) -> None:
    os.makedirs(os.path.join(root, ".dial"), exist_ok=True)
    with open(os.path.join(root, ".dial", "app.json"), "w", encoding="utf-8") as handle:
        json.dump({"version": version, "targets": targets}, handle)


class FakeRunner:
    """Records argv/cwd and returns scripted exit codes, one per call (default 0)."""

    def __init__(self, codes: Tuple[int, ...] = (), error: Exception = None):
        self.calls: List[Tuple[List[str], str, bool]] = []
        self._codes = list(codes)
        self._error = error

    def __call__(self, argv: List[str], cwd: str) -> int:
        existed = os.path.isdir(cwd)
        self.calls.append((list(argv), cwd, existed))
        if self._error is not None:
            raise self._error
        return self._codes.pop(0) if self._codes else 0


def run_launch(root: str, platform: str, runner: FakeRunner, limits: Limits = launch.DEFAULT_LIMITS):
    """Return (exit_code_or_exception_type, stderr_text)."""
    err = io.StringIO()
    with contextlib.redirect_stderr(err):
        try:
            code = launch.launch(root, platform_name=platform, runner=runner, limits=limits)
        except LaunchError as error:
            code = ("error", error.exit_code, str(error))
    return code, err.getvalue()


class PlatformSelectionTests(unittest.TestCase):
    def test_exact_platform_mapping(self):
        config = launch.parse_config({"version": 1, "targets": {"linux": {"run": ["a"]}, "windows": {"run": ["b"]}, "macos": {"run": ["c"]}}})
        cases = {"linux": "linux", "win32": "windows", "darwin": "macos"}
        for platform, key in cases.items():
            with self.subTest(platform=platform):
                self.assertEqual(launch.select_target(config, platform)[0], key)

    def test_unknown_platforms_are_refused_without_fallback(self):
        config = launch.parse_config({"version": 1, "targets": {"linux": {"run": ["a"]}}})
        for platform in ("linux2", "cygwin", "freebsd14", "Linux", "", "win"):
            with self.subTest(platform=platform), self.assertRaisesRegex(LaunchError, "unsupported guest platform"):
                launch.select_target(config, platform)

    def test_missing_target_for_platform_is_refused(self):
        config = launch.parse_config({"version": 1, "targets": {"linux": {"run": ["a"]}}})
        with self.assertRaisesRegex(LaunchError, "no target for windows"):
            launch.select_target(config, "win32")


class ConfigValidationTests(unittest.TestCase):
    def test_valid_config_parses(self):
        config = launch.parse_config(
            {
                "version": 1,
                "targets": {
                    "linux": {"cwd": "app/", "setup": [["python3", "-m", "venv", ".venv"]], "run": [".venv/bin/python", "main.py"]},
                    "macos": {"run": ["/usr/bin/true"]},
                },
            }
        )
        self.assertEqual(config.targets["linux"].cwd, "app")
        self.assertEqual(config.targets["linux"].setup, (("python3", "-m", "venv", ".venv"),))
        self.assertEqual(config.targets["macos"].cwd, ".")
        self.assertEqual(config.targets["macos"].setup, ())

    def test_invalid_configs_are_refused(self):
        valid_run = {"run": ["app"]}
        cases = {
            "non-object root": [],
            "missing version": {"targets": {"linux": valid_run}},
            "bool version": {"version": True, "targets": {"linux": valid_run}},
            "wrong version": {"version": 2, "targets": {"linux": valid_run}},
            "string version": {"version": "1", "targets": {"linux": valid_run}},
            "unknown top key": {"version": 1, "targets": {"linux": valid_run}, "extra": 1},
            "missing targets": {"version": 1},
            "empty targets": {"version": 1, "targets": {}},
            "unknown platform key": {"version": 1, "targets": {"freebsd": valid_run}},
            "target not object": {"version": 1, "targets": {"linux": ["app"]}},
            "unknown target key": {"version": 1, "targets": {"linux": {"run": ["app"], "shell": "sh -c x"}}},
            "missing run": {"version": 1, "targets": {"linux": {"setup": []}}},
            "empty run": {"version": 1, "targets": {"linux": {"run": []}}},
            "run is string": {"version": 1, "targets": {"linux": {"run": "app"}}},
            "run non-string arg": {"version": 1, "targets": {"linux": {"run": ["app", 3]}}},
            "run empty exe": {"version": 1, "targets": {"linux": {"run": [""]}}},
            "run NUL byte": {"version": 1, "targets": {"linux": {"run": ["app", "a\x00b"]}}},
            "setup not list": {"version": 1, "targets": {"linux": {"setup": "pip install x", "run": ["app"]}}},
            "setup step is string": {"version": 1, "targets": {"linux": {"setup": ["pip install x"], "run": ["app"]}}},
            "setup step empty": {"version": 1, "targets": {"linux": {"setup": [[]], "run": ["app"]}}},
            "cwd absolute": {"version": 1, "targets": {"linux": {"cwd": "/etc", "run": ["app"]}}},
            "cwd parent": {"version": 1, "targets": {"linux": {"cwd": "../x", "run": ["app"]}}},
            "cwd nested parent": {"version": 1, "targets": {"linux": {"cwd": "a/../../x", "run": ["app"]}}},
            "cwd drive letter": {"version": 1, "targets": {"windows": {"cwd": "C:\\x", "run": ["app"]}}},
            "cwd empty": {"version": 1, "targets": {"linux": {"cwd": "", "run": ["app"]}}},
            "cwd not string": {"version": 1, "targets": {"linux": {"cwd": 5, "run": ["app"]}}},
        }
        for name, data in cases.items():
            with self.subTest(case=name), self.assertRaises(LaunchError):
                launch.parse_config(data)

    def test_limits_on_setup_and_argv(self):
        tight = Limits(max_setup_steps=1, max_argv=2, max_arg_bytes=3)
        cases = {
            "too many setup steps": {"setup": [["a"], ["b"]], "run": ["app"]},
            "too many argv": {"run": ["app", "1", "2"]},
            "arg too long": {"run": ["app", "toolong"]},
        }
        for name, entry in cases.items():
            with self.subTest(case=name), self.assertRaises(LaunchError):
                launch.parse_config({"version": 1, "targets": {"linux": entry}}, tight)

    def test_duplicate_keys_in_json_are_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "app.json")
            with open(path, "w", encoding="utf-8") as handle:
                handle.write('{"version": 1, "version": 1, "targets": {"linux": {"run": ["app"]}}}')
            with self.assertRaisesRegex(LaunchError, "repeats key"):
                launch.load_config(path)

    def test_safe_relative_dir_table(self):
        ok = {"app": "app", "a/b": os.path.join("a", "b"), "./": ".", "a\\b": os.path.join("a", "b"), "a//./b": os.path.join("a", "b")}
        for value, expected in ok.items():
            with self.subTest(value=value):
                self.assertEqual(launch.safe_relative_dir(value, "cwd"), expected)
        for value in ("/abs", "..", "a/../..", "C:/x", "\\\\server\\share", "x\x00y", ""):
            with self.subTest(value=value), self.assertRaises(LaunchError):
                launch.safe_relative_dir(value, "cwd")


class CopySnapshotTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="launch-test-")
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.src = os.path.join(self.tmp, "src")
        os.mkdir(self.src)
        self.dst = os.path.join(self.tmp, "dst")

    def test_copies_tree_and_keeps_executable_bit(self):
        os.makedirs(os.path.join(self.src, "sub", "deep"))
        with open(os.path.join(self.src, "sub", "deep", "data.txt"), "w") as handle:
            handle.write("hi")
        script = os.path.join(self.src, "run.sh")
        with open(script, "w") as handle:
            handle.write("#!/bin/sh\n")
        os.chmod(script, 0o755)
        launch.copy_snapshot(self.src, self.dst, SMALL)
        with open(os.path.join(self.dst, "sub", "deep", "data.txt")) as handle:
            self.assertEqual(handle.read(), "hi")
        if os.name == "posix":
            self.assertTrue(os.stat(os.path.join(self.dst, "run.sh")).st_mode & 0o100)

    def test_writes_to_copy_do_not_touch_source(self):
        with open(os.path.join(self.src, "f.txt"), "w") as handle:
            handle.write("orig")
        launch.copy_snapshot(self.src, self.dst, SMALL)
        with open(os.path.join(self.dst, "f.txt"), "w") as handle:
            handle.write("changed")
        with open(os.path.join(self.src, "f.txt")) as handle:
            self.assertEqual(handle.read(), "orig")

    @unittest.skipUnless(HAS_SYMLINK, "symlinks unavailable")
    def test_symlink_is_refused(self):
        os.symlink("/etc/hosts", os.path.join(self.src, "hosts"))
        with self.assertRaisesRegex(LaunchError, "symbolic link"):
            launch.copy_snapshot(self.src, self.dst, SMALL)

    @unittest.skipUnless(HAS_FIFO, "fifo unavailable")
    def test_special_file_is_refused(self):
        os.mkfifo(os.path.join(self.src, "pipe"))
        with self.assertRaisesRegex(LaunchError, "special file"):
            launch.copy_snapshot(self.src, self.dst, SMALL)

    def test_limits_are_enforced(self):
        def three_files():
            for name in ("a.txt", "b.txt", "c.txt"):
                with open(os.path.join(self.src, name), "w") as handle:
                    handle.write("0123456789ABCDEF")

        def nested():
            os.makedirs(os.path.join(self.src, "x", "y"))

        cases = {
            "too many files": (Limits(max_files=2), three_files, "more than 2 files"),
            "too many bytes": (Limits(max_bytes=10, chunk_bytes=4), three_files, "larger than 10 bytes"),
            "too deep": (Limits(max_depth=1), nested, "deeper than 1 levels"),
        }
        for name, (limits, populate, message) in cases.items():
            with self.subTest(case=name):
                populate()
                with self.assertRaisesRegex(LaunchError, message):
                    launch.copy_snapshot(self.src, self.dst, limits)
                shutil.rmtree(self.dst, ignore_errors=True)
                shutil.rmtree(self.src)
                os.mkdir(self.src)


class LaunchFlowTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="launch-flow-")
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.root = os.path.join(self.tmp, "snapshot")
        os.mkdir(self.root)
        os.makedirs(os.path.join(self.root, "app"))
        with open(os.path.join(self.root, "app", "main.py"), "w") as handle:
            handle.write("print('hi')\n")

    def test_setup_then_run_in_copied_cwd_and_passes_status(self):
        write_app(self.root, {"linux": {"cwd": "app", "setup": [["python3", "-m", "venv", ".venv"]], "run": ["python3", "main.py"]}})
        runner = FakeRunner(codes=(0, 7))
        code, _ = run_launch(self.root, "linux", runner, SMALL)
        self.assertEqual(code, 7)
        self.assertEqual([call[0] for call in runner.calls], [["python3", "-m", "venv", ".venv"], ["python3", "main.py"]])
        for argv, cwd, existed in runner.calls:
            self.assertTrue(existed)
            self.assertTrue(cwd.endswith(os.path.join("app")))
            self.assertNotIn(self.root, cwd)
            self.assertTrue(os.path.isabs(cwd))

    def test_failing_setup_skips_run_and_reports(self):
        write_app(self.root, {"linux": {"setup": [["false-step"], ["second"]], "run": ["app"]}})
        runner = FakeRunner(codes=(2,))
        code, err = run_launch(self.root, "linux", runner, SMALL)
        self.assertEqual(code[0:2], ("error", 3))
        self.assertIn("setup 1/2 failed with status 2", code[2])
        self.assertEqual(len(runner.calls), 1)
        self.assertNotIn("second", err)

    def test_setup_that_cannot_start_skips_run(self):
        write_app(self.root, {"linux": {"setup": [["missing-tool", "--secret-token=abc123"]], "run": ["app"]}})
        runner = FakeRunner(error=FileNotFoundError(2, "No such file"))
        code, err = run_launch(self.root, "linux", runner, SMALL)
        self.assertEqual(code[0:2], ("error", 3))
        self.assertEqual(len(runner.calls), 1)
        self.assertIn("could not start missing-tool", code[2])

    def test_run_that_cannot_start_is_a_configuration_style_error(self):
        write_app(self.root, {"linux": {"run": ["nope"]}})
        code, _ = run_launch(self.root, "linux", FakeRunner(error=PermissionError(13, "Permission denied")), SMALL)
        self.assertEqual(code[0:2], ("error", 2))

    def test_secrets_in_args_are_never_logged(self):
        write_app(self.root, {"linux": {"setup": [["pip", "install", "--index-url=https://u:TOKEN-XYZ@example.invalid"]], "run": ["app", "--key=SECRET-VALUE"]}})
        code, err = run_launch(self.root, "linux", FakeRunner(), SMALL)
        self.assertEqual(code, 0)
        self.assertNotIn("TOKEN-XYZ", err)
        self.assertNotIn("SECRET-VALUE", err)
        self.assertIn("run: app", err)

    def test_nonzero_and_signal_statuses(self):
        write_app(self.root, {"linux": {"run": ["app"]}})
        cases = {"status 5": 5, "signal 9": -9}
        expected = {"status 5": 5, "signal 9": 137}
        for name, status in cases.items():
            with self.subTest(case=name):
                code, _ = run_launch(self.root, "linux", FakeRunner(codes=(status,)), SMALL)
                self.assertEqual(code, expected[name])

    def test_unknown_platform_runs_nothing(self):
        write_app(self.root, {"linux": {"run": ["app"]}})
        runner = FakeRunner()
        code, _ = run_launch(self.root, "freebsd", runner, SMALL)
        self.assertEqual(code[0:2], ("error", 2))
        self.assertEqual(runner.calls, [])

    def test_missing_platform_target_runs_nothing(self):
        write_app(self.root, {"linux": {"run": ["app"]}})
        runner = FakeRunner()
        code, _ = run_launch(self.root, "win32", runner, SMALL)
        self.assertIn("no target for windows", code[2])
        self.assertEqual(runner.calls, [])

    def test_missing_config_is_reported(self):
        runner = FakeRunner()
        code, _ = run_launch(self.root, "linux", runner, SMALL)
        self.assertIn("no .dial/app.json", code[2])
        self.assertEqual(runner.calls, [])

    def test_cwd_must_exist_in_snapshot(self):
        write_app(self.root, {"linux": {"cwd": "missing", "run": ["app"]}})
        runner = FakeRunner()
        code, _ = run_launch(self.root, "linux", runner, SMALL)
        self.assertIn("target cwd is not a directory", code[2])
        self.assertEqual(runner.calls, [])

    def test_relative_executable_resolves_inside_copy(self):
        os.makedirs(os.path.join(self.root, "app", "bin"))
        write_app(self.root, {"linux": {"cwd": "app", "run": ["./bin/tool", "x"]}})
        runner = FakeRunner()
        run_launch(self.root, "linux", runner, SMALL)
        argv, cwd, _ = runner.calls[0]
        self.assertEqual(argv[1:], ["x"])
        self.assertEqual(argv[0], os.path.join(cwd, "bin", "tool"))

    def test_relative_executable_cannot_escape_snapshot(self):
        # The schema already rejects '..' in cwd; the executable check must hold too.
        write_app(self.root, {"linux": {"run": ["../../evil", "x"]}})
        runner = FakeRunner()
        code, _ = run_launch(self.root, "linux", runner, SMALL)
        self.assertIsInstance(code, tuple)
        self.assertEqual(runner.calls, [])

    def test_bare_command_names_are_not_path_rewritten(self):
        write_app(self.root, {"linux": {"run": ["python3", "main.py"]}})
        runner = FakeRunner()
        run_launch(self.root, "linux", runner, SMALL)
        self.assertEqual(runner.calls[0][0], ["python3", "main.py"])

    def test_work_copy_is_removed_after_run(self):
        write_app(self.root, {"linux": {"run": ["app"]}})
        runner = FakeRunner()
        run_launch(self.root, "linux", runner, SMALL)
        cwd = runner.calls[0][1]
        self.assertFalse(os.path.exists(cwd))

    def test_work_copy_is_removed_after_setup_failure(self):
        write_app(self.root, {"linux": {"setup": [["bad"]], "run": ["app"]}})
        runner = FakeRunner(codes=(1,))
        run_launch(self.root, "linux", runner, SMALL)
        self.assertFalse(os.path.exists(runner.calls[0][1]))

    def test_snapshot_refusal_happens_before_any_command(self):
        write_app(self.root, {"linux": {"run": ["app"]}})
        big = Limits(max_files=1)
        runner = FakeRunner()
        code, _ = run_launch(self.root, "linux", runner, big)
        self.assertIn("more than 1 files", code[2])
        self.assertEqual(runner.calls, [])

    def test_version_mismatch_runs_nothing(self):
        write_app(self.root, {"linux": {"run": ["app"]}}, version=2)
        runner = FakeRunner()
        code, _ = run_launch(self.root, "linux", runner, SMALL)
        self.assertIn("version must be 1", code[2])
        self.assertEqual(runner.calls, [])


if __name__ == "__main__":
    unittest.main()
