# App runtime packaging

The App panel shows the hypervisor built into the computer Dial runs on, as a checklist with an install button for each missing part (`src-tauri/src/app_runner/host.rs`). Nothing is installed until you click and confirm in a native dialog. Each install runs a fixed command by absolute path, never one found on `PATH`.

| Host | Backend | What the panel can install |
| --- | --- | --- |
| macOS (Apple silicon) | Apple's Virtualization framework, through the packaged `dial-mac-guest` helper | **Download and install macOS**: the packaged `dial-mac-prepare` downloads Apple's latest restore image for this Mac and installs it into a new private virtual Mac. A window opens for Setup Assistant; the `DIAL_TOOLS` disc carries the guest companion installer. When you shut the guest down, Dial imports the base into `app-runner/macos/` and deletes the restore image. |
| Windows | QEMU with the Windows Hypervisor Platform (Hyper-V) | Turn on the `HypervisorPlatform` feature (UAC, then a restart), and install QEMU machine-wide with winget (`SoftwareFreedomConservancy.QEMU`). |
| Linux | QEMU with KVM | Add you to the `kvm` group or load the KVM module through `pkexec`, and install QEMU and OVMF with apt-get, dnf, pacman or zypper through `pkexec`. |

The macOS guest still needs the steps only you can do inside it: Setup Assistant, the companion installer, its Screen Recording and Accessibility grants, and automatic login (see [preparing a virtual Mac](../packaging/macos-guest/prepare/README.md)). On Windows and Linux, the guest disk is imported, not downloaded: choose a bootable raw disk for this machine's architecture.

When a build bundles no QEMU runtime, Dial uses a machine-wide QEMU instead, and only from admin-owned locations: `%ProgramFiles%\qemu` on Windows, or a root-owned, non-writable file under `/usr` on Linux (`runtime::system_qemu`). A bundled runtime is always preferred and still checked against its manifest.

## Registered virtual machines

**App → VM** lists registered VirtualBox and VMware machines from the local provider installations, including powered-off machines. This mode does not need Dial's QEMU bundle or an imported guest image. If a machine is missing, register it in the provider app first and refresh the list. Dial does not search the project for VM files, install VirtualBox or VMware, or import or rewrite machine settings.

Select a machine and start it to use its guest screen and keyboard in the sidebar. An agent cannot start a registered VM until you allow one launch for that machine, or see the screen of a user-controlled VM until you hand over control. Interact with the guest to take it back. Closing the pane leaves the session attached; **Disconnect** releases the console without powering off the machine. Writes are persistent and any existing shared folders, clipboard and network access remain in effect.

**SSH for commands (optional).** Below the VM list, **SSH host for commands** links a VM to a host alias from your `~/.ssh/config` (suggested from its `Host` entries). When no link is saved, an alias with the same name as the VM (ignoring case) is used automatically. The agent's `app_desktop` `state` then reports `ssh: {host, source}` for that VM, but only while Agent access is on for its run or for the VM you allowed it to launch. The agent then runs commands and reads files with Dial's `ssh` tool instead of typing into the screen and taking screenshots, and still uses the screen for graphical apps. Linking stores only the alias, in `app-runner/vm-ssh.json` under its app data, and changes nothing else; only **Set up SSH** below edits SSH config, keys or the guest. Neither bypasses the `ssh` tool's approvals. The guest must already accept SSH: for example, enable `sshd` in the guest and reach it through a bridged address or a NAT port forward named in `~/.ssh/config`.

**Set up SSH (VirtualBox, one press).** While a VirtualBox VM is connected and you are logged in to its desktop, enter the guest user name and press **Set up SSH**. Dial checks the VM's network. A NAT adapter gets a loopback SSH port forward (`controlvm natpfN`); a bridged adapter is found by its MAC in the host ARP table. Dial creates a dedicated key `~/.ssh/dial_<alias>` (ed25519) and opens a terminal in the guest with Ctrl+Alt+T, unless you tick that one is already open. It then types one command as your input: enable `sshd` (installing `openssh-server` with apt when missing) and add the key to `authorized_keys`. `sudo` asks for the guest password in the VM screen. Dial waits up to 5 minutes for SSH to accept the key, then writes a marked `Host <alias>` block at the top of `~/.ssh/config` (pressing again replaces it; your other entries are kept; an alias you already use becomes `dial-<alias>`) and links it to the VM.

