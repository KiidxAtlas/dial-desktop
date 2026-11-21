# Build the private virtual Mac backend

From the Dial Desktop checkout, with the JS dependencies, Rust toolchain and Xcode command-line tools already installed on an Apple Silicon Mac:

```sh
npm run build -- --debug --bundles app
# Use the Dial.app path printed by Tauri:
sh packaging/macos-guest/verify-bundle.sh /path/to/Dial.app
```

`npm run build` selects `src-tauri/tauri.macos-guest.conf.json` for arm64 macOS builds. The before-build hook compiles `Host*.swift` and signs `Contents/Resources/macos-guest/dial-mac-guest` with `com.apple.security.virtualization`. Only the helper gets this entitlement. Direct Tauri CLI callers building for arm64 must pass `--config src-tauri/tauri.macos-guest.conf.json` themselves. This overlay can coexist with the [QEMU runtime overlay](../../docs/app-runtime-packaging.md).

Local builds use an ad-hoc helper signature. Distribution requires an installed signing identity (`DIAL_GUEST_SIGN_IDENTITY`), the normal app signing and notarization process, and verification of the **final** signed bundle. A successful local debug build is not a distribution-signing test.

The helper requires macOS 13+ on Apple Silicon; attaching a read-only project ISO requires macOS 15+. The guest OS, guest companion and user data are not in the bundle. Before using **App → macOS**, [prepare and import a private virtual Mac](prepare/README.md). It must have a logged-in desktop and the [companion](README.md) configured inside the guest. Dial never substitutes the host desktop when these are missing.

`tests/run.sh` checks host protocol handling without booting a guest. For a live acceptance test, open an app **inside** the virtual Mac, send a click and key through the sidebar, then Stop and start again to verify the original base is unchanged.
