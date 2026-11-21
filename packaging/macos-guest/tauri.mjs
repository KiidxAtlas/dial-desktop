// Tauri CLI entry (package.json "tauri" script and the Makefile). For `tauri build` whose target is
// arm64 macOS (host default or --target aarch64-apple-darwin / universal-apple-darwin) it adds
// src-tauri/tauri.macos-guest.conf.json, which bundles the host helper. Every other invocation is passed
// through unchanged, so non-arm64 builds never see the helper resource (Tauri validates it at compile time).
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const here = path.dirname(fileURLToPath(import.meta.url));
const helperConfig = path.join(here, "..", "..", "src-tauri", "tauri.macos-guest.conf.json");
const args = process.argv.slice(2);

let target = null;
for (let i = 0; i < args.length; i++) {
    if ((args[i] === "--target" || args[i] === "-t") && i + 1 < args.length) target = args[i + 1];
    else if (args[i].startsWith("--target=")) target = args[i].slice("--target=".length);
}
target ??= process.env.CARGO_BUILD_TARGET || null;
if (!target && process.platform === "darwin" && process.arch === "arm64") target = "aarch64-apple-darwin";

const bundlesHelper = args[0] === "build" && (target === "aarch64-apple-darwin" || target === "universal-apple-darwin");
const extra = bundlesHelper ? ["--config", helperConfig] : [];
const cli = createRequire(import.meta.url).resolve("@tauri-apps/cli/tauri.js");
const result = spawnSync(process.execPath, [cli, ...args, ...extra], { stdio: "inherit" });
if (result.error) {
    console.error(`tauri wrapper: ${result.error.message}`);
    process.exit(1);
}
process.exit(result.status ?? 1);
