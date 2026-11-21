# Offline macOS base preparation (`Prepare.swift`)

`Prepare.swift` is an optional, single-file developer utility. It turns a restore image (IPSW) you
already have on disk into a macOS base VM that you finish setting up yourself, inside the VM. Dial
does not build or run it automatically. It lives at `packaging/macos-guest/prepare/`, next to the
runtime sources it feeds (`Host*.swift`, `Guest.swift`). It does not share names with them: every type
it declares is prefixed `OfflinePrep`.

It writes exactly four files into a new private folder:

| File                | Contents                                                              |
| ------------------- | --------------------------------------------------------------------- |
| `Disk.img`          | sparse virtual disk, the size chosen with `--disk-gib`                |
| `HardwareModel`     | `VZMacHardwareModel.dataRepresentation`                               |
| `MachineIdentifier` | `VZMacMachineIdentifier.dataRepresentation`                           |
| `AuxiliaryStorage`  | `VZMacAuxiliaryStorage` created by Virtualization.framework           |

Those four names are the complete import set. They match `MAC_BASE_FILES`
(`src-tauri/src/app_runner/runtime.rs:43-48`).

## How the base is imported and used (the import path exists)

- **Importer.** `src-tauri/src/app_disk/macos.rs` is the import action. The user picks a folder, and
  Dial checks the four files: each is a non-empty regular file with no symlinks, identity files are at
  most 1 MiB, and `Disk.img` is a multiple of 512 bytes. It clones them into a private staging folder
  and publishes it with `renamex_np(RENAME_EXCL)`, so an existing base is never replaced.
- **Destination.** The base lands in `<app data dir>/app-runner/macos/`. `macos_base_dir` builds this
  path (`runtime.rs:292-294`), and `app_runner/macos.rs` reads it.
- **Loader.** The runner starts the packaged helper, built from `Host*.swift`.
  `HostVirtualMachine.swift` (`makeMachineConfiguration`) reconstructs the platform from the four files.
  That is the loader, so no other code is needed to use an imported base.
- **Per-run copies.** Each run gets its own clone of `Disk.img` and `AuxiliaryStorage`, plus a copy of
  the identity files (`app_runner/macos.rs`). The base itself is never attached to a guest.
- **Missing-base message.** The runner asks for "a shut-down virtual Mac with its guest companion
  configured to start at login" (`runtime.rs`, `missing_message`). The companion setup below is what
  satisfies that request.

## Deployment versions (reconciled)

| Component | Built with | Runs on | Why |
| --- | --- | --- | --- |
| `Prepare.swift` (host utility) | `-target arm64-apple-macosx15.0` | Apple silicon host, macOS 15+ | Uses the xHCI controller to attach the optional tools ISO. |
| `Host*.swift` helper (host) | `build.sh`: `arm64-apple-macos13.0` | Apple silicon host, macOS 13+ | Attaching the optional project ISO needs macOS 15+. |
| Restore image (guest OS) | Apple's IPSW | guest macOS **14 or later** | The companion uses `SCScreenshotManager` (`@available(macOS 14.0)`). `Prepare.swift` refuses an older image using `operatingSystemVersion`. |
| `Guest.swift` companion (guest) | `-target arm64-apple-macos14.0` | guest macOS 14+ | Compiles for this target (checked). |

## Build and sign `Prepare.swift`

The Virtualization framework requires the `com.apple.security.virtualization` entitlement. Ad-hoc
signing is enough for local use. Use your own identity in its place if you have one.

```sh
REPO="$(git rev-parse --show-toplevel)"        # the dial-desktop checkout
OUT="$HOME/.cache/dial-offline-prep"           # build output, outside the repository
mkdir -p "$OUT"

cat > "$OUT/prepare.entitlements" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>com.apple.security.virtualization</key>
    <true/>
</dict>
</plist>
PLIST

swiftc -swift-version 5 -target arm64-apple-macosx15.0 -O \
  "$REPO/packaging/macos-guest/prepare/Prepare.swift" -o "$OUT/Prepare"
codesign --force --sign - --entitlements "$OUT/prepare.entitlements" "$OUT/Prepare"
codesign -d --entitlements - "$OUT/Prepare"    # confirm the virtualization entitlement is present
```

Argument refusals can be checked without a restore image:

```sh
"$OUT/Prepare" --ipsw /nonexistent.ipsw --dest "$HOME/x" --disk-gib 64; echo "exit=$?"   # exit=2
```

## Arguments

```text
Prepare --ipsw PATH --dest NEW_DIR --disk-gib N [--cpus N] [--memory-gib N] [--tools-iso PATH]
```

- `--ipsw`: a local `.ipsw` of at least 1 GiB, a macOS 14+ image supported by this host. No symlinks.
- `--dest`: a folder that does **not** exist. Its parent must be a real folder (no symlinks), owned by
  you, and not group- or world-writable. Use the real path, and keep it outside any project.
- `--disk-gib`: required, 64 to 4096. There is no default.
- `--cpus`, `--memory-gib`: optional. Defaults are the image minimum, up to 4 vCPUs, and 8 GiB.
- `--tools-iso`: optional non-empty `.iso`/`.cdr` up to 32 GiB, different from the IPSW. Attached read-only
  over USB during setup. It is not copied into the base.

