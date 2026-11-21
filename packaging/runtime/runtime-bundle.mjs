// Validation for the optional, self-contained QEMU runtime that Dial Desktop can ship as Tauri
// resources. Dependency-free on purpose: it runs on the release runners with the Node that
// `actions/setup-node` already installs.
//
// A runtime bundle is a directory with a `runtime.json` manifest at its root. The manifest lists
// every other regular file in the directory. Verification rejects symlinks and other special
// files, undeclared or missing files, hash or size drift, unsafe relative paths, and wrong
// executable bits. Nothing here executes the runtime or consults PATH.
//
// Keep the manifest contract in sync with docs/app-runtime-packaging.md and with the Rust reader
// in src-tauri/src/app_runner/runtime.rs.

import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import fs from "node:fs/promises";
import path from "node:path";

export const SCHEMA_VERSION = 1;
export const MANIFEST_NAME = "runtime.json";

// Exactly the host triples of the release matrix (.github/workflows/release.yml). A new triple
// needs a matrix entry as well as a line here.
export const TARGETS = Object.freeze({
  "aarch64-apple-darwin": Object.freeze({ exe: false }),
  "x86_64-apple-darwin": Object.freeze({ exe: false }),
  "x86_64-pc-windows-msvc": Object.freeze({ exe: true }),
  "x86_64-unknown-linux-gnu": Object.freeze({ exe: false }),
});

// Top-level directories a runtime may populate. Everything else is refused.
export const ALLOWED_ROOTS = Object.freeze(["bin", "lib", "share", "licenses"]);

const TOP_KEYS = ["schema", "target", "binary", "qemu", "files"];
const QEMU_KEYS = ["version", "sourceUrl", "sourceSha256"];
const FILE_KEYS = ["path", "sha256", "size", "executable"];

export class RuntimeBundleError extends Error {
  constructor(message) {
    super(message);
    this.name = "RuntimeBundleError";
  }
}

const fail = (message) => {
  throw new RuntimeBundleError(message);
};

const isPosix = () => process.platform !== "win32";

export function targetInfo(target) {
  if (!Object.hasOwn(TARGETS, target)) {
    fail(`unsupported target "${target}"; expected one of ${Object.keys(TARGETS).join(", ")}`);
  }
  return TARGETS[target];
}

// The only binary a target may ship: qemu-system-<arch> for the target's own architecture.
export function expectedBinary(target) {
  const arch = target.split("-")[0];
  return `bin/qemu-system-${arch}${targetInfo(target).exe ? ".exe" : ""}`;
}

// UEFI images the app's runner loads for the target's architecture. The names are the ones
// `Arch::firmware_code` and `Arch::firmware_vars` return in src-tauri/src/app_runner/runtime.rs;
// both must be bundled, under share/.
export function requiredFirmware(target) {
  const arch = target.split("-")[0];
  return [`share/edk2-${arch}-code.fd`, `share/edk2-${arch}-vars.fd`];
}