VirtualBox needs its Python SDK alongside `VBoxManage`. On macOS and Linux the SDK must work with `/usr/bin/python3`; on Windows it needs a machine-wide Python 3 under Program Files with pywin32. Guest clicks require an absolute pointing device such as a USB tablet. Dial does not install these prerequisites or change the VM's devices.

VMware Fusion or Workstation must already expose the selected guest's VNC console on an explicit loopback address and port. Dial verifies the listener and the connected socket against the selected VM before exchanging display or input data. Password-authenticated VNC is not supported. Dial does not enable remote display, change a password, or open a port. If the existing configuration cannot be verified, use the provider's console instead.

Dial's own QEMU guests serve their display as VNC on a Unix socket inside the private run folder (`vnc.sock`), never on a TCP port, so the live screen receives only the regions that changed. Where that socket is unavailable (Windows, or a run folder path too long for a socket address), or if the display connection fails, the run falls back to QMP screendumps.

For live acceptance use a disposable registered VM: start it from the picker, type in a guest editor, hand it to the agent, take it back and confirm the agent can no longer read its screen. Hide and reopen the pane, disconnect, then confirm in the provider that the same VM is still running. A mocked console or successful inventory query does not verify those steps.

## What is in this repository

| Path | Purpose |
| --- | --- |
| `packaging/runtime/runtime-bundle.mjs` | Validator and stager: manifest parsing, tree and hash verification, atomic copy into place. |
| `packaging/runtime/stage-runtime.mjs` | CLI wrapper (`--check` and staging). |
| `packaging/runtime/stage-runtime.test.mjs` | Fixture-based tests (`node --test packaging/runtime/stage-runtime.test.mjs`). No QEMU binary is needed. |
| `src-tauri/tauri.runtime.conf.json` | Opt-in Tauri config overlay that maps `resources/qemu/` into the app resources as `qemu/`. |
| `src-tauri/resources/qemu/` | Output of staging. Git-ignored. Never committed. |

Nothing in this flow downloads anything, touches the system, or consults `PATH`. The tool only reads
a bundle directory that you have already built and copies validated files.

## Supplying a bundle

You build the QEMU runtime yourself, per host target, from exact upstream sources, and place it in a
directory. That directory is the input to the tool. Dial does not fetch QEMU, firmware or images.

```sh
# Verify a bundle without writing anything
node packaging/runtime/stage-runtime.mjs --target x86_64-unknown-linux-gnu --check /path/to/bundle

# Stage it into src-tauri/resources/qemu (replace a previous staging with --force)
node packaging/runtime/stage-runtime.mjs --target x86_64-unknown-linux-gnu --from /path/to/bundle
```

`--target` is required (no host default), so a release build cannot silently stage the wrong
architecture. Supported targets are exactly the release matrix in
`.github/workflows/release.yml`:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `x86_64-pc-windows-msvc`
- `x86_64-unknown-linux-gnu`

Then build with the overlay, which only applies when the staged directory exists:

```sh
npm run build -- --config src-tauri/tauri.runtime.conf.json
```

Without the overlay, Tauri does not look for `resources/qemu` and the bundle is unchanged. With the
overlay and no staging, the build fails, which is deliberate: a runtime build must not silently
omit its runtime. The tool's output is git-ignored by `.gitignore`.

## Bundle layout

