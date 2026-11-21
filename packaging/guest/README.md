# Guest launcher (`packaging/guest/launch.py`)

`launch.py` is a language-neutral helper that runs **inside a guest OS** for a project the user has
explicitly opted in to. It uses only the Python 3 standard library. The Dial host never executes it,
and nothing in this directory runs on the host.

Commit `.dial/app.json`, then enable **Share a read-only snapshot of committed HEAD** before starting the guest. Dial adds its own `DIAL-LAUNCH.py` to the project disc; that name is reserved at the project root when a recipe exists. Nothing on the disc runs automatically. From a terminal **inside the guest**, change to the disc root and run:

```
python3 -I DIAL-LAUNCH.py .
```

On Windows use `py -3 -I DIAL-LAUNCH.py .`. Isolated mode (`-I`) keeps project files out of the launcher's Python import path. A recipe describes what to run; it does not supply the guest OS, runtime or dependencies. For the native Apple Silicon path see [preparing a virtual Mac](../macos-guest/prepare/README.md).

## Guest-only prerequisites

These are the guest's responsibility. The launcher does not install or download any of them.

- **Python 3.8 or newer** on `PATH` (`python3` on Linux and macOS, `python` or `py -3` on Windows) to run
  `launch.py` itself.
- **Whatever your app needs to run, preinstalled in the guest**: an interpreter, a compiler, a runtime
  or a GUI toolkit. Setup steps may use these tools, but they must not need network access unless the
  guest is configured for it.
- **Offline or vendored dependencies** are the recommended path. Commit them (for example `vendor/`
  or a wheelhouse) and reference them from setup with flags such as `pip --no-index --find-links`,
  `cargo --offline`, or `go build -mod=vendor`.
- **Writable temp space**: the launcher copies the snapshot to a private directory under the system
  temp directory (`$TMPDIR`, `/tmp`, or `%TEMP%`) and deletes it when the run ends. Setup output is
  written into that copy and is not synced back to the mounted source.

## `.dial/app.json`

```json
{
  "version": 1,
  "targets": {
    "linux": {
      "cwd": "app",
      "setup": [["python3", "-m", "venv", ".venv"],
                [".venv/bin/pip", "install", "--no-index", "--find-links", "vendor", "-r", "requirements.txt"]],
      "run": [".venv/bin/python", "main.py"]
    }
  }
}
```

| Key | Required | Meaning |
| --- | --- | --- |
| `version` | yes | Must be the integer `1`. Other values, including `true`, are refused. |
| `targets` | yes | Non-empty object. Keys are only `linux`, `windows` or `macos`. |
| `targets.<os>.cwd` | no | Relative directory inside the snapshot. Default `.`. Absolute paths, `..` and drive letters are refused. |
| `targets.<os>.setup` | no | List of argv lists, run in order in `cwd`. The first failure stops the launch before `run`. |
| `targets.<os>.run` | yes | One argv list. Its exit status becomes the launcher's exit status. |

Unknown keys are rejected at every level. Duplicate JSON keys are rejected.

Each command is an argv list run **without a shell**. Pipes, `&&`, globbing and `$VAR` expansion do
not happen. Wrap a shell explicitly if you need one, for example `["sh", "-c", "..."]`.

### Platform selection

The launcher compares `sys.platform` exactly:

| `sys.platform` | target key |
| --- | --- |
| `linux` | `linux` |
| `win32` | `windows` |
| `darwin` | `macos` |

Any other value is refused, and so is a guest whose OS has no matching target. Nothing falls back to
another target.

### Executable paths

- A bare name such as `python3` is looked up on the guest's `PATH`.
- An absolute path such as `/usr/bin/true` or `C:\Tools\x.exe` is used as given.
- A relative path with a separator such as `./bin/tool` or `.venv/bin/python` is resolved against
  `cwd` and must stay inside the copy.

## Safety rules

- The snapshot is copied first, and only the copy is validated and executed. Source changes made
  while the launcher runs do not reach the run.
- Symlinks and special files (FIFOs, devices, sockets) are refused, not skipped.
- Copy limits: 50 000 files, 2 GiB total, 32 directory levels, 1 MiB for `app.json`. Commands may
  have up to 32 setup steps, 256 argv entries and 4 KiB per argument.
- Logs show the step label and the executable's basename only. Arguments are never printed, so
  tokens in arguments stay out of the log.
- Exit codes: `2` for configuration or usage errors (and for `run` failing to start), `3` when a
  setup step fails or cannot start, otherwise `run`'s exit status (`128 + signal` if killed by a
  signal).

## Sample recipes

### Python app (vendored dependencies)

```json
{"version": 1, "targets": {
  "linux": {"cwd": "app",
    "setup": [["python3", "-m", "venv", ".venv"],
              [".venv/bin/pip", "install", "--no-index", "--find-links", "vendor", "-r", "requirements.txt"]],
    "run": [".venv/bin/python", "main.py"]},
  "windows": {"cwd": "app",
    "setup": [["py", "-3", "-m", "venv", ".venv"],
              [".venv\\Scripts\\pip.exe", "install", "--no-index", "--find-links", "vendor", "-r", "requirements.txt"]],
    "run": [".venv\\Scripts\\python.exe", "main.py"]}
}}
```

Do not rely on a host `.venv`: it may be untracked or contain host-specific paths and native libraries. Build it inside the guest from offline inputs.

### Compiled app (Rust, offline)

```json
{"version": 1, "targets": {
  "linux": {"cwd": ".",
    "setup": [["cargo", "build", "--release", "--offline", "--locked"]],
    "run": ["target/release/myapp", "--gui"]}
}}
```

Vendor crates first (`cargo vendor` plus `.cargo/config.toml` source replacement) so `--offline`
succeeds.

### Compiled app (Go, vendored)

```json
{"version": 1, "targets": {
  "linux": {"setup": [["go", "build", "-mod=vendor", "-o", "bin/myapp", "./cmd/myapp"]],
            "run": ["./bin/myapp"]}
}}
```

### Prebuilt binary, no setup

```json
{"version": 1, "targets": {
  "windows": {"cwd": "dist", "run": ["./MyApp.exe"]}
}}
```

## Tests

The tests use a fake runner, so no command runs on the host.

```sh
python3 -m unittest packaging/guest/test_launch.py
```

They cover platform selection, schema validation, copy limits, symlink and special-file refusal,
failing setup skipping `run`, unstartable commands, argument redaction in logs, exit status mapping,
and removal of the private copy.

Live guest verification (booting a guest, provisioning an XDG launcher and running a real project)
is a separate, manual step. It is not part of this test suite.