// Windows device names. A segment whose stem (text before the first dot) is one of these is
// reserved on Windows whatever its extension, e.g. "nul.txt". Compared case-insensitively.
const WINDOWS_RESERVED = /^(CON|PRN|AUX|NUL|COM[0-9\u00b9\u00b2\u00b3]|LPT[0-9\u00b9\u00b2\u00b3])$/i;
// Characters Windows refuses in a file name, plus control characters. ":" also covers
// alternate data streams ("file:stream") and drive prefixes.
const WINDOWS_FORBIDDEN = /[<>:"|?*\u0000-\u001f]/;

// A manifest path: forward slashes, relative, no empty/"."/".." segments, no drive letters, no
// backslashes, no NUL, and rooted in one of ALLOWED_ROOTS with at least one segment below it.
export function checkRelativePath(p, label = "path") {
  if (typeof p !== "string" || p.length === 0) fail(`${label}: must be a non-empty string`);
  if (p.includes("\\")) fail(`${label}: "${p}" uses a backslash; use forward slashes`);
  if (p.includes("\0")) fail(`${label}: "${p}" contains a NUL byte`);
  if (p.startsWith("/") || /^[A-Za-z]:/.test(p)) fail(`${label}: "${p}" is absolute`);
  const segments = p.split("/");
  for (const segment of segments) {
    if (segment === "" || segment === "." || segment === "..") {
      fail(`${label}: "${p}" has an empty, "." or ".." segment`);
    }
    // These paths are written on Windows too, so refuse what Windows cannot store faithfully.
    if (WINDOWS_FORBIDDEN.test(segment)) {
      fail(`${label}: "${p}" has a character Windows forbids (including ":" for alternate data streams)`);
    }
    if (segment.endsWith(".") || segment.endsWith(" ")) {
      fail(`${label}: "${p}" has a segment ending in a dot or space, which Windows strips`);
    }
    if (WINDOWS_RESERVED.test(segment.split(".")[0])) {
      fail(`${label}: "${p}" uses the Windows device name "${segment.split(".")[0]}"`);
    }
  }
  if (segments.length < 2 || !ALLOWED_ROOTS.includes(segments[0])) {
    fail(`${label}: "${p}" must live under ${ALLOWED_ROOTS.join(", ")}/`);
  }
  return p;
}

function requireExactKeys(obj, keys, label) {
  if (obj === null || typeof obj !== "object" || Array.isArray(obj)) {
    fail(`${label} must be an object`);
  }
  for (const key of Object.keys(obj)) {
    if (!keys.includes(key)) fail(`${label}: unknown key "${key}"`);
  }
  for (const key of keys) {
    if (!Object.hasOwn(obj, key)) fail(`${label}: missing "${key}"`);
  }
}

const isSha256 = (value) => typeof value === "string" && /^[0-9a-f]{64}$/.test(value);

// Parses and checks a manifest's shape and invariants. Returns a canonical copy (files sorted by
// path) so that staging writes byte-identical output for the same input.
export function parseManifest(text, { expectedTarget } = {}) {
  let raw;
  try {
    raw = JSON.parse(text);
  } catch (error) {
    fail(`${MANIFEST_NAME} is not valid JSON: ${error.message}`);
  }
  requireExactKeys(raw, TOP_KEYS, MANIFEST_NAME);
  if (raw.schema !== SCHEMA_VERSION) {
    fail(`${MANIFEST_NAME}: schema must be ${SCHEMA_VERSION}, got ${JSON.stringify(raw.schema)}`);
  }
  if (typeof raw.target !== "string") fail(`${MANIFEST_NAME}: target must be a string`);
  targetInfo(raw.target);
  if (expectedTarget !== undefined && raw.target !== expectedTarget) {
    fail(`${MANIFEST_NAME}: built for "${raw.target}", expected "${expectedTarget}"`);
  }
  const binary = expectedBinary(raw.target);
  if (raw.binary !== binary) {
    fail(`${MANIFEST_NAME}: binary must be "${binary}" for ${raw.target}, got ${JSON.stringify(raw.binary)}`);
  }

  requireExactKeys(raw.qemu, QEMU_KEYS, `${MANIFEST_NAME}: qemu`);
  if (typeof raw.qemu.version !== "string" || !/^[0-9][0-9A-Za-z.+~-]{0,63}$/.test(raw.qemu.version)) {
    fail(`${MANIFEST_NAME}: qemu.version must be a short version string`);
  }
  if (typeof raw.qemu.sourceUrl !== "string" || !raw.qemu.sourceUrl.startsWith("https://")) {
    fail(`${MANIFEST_NAME}: qemu.sourceUrl must be an https:// URL of the exact QEMU source`);
  }
  if (!isSha256(raw.qemu.sourceSha256)) {
    fail(`${MANIFEST_NAME}: qemu.sourceSha256 must be 64 lowercase hex characters`);
  }

  if (!Array.isArray(raw.files) || raw.files.length === 0) {
    fail(`${MANIFEST_NAME}: files must be a non-empty array`);
  }
  const seen = new Set();
  const files = raw.files.map((entry, index) => {
    const label = `${MANIFEST_NAME}: files[${index}]`;
    requireExactKeys(entry, FILE_KEYS, label);
    checkRelativePath(entry.path, label);
    if (!isSha256(entry.sha256)) fail(`${label}: sha256 must be 64 lowercase hex characters`);
    if (!Number.isSafeInteger(entry.size) || entry.size < 0) {
      fail(`${label}: size must be a non-negative integer`);
    }
    if (typeof entry.executable !== "boolean") fail(`${label}: executable must be a boolean`);
    const folded = entry.path.toLowerCase();
    if (seen.has(folded)) fail(`${label}: "${entry.path}" appears twice (case-insensitively)`);
    seen.add(folded);
    return { path: entry.path, sha256: entry.sha256, size: entry.size, executable: entry.executable };
  });

  const byPath = new Map(files.map((file) => [file.path, file]));
  const binaryEntry = byPath.get(binary);
  if (!binaryEntry) fail(`${MANIFEST_NAME}: the binary "${binary}" is not listed in files`);
  if (!binaryEntry.executable) fail(`${MANIFEST_NAME}: "${binary}" must be marked executable`);
  for (const firmware of requiredFirmware(raw.target)) {
    if (!byPath.has(firmware)) fail(`${MANIFEST_NAME}: the firmware "${firmware}" is not listed in files`);
  }
  if (!files.some((file) => file.path.startsWith("licenses/"))) {
    fail(`${MANIFEST_NAME}: no license file under licenses/ is listed; QEMU's license texts must ship with it`);
  }

  files.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
  return {
    schema: SCHEMA_VERSION,
    target: raw.target,
    binary,
    qemu: {
      version: raw.qemu.version,
      sourceUrl: raw.qemu.sourceUrl,
      sourceSha256: raw.qemu.sourceSha256,
    },
    files,
  };
}

// Canonical bytes for a parsed manifest; the same input always produces the same file.
export function canonicalManifestText(manifest) {
  return `${JSON.stringify(manifest, null, 2)}\n`;
}

async function sha256File(file) {
  const hash = createHash("sha256");
  await new Promise((resolve, reject) => {
    createReadStream(file)
      .on("data", (chunk) => hash.update(chunk))
      .on("error", reject)
      .on("end", resolve);
  });
  return hash.digest("hex");
}

// Lists every regular file under `root` (relative, forward slashes). Symlinks and other special
// files fail the walk; directories are only descended into, never followed.
async function listRegularFiles(root) {
  const out = [];
  async function walk(dir, prefix) {
    const entries = await fs.readdir(dir, { withFileTypes: true });
    for (const entry of entries) {
      const rel = prefix ? `${prefix}/${entry.name}` : entry.name;
      const full = path.join(dir, entry.name);
      if (entry.isSymbolicLink()) fail(`"${rel}" is a symbolic link; runtime bundles must not contain links`);
      if (entry.isDirectory()) {
        await walk(full, rel);
      } else if (entry.isFile()) {
        out.push(rel);
      } else {
        fail(`"${rel}" is not a regular file or directory`);
      }
    }
  }
  await walk(root, "");
  return out.sort();
}

async function assertRealDirectory(dir, label) {
  let stat;
  try {
    stat = await fs.lstat(dir);
  } catch {
    fail(`${label} "${dir}" does not exist`);
  }
  if (stat.isSymbolicLink()) fail(`${label} "${dir}" is a symbolic link`);
  if (!stat.isDirectory()) fail(`${label} "${dir}" is not a directory`);
}

// Verifies a runtime directory against its own manifest. Returns the parsed manifest.
export async function verifyRuntimeDir(dir, { target } = {}) {
  await assertRealDirectory(dir, "runtime directory");
  const manifestPath = path.join(dir, MANIFEST_NAME);
  let text;
  try {
    text = await fs.readFile(manifestPath, "utf8");
  } catch {
    fail(`${MANIFEST_NAME} is missing from "${dir}"`);
  }
  const manifest = parseManifest(text, { expectedTarget: target });

  const present = (await listRegularFiles(dir)).filter((rel) => rel !== MANIFEST_NAME);
  const declared = new Set(manifest.files.map((file) => file.path));
  const undeclared = present.filter((rel) => !declared.has(rel));
  if (undeclared.length) fail(`undeclared file(s) in runtime: ${undeclared.join(", ")}`);
  const presentSet = new Set(present);
  const missing = manifest.files.map((file) => file.path).filter((rel) => !presentSet.has(rel));
  if (missing.length) fail(`file(s) listed in the manifest but absent: ${missing.join(", ")}`);

  for (const file of manifest.files) {
    const full = path.join(dir, ...file.path.split("/"));
    const stat = await fs.lstat(full);
    if (stat.size !== file.size) {
      fail(`"${file.path}": size ${stat.size} does not match the manifest (${file.size})`);
    }
    if (isPosix()) {
      const executable = (stat.mode & 0o111) !== 0;
      if (file.executable && !executable) fail(`"${file.path}" must be executable`);
      if (!file.executable && executable) fail(`"${file.path}" is executable but not declared so`);
    }
    const digest = await sha256File(full);
    if (digest !== file.sha256) fail(`"${file.path}": sha256 does not match the manifest`);
  }
  return manifest;
}

// Copies a verified bundle into `out` through a sibling temporary directory, re-verifies the
// copy, then swaps it into place. A previous staging (a directory with runtime.json) may be
// replaced with `force`; any other existing content is never removed.
export async function stageRuntime({ from, target, out, force = false }) {
  targetInfo(target);
  const source = path.resolve(from);
  const destination = path.resolve(out);
  if (source === destination) fail("source and output are the same directory");
  const manifest = await verifyRuntimeDir(source, { target });

  const parent = path.dirname(destination);
  await fs.mkdir(parent, { recursive: true });
  await assertRealDirectory(parent, "output parent");

  let existing = null;
  try {
    existing = await fs.lstat(destination);
  } catch {
    existing = null;
  }
  if (existing) {
    if (existing.isSymbolicLink()) fail(`output "${destination}" is a symbolic link`);
    if (!existing.isDirectory()) fail(`output "${destination}" exists and is not a directory`);
    const entries = await fs.readdir(destination);
    if (entries.length > 0) {
      const previous = entries.includes(MANIFEST_NAME);
      if (!previous) fail(`output "${destination}" is not empty and was not created by staging`);
      if (!force) fail(`output "${destination}" already holds a staged runtime; pass --force to replace it`);
    }
  }

  const temp = `${destination}.partial-${process.pid}`;
  await fs.rm(temp, { recursive: true, force: true });
  await fs.mkdir(temp, { recursive: false });
  try {
    for (const file of manifest.files) {
      const from = path.join(source, ...file.path.split("/"));
      const to = path.join(temp, ...file.path.split("/"));
      await fs.mkdir(path.dirname(to), { recursive: true });
      await fs.copyFile(from, to);
      if (isPosix()) await fs.chmod(to, file.executable ? 0o755 : 0o644);
    }
    await fs.writeFile(path.join(temp, MANIFEST_NAME), canonicalManifestText(manifest));
    await verifyRuntimeDir(temp, { target });
    if (existing) await fs.rm(destination, { recursive: true, force: true });
    await fs.rename(temp, destination);
  } catch (error) {
    await fs.rm(temp, { recursive: true, force: true });
    throw error;
  }

  return {
    out: destination,
    target,
    binary: manifest.binary,
    files: manifest.files.length,
    bytes: manifest.files.reduce((sum, file) => sum + file.size, 0),
  };
}