Exit codes: `0` success, `2` refused (nothing created), `1` failure (partial preparation removed),
`130` cancelled (partial preparation removed).

The prep VM uses the same display (1280x800 at 80 ppi), keyboard and USB screen-coordinate pointer as
the runtime (`HostVirtualMachine.swift`, `MachineLayout`). It has **no network device and no vsock**,
and no shares, clipboard or serial. Only the runtime adds the vsock device, so the companion's vsock
listener cannot be exercised during preparation.

## Stage the companion onto the tools ISO (host)

The companion is compiled for arm64 on the host, but it is **never executed on the host**. It refuses
to serve unless `kern.hv_vmm_present` is 1, so do not run it to test it. Only the guest runs it.

```sh
REPO="$(git rev-parse --show-toplevel)"
STAGE="$(mktemp -d "$HOME/dial-guest-stage.XXXXXX")"
mkdir -p "$STAGE/tools"

# 1. Compile the companion for guest macOS 14+ (-parse-as-library is required, see above).
swiftc -swift-version 5 -parse-as-library -O -target arm64-apple-macos14.0 \
  "$REPO/packaging/macos-guest/Guest.swift" -o "$STAGE/tools/dial-guest"

# 2. Ad-hoc sign it. The signature identifies the binary to TCC; see "Guest TCC permissions".
codesign --force --sign - "$STAGE/tools/dial-guest"

# 3. The LaunchAgent template. Nothing loads it until the user runs the installer in the guest.
cat > "$STAGE/tools/com.dial.guest-companion.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.dial.guest-companion</string>
    <key>ProgramArguments</key>
    <array>
        <string>/Library/Application Support/Dial/dial-guest</string>
        <string>--serve</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>ThrottleInterval</key>
    <integer>30</integer>
    <key>StandardOutPath</key>
    <string>/tmp/dial-guest.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/dial-guest.log</string>
</dict>
</plist>
PLIST

# 4. The guest-side installer. It is copied onto the ISO and run by the user inside the guest.
cat > "$STAGE/tools/install-dial-companion.sh" <<'SH'
#!/bin/sh
# Run INSIDE the guest, as an admin user, from the attached DIAL_TOOLS volume:
#   sudo sh "/Volumes/DIAL_TOOLS/install-dial-companion.sh"
set -eu
here=$(cd "$(dirname "$0")" && pwd)
if [ "$(sysctl -n kern.hv_vmm_present)" != 1 ]; then
    echo "refusing: this is not a virtual machine" >&2
    exit 1
fi
(cd "$here" && shasum -a 256 -c SHA256SUMS)
printf 'Install the Dial guest companion as a login agent for this Mac? Type yes: '
read -r answer
if [ "$answer" != yes ]; then
    echo "not installed"
    exit 1
fi
dest_dir="/Library/Application Support/Dial"
mkdir -p "$dest_dir"
install -m 0755 -o root -g wheel "$here/dial-guest" "$dest_dir/dial-guest"
install -m 0644 -o root -g wheel "$here/com.dial.guest-companion.plist" \
    /Library/LaunchAgents/com.dial.guest-companion.plist
echo "installed $dest_dir/dial-guest and the LaunchAgent."
echo "Next: grant Screen Recording and Accessibility to $dest_dir/dial-guest (see the README),"
echo "then log out and back in, or run: launchctl bootstrap gui/\$(stat -f %u /dev/console) /Library/LaunchAgents/com.dial.guest-companion.plist"
SH
chmod 0755 "$STAGE/tools/install-dial-companion.sh"

# 5. Checksums for what the guest verifies before copying.
(cd "$STAGE/tools" && shasum -a 256 dial-guest com.dial.guest-companion.plist install-dial-companion.sh > SHA256SUMS)

# 6. The read-only ISO. The folder's contents become the volume root. The volume name must be
#    DIAL_TOOLS, because the guest path below depends on it.
hdiutil makehybrid -iso -joliet -default-volume-name DIAL_TOOLS \
  -o "$STAGE/dial-tools" "$STAGE/tools"
ls -l "$STAGE"        # hdiutil writes dial-tools.iso; pass that path to --tools-iso
```

The installer runs `shasum -c` against `SHA256SUMS` before copying, so a damaged copy is refused. The
ISO's own permissions are not trusted: the installer sets mode and owner with `install`.

To check the ISO from the host without running anything, mount it read-only and compare checksums:

```sh
hdiutil attach -readonly -nobrowse -mountpoint "$STAGE/mnt" "$STAGE/dial-tools.iso"
(cd "$STAGE/mnt" && shasum -a 256 -c SHA256SUMS)
hdiutil detach "$STAGE/mnt"
```

Then prepare the base with the ISO attached:

```sh
"$OUT/Prepare" --ipsw "$HOME/Downloads/UniversalMac_<version>_Restore.ipsw" \
  --dest "$HOME/Virtual Machines/macos-base/v1" --disk-gib 128 \
  --tools-iso "$STAGE/dial-tools.iso"
```