```
<bundle>/
  runtime.json                 manifest (schema 1), not listed in itself
  bin/qemu-system-<arch>[.exe] the one executable this target ships
  bin/*.dll                    Windows only: runtime DLLs sit next to the exe
  lib/...                      Linux/macOS shared libraries the binary needs
  share/edk2-<arch>-code.fd    UEFI firmware code (required; name from Arch::firmware_code)
  share/edk2-<arch>-vars.fd    UEFI variable store template (required; Arch::firmware_vars)
  share/...                    other firmware or data, if any, listed in the manifest
  licenses/...                 license texts and notices (at least one file)
```

`<arch>` is the target's architecture (`x86_64` or `aarch64`). A target ships exactly one
`qemu-system-*` binary for its own architecture. Cross-architecture guests are not included.

### Manifest

```json
{
  "schema": 1,
  "target": "x86_64-unknown-linux-gnu",
  "binary": "bin/qemu-system-x86_64",
  "qemu": {
    "version": "9.2.1",
    "sourceUrl": "https://download.qemu.org/qemu-9.2.1.tar.xz",
    "sourceSha256": "<64 lowercase hex>"
  },
  "files": [
    { "path": "bin/qemu-system-x86_64", "sha256": "<hex>", "size": 123, "executable": true }
  ]
}
```

Every key is required; unknown keys are refused. `files` must list every regular file in the
bundle except `runtime.json`, and nothing else may be present. Paths are forward-slash relative and
must start with `bin/`, `lib/`, `share/` or `licenses/`. The manifest must include both
`share/edk2-<arch>-code.fd` and `share/edk2-<arch>-vars.fd` for the target's architecture (the names
`Arch::firmware_code` and `Arch::firmware_vars` return in `src-tauri/src/app_runner/runtime.rs`), and
at least one `licenses/` file.

### What verification refuses

The validator (`runtime-bundle.mjs`) refuses, with a message naming the file:

- symbolic links or any special file, anywhere in the bundle, and symlinked source or output directories;
- undeclared files, missing declared files, size or SHA-256 drift;
- absolute paths, `..` or `.` segments, backslashes, drive letters, case-insensitive duplicates;
- a binary that is not the target's `bin/qemu-system-<arch>[.exe]`, or is not marked executable;
- missing or wrong-architecture firmware (both `share/edk2-<arch>-{code,vars}.fd` are required);
- path forms Windows cannot store faithfully, on every platform: device names (`CON`, `PRN`, `AUX`,
  `NUL`, `COM0`-`COM9`, `LPT0`-`LPT9`, with or without an extension), alternate data streams (any
  `:`), characters `<>"|?*` and control characters, and a segment ending in `.` or a space;
- on Linux and macOS, an executable bit that does not match the manifest (Windows has no exec bit, so
  this check is skipped there and the `.exe` name is the rule instead);
- a manifest for a different target, a non-`https://` source URL, or a missing license.

Staging writes a canonical `runtime.json` (sorted files, two-space indent), so the same input always
produces byte-identical output. The copy is re-verified before it is moved into place. A destination
that holds unrelated content is never removed.

## Runtime contract (must match `src-tauri/src/app_runner/runtime.rs`)

The Rust reader is owned by the guest backend. Its contract:

- The runtime is read from the Tauri resource directory, `<resource_dir>/qemu/`, and the manifest at
  `<resource_dir>/qemu/runtime.json` is the only source of paths.
- The binary is the manifest's `binary` path, resolved under `qemu/`. It is never found through
  `PATH`, an environment variable or a user setting.
- The binary's and every firmware file's SHA-256 is checked against the manifest before spawn.
- Symlinks are refused at runtime as well as at staging.
- The manifest's `target` must equal the compile-time target, or the runtime is reported unavailable.
- Explicit `default` (unlabelled QEMU guest) uses the original `app-runner/guest.raw`. `linux` and `windows` use separate `linux.raw` and `windows.raw` files. These labels select disks, not certified OS installations. Omitting the target selects `windows` on Windows, `linux` on Linux, `macos` on Apple Silicon, or `default` elsewhere. A missing labelled disk never causes a switch to `guest.raw`. The native Mac backend uses its own `app-runner/macos/` base.

