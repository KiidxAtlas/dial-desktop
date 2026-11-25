#!/usr/bin/env node
// Browser UI preview: writes the Rust design-preview fixtures as JSON, then serves the real UI
// with a stand-in backend (`dev/ui-preview.html`). Quicker than a Tauri build for layout work,
// and any browser (or a test runner) can resize it freely. `--fixtures` only writes the fixtures.
import { spawn, spawnSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(root, ".tmp", "ui-preview");
mkdirSync(out, { recursive: true });

const fixtures = spawnSync("cargo", ["run", "--quiet", "--example", "ui_preview_fixtures"], {
  cwd: join(root, "src-tauri"),
  env: { ...process.env, OUT: out },
  stdio: "inherit",
});
if (fixtures.status !== 0) {
  console.error("Could not write the UI preview fixtures (cargo run --example ui_preview_fixtures).");
  process.exit(fixtures.status ?? 1);
}
if (process.argv.includes("--fixtures")) process.exit(0);

const url = "http://localhost:1420/dev/ui-preview.html";
console.log(`\nUI preview: ${url}`);
console.log("  ?state=session|idle|approvals|long|offline|empty|question  &theme=light|dark  &check\n");
const vite = spawn(join(root, "node_modules", ".bin", "vite"), [], { cwd: root, stdio: "inherit" });
vite.on("exit", (code) => process.exit(code ?? 0));
for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, () => vite.kill(signal));
