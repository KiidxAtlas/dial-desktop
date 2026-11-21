#!/usr/bin/env node
// Stage or verify the optional QEMU runtime that Dial Desktop bundles as Tauri resources.
//
//   node packaging/runtime/stage-runtime.mjs --target <triple> --from <bundle-dir> [--out <dir>] [--force]
//   node packaging/runtime/stage-runtime.mjs --target <triple> --check <bundle-dir>
//
// Nothing is downloaded and nothing is executed. The bundle must already exist on disk, built
// and licensed as described in docs/app-runtime-packaging.md.

import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { RuntimeBundleError, stageRuntime, verifyRuntimeDir } from "./runtime-bundle.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const DEFAULT_OUT = path.resolve(here, "../../src-tauri/resources/qemu");

export function parseArgs(argv) {
  const options = { target: undefined, from: undefined, out: DEFAULT_OUT, force: false, check: false };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const value = () => {
      const next = argv[i + 1];
      if (next === undefined || next.startsWith("--")) throw new RuntimeBundleError(`${arg} needs a value`);
      i += 1;
      return next;
    };
    if (arg === "--target") options.target = value();
    else if (arg === "--from") options.from = value();
    else if (arg === "--check") {
      options.check = true;
      options.from = value();
    } else if (arg === "--out") options.out = path.resolve(value());
    else if (arg === "--force") options.force = true;
    else throw new RuntimeBundleError(`unknown argument ${arg}`);
  }
  if (!options.target) throw new RuntimeBundleError("--target is required (no host default: release builds are explicit)");
  if (!options.from) throw new RuntimeBundleError("--from <bundle-dir> is required");
  return options;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.check) {
    const manifest = await verifyRuntimeDir(path.resolve(options.from), { target: options.target });
    console.log(`runtime ok: ${manifest.target}, ${manifest.files.length} files, qemu ${manifest.qemu.version}`);
    return;
  }
  const result = await stageRuntime({
    from: options.from,
    target: options.target,
    out: options.out,
    force: options.force,
  });
  console.log(`staged ${result.files} files (${result.bytes} bytes) for ${result.target} into ${result.out}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main().catch((error) => {
    const label = error instanceof RuntimeBundleError ? "runtime packaging" : "runtime packaging (unexpected)";
    console.error(`${label}: ${error.message}`);
    process.exitCode = 1;
  });
}
