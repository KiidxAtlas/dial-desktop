#!/bin/sh
# Build and sign the Apple silicon host helper (dial-mac-guest), the virtual Mac prep tool
# (dial-mac-prepare) and the guest companion disc (dial-tools.iso). Host*.swift only goes into the host
# helper: Guest.swift is the guest-side companion; it is compiled onto the disc and never run on the host.
# Output defaults to src-tauri/resources/macos-guest/, bundled into the app as resource_dir/macos-guest/
# (src-tauri/src/app_runner/runtime.rs HELPER_DIR/HELPER_NAME, app_runner/host.rs PREPARE/TOOLS_ISO).
# Signing: ad hoc by default (local runs); DIAL_GUEST_SIGN_IDENTITY=<Developer ID> signs with hardened
# runtime and a secure timestamp. Nothing here downloads, installs or reads credentials.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
out=${1:-"$root/src-tauri/resources/macos-guest/dial-mac-guest"}
dir=$(dirname "$out")
identity=${DIAL_GUEST_SIGN_IDENTITY:--}
mkdir -p "$dir"

sign() {
    if [ "$identity" = "-" ]; then
        codesign --force --sign - --entitlements "$here/Host.entitlements" "$1"
    else
        codesign --force --options runtime --timestamp --sign "$identity" --entitlements "$here/Host.entitlements" "$1"
    fi
    codesign --verify --strict "$1"
}

swiftc -parse-as-library -O -swift-version 5 -target arm64-apple-macos13.0 -framework Virtualization \
    "$here"/Host*.swift -o "$out"
sign "$out"
echo "built and signed $out"

# The prep tool installs macOS into a new virtual Mac (App panel → Install). It needs macOS 15 for the
# USB disc that carries the companion.
swiftc -swift-version 5 -O -target arm64-apple-macosx15.0 "$here/prepare/Prepare.swift" -o "$dir/dial-mac-prepare"
sign "$dir/dial-mac-prepare"
echo "built and signed $dir/dial-mac-prepare"

# The DIAL_TOOLS disc attached during preparation: the companion, its LaunchAgent and its installer,
# as described in prepare/README.md. The user runs the installer inside the guest.
stage=$(mktemp -d "${TMPDIR:-/tmp}/dial-tools.XXXXXX")
trap 'rm -rf "$stage"' EXIT
mkdir "$stage/tools"
swiftc -swift-version 5 -parse-as-library -O -target arm64-apple-macos14.0 \
    "$here/Guest.swift" -o "$stage/tools/dial-guest"
codesign --force --sign - "$stage/tools/dial-guest"
cat > "$stage/tools/com.dial.guest-companion.plist" <<'PLIST'
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
cat > "$stage/tools/install-dial-companion.sh" <<'SH'
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
echo "Next: grant Screen Recording and Accessibility to $dest_dir/dial-guest,"
echo "turn on automatic login, then shut this Mac down from the Apple menu."
SH
chmod 0755 "$stage/tools/install-dial-companion.sh"
(cd "$stage/tools" && shasum -a 256 dial-guest com.dial.guest-companion.plist install-dial-companion.sh > SHA256SUMS)
rm -f "$dir/dial-tools.iso"
hdiutil makehybrid -quiet -iso -joliet -default-volume-name DIAL_TOOLS -o "$dir/dial-tools.iso" "$stage/tools"
echo "built $dir/dial-tools.iso"