## Inside the guest (during setup, before shutting down)

Do this in the prep VM after Setup Assistant. Use a local account with no Apple ID or personal data.
The import dialog asks for a guest "with no personal accounts". Networking is not needed.

1. Open Terminal. The ISO should mount at `/Volumes/DIAL_TOOLS`. If it does not appear, open the USB
   volume in Finder.
2. Run the installer as an admin user. It asks for the password and for the explicit `yes`:
   `sudo sh "/Volumes/DIAL_TOOLS/install-dial-companion.sh"`
3. Grant the permissions in "Guest TCC permissions", then set up automatic login.
4. Verify in the guest:
   - `sysctl -n kern.hv_vmm_present` prints `1`.
   - `launchctl print "gui/$(id -u)/com.dial.guest-companion"` shows the agent after login.
   - `/tmp/dial-guest.log` contains `dial-guest listening on vsock port 49152`. The vsock device is
     attached only by the runtime, so this line is the furthest preparation can verify.
5. Shut macOS down from the Apple menu. The prep utility then verifies the four files and exits.

### Install location and opt-in

- Binary: `/Library/Application Support/Dial/dial-guest` (root:wheel, 0755). The path is fixed and does
  not depend on any user name.
- LaunchAgent: `/Library/LaunchAgents/com.dial.guest-companion.plist` (root:wheel, 0644). It loads for
  each logged-in user. It is a LaunchAgent rather than a LaunchDaemon because ScreenCaptureKit and event
  posting need the user's GUI session.
- Log: `/tmp/dial-guest.log`.
- Opt-in: the companion is installed only when the user runs the installer and types `yes`. The base
  does not start it by default. To remove it later:

```sh
sudo launchctl bootout "gui/$(stat -f %u /dev/console)" /Library/LaunchAgents/com.dial.guest-companion.plist
sudo rm -f /Library/LaunchAgents/com.dial.guest-companion.plist
sudo rm -f "/Library/Application Support/Dial/dial-guest"
```

### Automatic login (needed for a desktop)

The companion captures the desktop and posts events, so it needs a logged-in session. Without one,
ScreenCaptureKit captures nothing useful and the agent has no desktop.

1. System Settings > Users & Groups > Login Options > Automatic login: choose the base's admin user.
2. Automatic login is unavailable while FileVault is on. Keep FileVault off in this guest.
3. Recommended for a headless base (your choice): turn off the screen saver and "require password after
   sleep", and set the display to never turn off. A locked session shows no desktop.

### Guest TCC permissions

Grant these to the **installed binary** `/Library/Application Support/Dial/dial-guest`, in System
Settings > Privacy & Security, using "+" to add it. Grant nothing else.

- **Screen & System Audio Recording** (named "Screen Recording" on macOS 14). The companion checks it
  with `CGPreflightScreenCaptureAccess`.
- **Accessibility**, for event posting. The companion checks `CGPreflightPostEventAccess`. Confirm which
  pane lists the binary on your guest's macOS version. This is not verified.

Notes:

- The grants live in the guest's TCC database inside `Disk.img`, so they travel with the base.
- Rebuilding the binary changes its ad-hoc signature, so TCC no longer matches it. Remove the entries,
  add the new binary again, and restart the agent with
  `launchctl kickstart -k "gui/$(id -u)/com.dial.guest-companion"`.
- The companion never prompts for permissions. A missing grant is reported to the host as an error reply.
- No privacy permission is needed on the host.

## Cancel

- Closing the VM window, or Ctrl-C (SIGINT or SIGTERM), before the guest shuts down cancels. The utility
  stops the VM and deletes only the four files it created, then the folder if it is empty. It says
  "removed" only when every removal succeeded. If a removal fails, it lists what was left in place.
- A SIGKILL or crash leaves the partial folder. Delete it yourself. The utility never recurses into a
  folder it did not create.
- Only the setup VM's clean guest shutdown, after the installer has finished, can publish success. A stop
  during the installer run is ignored, and the installer's own result decides that phase.

## Import

1. Shut the prep VM down from the Apple menu. The utility exits `0` and prints the four paths.
2. In Dial, use the import action for a shut-down virtual Mac and choose the folder that contains exactly
   those four files. The dialog repeats the shut-down and "no personal accounts" guidance.
3. Dial publishes the base to `<app data dir>/app-runner/macos/`. The runner clones it for each run.

Keep the installed base's `HardwareModel` and `MachineIdentifier` together. Do not run two copies of the same virtual Mac at the same time.

## Status

- Verified: `Prepare.swift` compiles with the targets above from its final folder, and its argument
  refusals were checked there. `Guest.swift` compiles for `arm64-apple-macos14.0` with
  `-parse-as-library` (compiled to a temporary directory only, never run).
- Not verified, and not run: the installer, Setup Assistant, the shutdown path, cancel cleanup, the ISO
  mount name, the guest TCC panes, and the companion's behaviour in a guest. Treat them as unverified
  until someone runs them on a real restore image.
- The companion, its installer and the LaunchAgent are user-installed and user-opted-in. The utility
  does not install them.