## Accepting the Apple Silicon default

Use a disposable test account inside a licensed, prepared macOS base. Do not grant host Screen Recording or Accessibility access for this test.

1. Open the App pane in an arm64 Dial build with no imported Mac. It should select macOS, explain the missing component, and **not** boot an existing QEMU disk. Explicitly selecting the unlabelled QEMU disk should still select that disk.
2. Import the shut-down virtual Mac and start **without** project sharing. Wait for a guest desktop, then open TextEdit **inside the panel** and type a distinctive phrase into a new document. Verify the phrase in a fresh guest frame, not merely a successful input response. Hide and reopen the pane: the same document should still be there.
3. Save the document to the guest Desktop, stop the run, and start again. The document should be absent. Do not save anything to the base or host. If the guest companion is missing, logged out, or lacks its guest permissions, expect a refusal rather than a host screen or input fallback.
4. To test Simple Stipple, prepare the base with the app's Python and dependencies **inside the guest** first. Start with the committed snapshot explicitly shared, copy the snapshot into a writable guest folder, and invoke `python3 main.py` there using the guest environment. Count success only after its own window opens and responds to input. The host checkout and `.venv` are not mounted.

## Smoke test on a real guest (packagers and developers)

The ignored Rust test `real_guest_shows_a_picture_takes_input_and_stops`
(`src-tauri/src/app_runner/tests.rs`) boots the staged bundle on a disposable disk. It runs the
production owners: the bundle verifier (`stage-runtime.mjs --check`), `runtime::discover`,
`qemu::args`, the QMP handshake, `capture`, the input gestures, and `kill`. It is not part of
`cargo test`, and it never downloads or installs anything.

Prerequisites, all explicit. Missing one is a failure, not a skip:

- A staged bundle: `DIAL_APP_RUNNER_SMOKE_RESOURCES` names the resource directory that contains
  `qemu/runtime.json`. After staging, that is `src-tauri/resources`.
- A disposable raw disk: `DIAL_APP_RUNNER_SMOKE_DISK` names an image. The test copies it into a
  temporary data directory, so the source is only read. The copy is discarded with the run.
- Node 22 on `PATH`, for the verifier only. The bundle itself is never executed by Node.
- A host whose target is in the release matrix, and a bundle built for that exact target. An
  Apple silicon host needs an `aarch64-apple-darwin` bundle.

```sh
# macOS or Linux
DIAL_APP_RUNNER_SMOKE_RESOURCES=src-tauri/resources \
DIAL_APP_RUNNER_SMOKE_DISK=/path/to/disposable.raw \
cargo test --locked --manifest-path src-tauri/Cargo.toml real_guest_ -- --ignored --nocapture
```

On Windows (PowerShell), set `$env:DIAL_APP_RUNNER_SMOKE_RESOURCES` and
`$env:DIAL_APP_RUNNER_SMOKE_DISK` and run the same `cargo test` line.

Set `DIAL_APP_RUNNER_SMOKE_EXPECT_INPUT_EFFECT=1` to require the screen to change after the click
and Enter. Without it the effect is printed only, because firmware may ignore that input.

What a pass shows, in order:

1. The verifier accepts the bundle, and discovery finds the QEMU binary and firmware.
2. The guest starts with the production argument list and answers QMP.
3. A screendump has non-blank pixels within two minutes. This is the **firmware frame**, the UEFI
   screen drawn before any operating system. It proves the emulator and firmware work on this host,
   not that a guest desktop runs.
4. A click and Enter are accepted by QEMU.
5. The process exits on `kill`, and the disk copy's size and mtime are unchanged. The writes stayed
   in snapshot mode.

A watchdog kills the guest after four minutes, so a hung handshake cannot stall the run.

What a pass does not show: a guest desktop, a working operating system, rendering in Dial's app
pane, Gatekeeper or Authenticode acceptance, or that the bundle's `lib/` rpaths resolve on a
machine other than this one. QEMU may use KVM, WHPX, HVF or TCG; a pass under TCG does not verify a hardware accelerator.

