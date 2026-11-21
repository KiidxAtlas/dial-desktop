// Cross-platform entry for Tauri's beforeBuildCommand. It follows the Tauri *target*, not the host:
//   * TAURI_ENV_TARGET_TRIPLE (set by `tauri build`/`tauri dev`) decides whether the arm64 macOS helper is
//     needed: aarch64-apple-darwin and universal-apple-darwin build it; any other target does not.
//   * Without the variable (a direct `node packaging/macos-guest/build.mjs` run) the host decides.
// Non-Mac targets have no helper resource mapping. Leave their staging area alone.
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const staged = path.join(here, "..", "..", "src-tauri", "resources", "macos-guest");
const triple = process.env.TAURI_ENV_TARGET_TRIPLE;
const needsHelper = triple
    ? triple === "aarch64-apple-darwin" || triple === "universal-apple-darwin"
    : process.platform === "darwin" && process.arch === "arm64";

if (!needsHelper) {
    console.log(`macos-guest: skipped for target ${triple ?? `${process.platform}/${process.arch}`}`);
    process.exit(0);
}
if (process.platform !== "darwin") {
    console.error(`macos-guest: target ${triple} needs the macOS helper but this host is ${process.platform}`);
    process.exit(1);
}
const result = spawnSync("sh", [path.join(here, "build.sh")], { stdio: "inherit" });
if (result.error) {
    console.error(`macos-guest: cannot run build.sh: ${result.error.message}`);
    process.exit(1);
}
const outputs = ["dial-mac-guest", "dial-mac-prepare", "dial-tools.iso"];
if (result.status !== 0 || !outputs.every((name) => existsSync(path.join(staged, name)))) {
    console.error("macos-guest: helper was not built");
    process.exit(1);
}
process.exit(0);
