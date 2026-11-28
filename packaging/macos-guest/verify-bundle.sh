#!/bin/sh
# Verify that a built Dial .app carries the macOS host helper at its fixed resource path, signed with the
# virtualization entitlement, and that no guest companion or guest OS media is inside the bundle.
#
# Usage: packaging/macos-guest/verify-bundle.sh path/to/Dial.app [--with-engine]
#
# Read-only: it never runs the helper, a VM or any installer. Exits 0 when every check holds, 1 otherwise.
set -eu

app=${1:?usage: verify-bundle.sh path/to/Dial.app}
helper="$app/Contents/Resources/macos-guest/dial-mac-guest"
macos_dir="$app/Contents/MacOS"
fail() { echo "verify-bundle: FAIL: $*" >&2; exit 1; }
case "${2:-}" in
	'') expected_count=1 ;;
	--with-engine) expected_count=2 ;;
	*) fail "unknown option: $2" ;;
esac

[ -d "$app" ] || fail "not an app bundle: $app"

# 1. helper at the fixed resource path, regular file, executable, arm64 Mach-O
[ -f "$helper" ] && [ ! -L "$helper" ] || fail "missing helper at Contents/Resources/macos-guest/dial-mac-guest"
[ -x "$helper" ] || fail "helper is not executable"
case "$(file -b "$helper")" in
	*"Mach-O 64-bit executable arm64"*) ;;
	*) fail "helper is not a Mach-O arm64 executable: $(file -b "$helper")" ;;
esac

# 2. helper signature verifies, and it carries com.apple.security.virtualization = true
codesign --verify --strict "$helper" 2>/dev/null || fail "helper signature does not verify"
helper_ent=$(codesign -d --entitlements :- "$helper" 2>/dev/null | tr -d ' \n\t')
case "$helper_ent" in
	*"<key>com.apple.security.virtualization</key><true/>"*) ;;
	*) fail "helper lacks com.apple.security.virtualization = true" ;;
esac

# 3. Contents/MacOS holds the main executable and, for release builds, the engine sidecar.
[ -d "$macos_dir" ] || fail "missing Contents/MacOS"
if [ "$expected_count" = 2 ]; then
	engine="$macos_dir/dial"
	[ -f "$engine" ] && [ ! -L "$engine" ] && [ -x "$engine" ] || fail "missing executable engine sidecar at Contents/MacOS/dial"
fi
count=$(find "$macos_dir" -mindepth 1 -maxdepth 1 | wc -l | tr -d ' ')
[ "$count" = "$expected_count" ] || fail "expected $expected_count entries in Contents/MacOS, found $count"
if [ "$expected_count" = 2 ]; then
	main=$(find "$macos_dir" -mindepth 1 -maxdepth 1 ! -name dial)
else
	main=$(find "$macos_dir" -mindepth 1 -maxdepth 1)
fi
[ -f "$main" ] && [ ! -L "$main" ] && [ -x "$main" ] || fail "main entry in Contents/MacOS is not a regular executable"
case "$(basename "$main")" in
	*guest*|*Guest*) fail "guest helper or companion baked into Contents/MacOS as $(basename "$main")" ;;
esac
if codesign -d --entitlements :- "$main" 2>/dev/null | grep -q "com.apple.security.virtualization"; then
	fail "the host app executable must not carry the virtualization entitlement"
fi

# 4. no guest companion and no guest OS media anywhere in the bundle
if find "$app" \( -name 'dial-guest*' -o -name 'Disk.img' -o -name 'AuxiliaryStorage' \
	-o -name 'HardwareModel' -o -name 'MachineIdentifier' -o -name '*.ipsw' \) | grep -q .; then
	fail "guest companion or guest OS media found inside the bundle"
fi

echo "verify-bundle: ok: $app (helper at Contents/Resources/macos-guest/dial-mac-guest, arm64, entitled; no guest companion or OS media)"
