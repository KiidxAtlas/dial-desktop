#!/bin/sh
# Build and run the host helper tests on this Mac (no Virtualization framework, no guest).
# Compile the actual companion serve loop with only its production entry point disabled.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(dirname "$here")
work=$(mktemp -d "${TMPDIR:-/tmp}/dial-host-tests.XXXXXX")
trap 'rm -rf "$work"' EXIT
swiftc -swift-version 5 -D DIAL_GUEST_TESTS -target arm64-apple-macos13.0 \
    "$root/HostErrors.swift" "$root/HostProtocol.swift" "$root/HostGuest.swift" \
    "$root/HostMailbox.swift" "$root/HostSession.swift" "$root/HostArguments.swift" \
    "$root/Guest.swift" "$here/main.swift" -o "$work/host-tests"
"$work/host-tests"