Known duplication: `start` in `app_runner.rs` owns the spawn (environment clearing, stdio pipes,
the log file and `CREATE_NO_WINDOW`), and the smoke test repeats it. The follow-up is one shared
`spawn_guest` owner used by both. Until then, a change to spawn must update both.

### Human GUI acceptance (per release target)

Automated checks cannot judge the pane. Before a release that ships a runtime, someone opens the
app on each target and confirms:

1. Starting the app runner on a workspace shows the firmware frame in the app pane within two
   minutes, and the pane is not blank or stretched.
2. A click and a key in the pane reach the guest. Use a disk where the effect is visible.
   For responsiveness, compare the time from a gesture to its visible effect. The capture loop wakes
   on input, checks more frequently during screen changes and backs off when the guest is idle.
   Capture frequency alone does not measure the webview's display latency.
3. Stop ends the QEMU process (`pgrep -fl qemu-system` shows nothing afterwards), and the imported
   disk image is unchanged.
4. With the runtime removed from the bundle, the pane reports the missing runtime, not a crash.

Record the host, the bundle's `runtime.json` target and the result. A pass on one target does not
cover the others.

## Licensing (read before shipping)

- QEMU is licensed under GPL-2.0 (with LGPL and BSD parts). Shipping its binary means you must also
  make the corresponding source available. Keep `qemu.sourceUrl` and `qemu.sourceSha256` in the
  manifest, and ship the exact upstream tarball or an equivalent offer in the release notes. Ship the
  license texts under `licenses/`. Dial's own MIT license does not cover QEMU.
- The edk2 firmware images have their own licenses and patent notices. Include each one under
  `licenses/`. Staging only checks that a license file exists, not that it is complete.
- Guest operating-system images are never bundled. Windows and Linux images carry their own license
  terms; users import images they are licensed to use.
- The app needs a third-party notices file that lists the bundled runtime before any release that
  includes it. This repository has none yet. Get a legal review before the first such release.

## Platform limits

- **Architectures:** each release target ships one host-architecture QEMU. There is no cross-arch
  guest support (for example x86 guests on Apple Silicon). Emulating a foreign architecture would
  need a second binary and is not part of this layout.
- **Windows and Linux acceleration:** build the Windows runtime with WHPX and TCG, and the Linux runtime with KVM and TCG. Hardware virtualization must be available to the host. WHPX requires the optional **Windows Hypervisor Platform** feature; KVM needs a usable `/dev/kvm` accessible to the user. Dial does not enable features, elevate privileges or change device permissions. If QEMU cannot initialize the hardware accelerator, it tries TCG (slower software emulation) with the **same** disk and isolation settings. This does not recover guest boot or driver failures.
- **Mac acceleration:** explicitly selected QEMU guests on Apple Silicon still use HVF. A local Apple Silicon test bundle booted Fedora LXQt with HVF, not KVM or WHPX.
- **Shared libraries:** the validator does not inspect ELF or Mach-O files. Linux and macOS
  binaries must find their `lib/` dependencies through relative rpaths (`$ORIGIN/../lib`,
  `@executable_path/../lib`). That is not verified by this tool. Check it with `ldd`, `otool -L`
  or `dumpbin` before release.
- **Signing:** on macOS the bundled QEMU binary and libraries must be signed by the same identity
  and be valid under the hardened runtime, or Gatekeeper will refuse them. On Windows the bundled
  `.exe` and DLLs need Authenticode signatures through the same signing path as the app. Neither is
  implemented yet; the current signing config (`tauri.release.conf.json`) is written for the app
  binary and does not show that it signs bundled resources.
- **Updates:** Tauri's updater signs the whole bundle archive, so staged resources are covered by the
  same update signature as the app. Changing the runtime requires a new release.

### Native app compatibility

