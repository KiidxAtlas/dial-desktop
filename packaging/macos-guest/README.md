# macOS guest companion (`Guest.swift`)

`Guest.swift` captures the display and delivers input **inside a prepared macOS virtual machine**. It does not capture the host, send host input, or fall back to the host desktop. Dial does not download a guest OS or grant privacy permissions.

## Refusals (fail closed)

`dial-guest --serve` exits before opening any socket unless all of these hold:

- `sysctl kern.hv_vmm_present` reports `1` (a virtual machine). A host Mac reports `0`.
- The vsock local CID (`IOCTL_VM_SOCKETS_GET_LOCAL_CID`) is a guest CID, `3` or more. The host
  (`2`), the hypervisor (`0`) and the reserved CID (`1`) are refused.

Any other invocation prints a usage line and exits with status 2.

## Transport

- `AF_VSOCK` (`40`), port `49152`, bound to the guest's own CID.
- Peers are accepted only from the host CID (`2`). Other peers are closed at once.
- No TCP or UDP listener, no shell, and no filesystem API is exposed to the peer.
- One connection is served at a time. A 30-second idle read timeout applies.

## Wire protocol

Each message, in both directions, is a `uint32` big-endian length followed by the body.

Requests are JSON objects, at most 64 KiB:

| Request body | Meaning |
| --- | --- |
| `{"execute":"screendump"}` | Capture the main display. |
| `{"execute":"input-send-event","arguments":{"events":[...]}}` | Post one gesture (at most 16 events). |

Replies:

| Reply body | Meaning |
| --- | --- |
| `0x00` + binary P6 PPM | `screendump` succeeded. |
| `0x00` (one byte, length `1`) | `input-send-event` succeeded. Golden frame: `[0,0,0,1,0]`. |
| `0x01` + UTF-8 message (at most 512 bytes) | Any refusal or failure. |

The events are the ones the host's QEMU-compatible translator emits (`src-tauri/src/app_runner/input.rs`):

- A click is exactly `abs x`, `abs y`, `btn <button> down`, `btn <button> up`.
- A key gesture is modifier downs, key down, key up, then modifier ups in reverse. Modifiers are
  distinct, come from `shift`, `ctrl`, `alt` and `meta_l`, and are at most four.

Every gesture is parsed and every macOS event is built before the first event is posted. A malformed
gesture posts nothing. Keys are mapped by QEMU qcode to macOS virtual key codes; `insert` has no
macOS equivalent and is refused.

Pointer values `0...32767` map onto the main display in points, the same as the host's tablet scaling.

## Guest prerequisites

These are the guest's responsibility. The companion does not prompt for them.

- macOS 14 or later (ScreenCaptureKit's `SCScreenshotManager`).
- **Screen Recording** granted to the built binary in the guest's System Settings. A missing grant
  is reported to the host as an error reply.
- Event posting allowed for the binary in the guest (`CGPreflightPostEventAccess`). A missing grant
  is reported the same way.

## Build

Compile with the guest's Xcode command line tools, or cross-build on the host and transfer the binary on the read-only tools ISO described in the [offline preparation guide](prepare/README.md). Execute it only inside the guest.

```sh
swiftc -swift-version 5 -parse-as-library -O Guest.swift -o dial-guest
```

`-parse-as-library` is required because the entry point is a `@main` type.

## Execution: guest only

Run `dial-guest --serve` only inside a prepared macOS guest VM, started by the user there. The
companion refuses to serve unless the kernel reports a virtual machine and the vsock CID is a guest
CID. The host Mac never runs it.

To create a base, see [preparing a virtual Mac](prepare/README.md). The optional preparation utility uses a user-supplied local IPSW.

## Host-side checks of the pure code

These checks run on the host. They do not start the companion, touch the VM, vsock, Screen Recording
or event posting. They compile the same file with `DIAL_GUEST_TESTS`, which removes the `@main`
entry point:

```sh
swiftc -swift-version 5 -D DIAL_GUEST_TESTS Guest.swift tests/guest/main.swift -o /tmp/dial-guest-tests
/tmp/dial-guest-tests
```

`tests/main.swift` and `tests/run.sh` are the host helper suite and are separate from this guest suite.

They cover request and gesture validation, the key table, coordinate mapping, PPM encoding, error
bounding, and the framed serve loop over a socketpair with a fake backend, including the golden input
reply `[0,0,0,1,0]`.

Not covered by these checks: the vsock listener, the CID checks, ScreenCaptureKit capture, and event
posting. Those need a prepared macOS guest VM and are unverified until run there.