The build targets above identify where **Dial** runs, not what an app can run on. A raw disk
at `app-runner/guest.raw` has no trusted OS or toolchain identity. Successfully booting it, or
seeing a `Cargo.toml`, `package.json` or Python lockfile, does not establish app compatibility.

| App target | Guest required | Acceptance gap |
| --- | --- | --- |
| Linux | Matching architecture, Linux userspace and GUI dependencies | On Apple Silicon, a disposable Fedora LXQt guest opened FeatherPad on a file created by an offline recipe's setup step. Guest screenshots showed the file before and after typing `ok` through QEMU. This tested the recipe and guest input, not a packaged Tauri sidebar end to end. |
| Windows | Licensed Windows guest, compatible CPU, boot devices and display/input drivers | Test with an actual Windows GUI app; a Windows *host* build or UEFI frame is not evidence. |
| macOS | Licensed, compatible virtual Mac on Apple Silicon, with a signed host helper and a guest companion in a logged-in desktop session | Live acceptance requires booting an imported base, opening an actual Mac app and interacting with it in the sidebar. Compilation and a passing import test do not establish that. No host display capture or input injection is used. |

**Available today:** In a runtime-enabled build, select the guest and start it with the committed snapshot checkbox enabled. A committed `.dial/app.json` adds `DIAL-LAUNCH.py` to the read-only project disc. Inside the guest, change into the disc's root and run `python3 -I DIAL-LAUNCH.py .` (Windows: `py -3 -I DIAL-LAUNCH.py .`). This runs the recipe for the actual guest OS in a temporary writable copy. Starting a guest does not run its project's code automatically. See the [recipe format and examples](../packaging/guest/README.md). Toolchains and dependencies must already be in the guest or provided offline. There is no prepared-base cache, export feature or dependency downloader.

**Future provisioning design (not implemented):**

1. Select a target OS, architecture and guest image before resolving dependencies. For source
   projects, prefer an explicit project recipe (working directory, setup and launch argument
   vectors, required toolchain and lock inputs). A known manifest can suggest a recipe; ambiguous
   projects, monorepos and native SDK requirements need a choice. Never execute it on the host.
2. Separate preparation from execution. Resolve the package and system-tool downloads first;
   show sources and total bytes for approval. If an installer fetches unknown additional data,
   refuse it or accept user-supplied offline media instead. The ordinary app run stays offline.
3. Publish a prepared base only after setup succeeds. Scope it to the project and key it by guest
   image digest, OS/architecture, toolchain, recipe and all setup inputs. If inputs cannot be
   identified, skip reuse. Boot disposable writes over that base; never promote an app run to
   the cache. Show cache usage, bound it, and allow removal without deleting a base in use.
4. Stop discards run writes, not the approved base. Saving an output requires an explicit export;
   do not implement persistence by sharing a writable host folder. No fallback to the host for
   unsupported drivers, hardware, credentials or missing runtimes.

**Acceptance per claimed host/guest pair:** cold prepare with approval, offline warm launch,
visible guest app and an input-driven change, invalidation on dependency changes, explicit
export, stop and cache removal. Verify denial and failed setup leave no usable partial base.
Exercise a native compiled app and an interpreted or managed-runtime app, not just boot screens.
Until a pair passes, report it as unverified. The target preference is a routing choice, not evidence of compatibility or permission to substitute another OS.


## Not yet done

- The release workflow (`.github/workflows/release.yml`) does not stage the optional QEMU runtime. Adding it needs
  a per-target step that builds or receives the bundle directory, runs `stage-runtime.mjs`, and passes
  `--config src-tauri/tauri.runtime.conf.json` alongside the native helper configuration on Apple Silicon.
- No CI job runs the fixture tests yet. They need Node 22 only.
- An ad-hoc signed macOS arm64 runtime has booted locally; release-signed bundles and live Linux
  and Windows boots remain unverified.
- The fixture tests are written to run on Windows: exec-bit checks are POSIX-only and symlink cases
  skip when the host cannot create links. They have been run on macOS only.
