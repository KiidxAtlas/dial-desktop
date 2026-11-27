// Static contract verification across Rust <-> TypeScript Tauri IPC and Desktop <-> Engine RPC.
// Prevents hand-mirrored wire names from silently drifting across crates or languages.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const SRC_DIR = path.dirname(fileURLToPath(import.meta.url));
const DESKTOP_ROOT = path.resolve(SRC_DIR, '..');
const APP_RS = path.join(DESKTOP_ROOT, 'src-tauri', 'src', 'app.rs');
const ENGINE_RPC_SCHEMA = path.resolve(DESKTOP_ROOT, '..', 'dial-engine', 'docs', 'rpc-schema.json');

function walkFiles(dir, exts) {
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walkFiles(full, exts));
    else if (exts.some((ext) => entry.name.endsWith(ext))) out.push(full);
  }
  return out;
}

function registeredTauriCommands(appRsText) {
  const match = appRsText.match(/tauri::generate_handler!\[([\s\S]*?)\]/);
  assert.ok(match, 'tauri::generate_handler![...] block must exist in app.rs');
  const commands = new Set();
  for (const raw of match[1].split(',')) {
    const cleaned = raw.replace(/\/\/[^\n]*/g, '').trim();
    if (!cleaned) continue;
    const leaf = cleaned.split('::').pop();
    if (leaf) commands.add(leaf);
  }
  return commands;
}

test('every frontend invoke() target is registered in Tauri generate_handler', () => {
  const registered = registeredTauriCommands(fs.readFileSync(APP_RS, 'utf8'));
  assert.ok(registered.size > 50, `expected >50 registered commands, found ${registered.size}`);

  const files = walkFiles(SRC_DIR, ['.ts', '.vue']);
  const missing = [];
  const invokePattern = /\binvoke(?:<[^()]*>)?\(\s*['"]([a-zA-Z0-9_]+)['"]/g;
  for (const file of files) {
    const text = fs.readFileSync(file, 'utf8');
    let m;
    while ((m = invokePattern.exec(text)) !== null) {
      const cmd = m[1];
      if (!registered.has(cmd)) {
        missing.push(`${path.relative(DESKTOP_ROOT, file)}: ${cmd}`);
      }
    }
  }
  assert.deepEqual(
    missing,
    [],
    `Frontend calls unregistered Tauri command(s):\n${missing.join('\n')}`,
  );
});

test('every Desktop workspace_rpc() and engine Command method exists in dial-engine rpc-schema.json', (t) => {
  if (!fs.existsSync(ENGINE_RPC_SCHEMA)) {
    t.skip('sibling dial-engine checkout not present');
    return;
  }
  const schema = JSON.parse(fs.readFileSync(ENGINE_RPC_SCHEMA, 'utf8'));
  const requestVariants = schema?.$defs?.request?.oneOf ?? [];
  const declared = new Set(
    requestVariants
      .map((variant) => variant?.properties?.method?.const)
      .filter((name) => typeof name === 'string'),
  );
  assert.ok(declared.size > 40, `expected >40 RPC methods in schema, found ${declared.size}`);

  const rustFiles = walkFiles(path.join(DESKTOP_ROOT, 'src-tauri', 'src'), ['.rs']);
  const unknown = [];
  const rpcCallPattern = /\bworkspace_rpc\([\s\S]{0,160}?"([a-z0-9_]+)"/g;
  for (const file of rustFiles) {
    const text = fs.readFileSync(file, 'utf8');
    let m;
    while ((m = rpcCallPattern.exec(text)) !== null) {
      const method = m[1];
      if (!declared.has(method)) {
        unknown.push(`${path.relative(DESKTOP_ROOT, file)}: ${method}`);
      }
    }
  }
  // Also verify every `engine.rs` `rpc("method", ...)` and `(true|false, "method", ...)` tuple.
  const engineRs = fs.readFileSync(path.join(DESKTOP_ROOT, 'src-tauri', 'src', 'engine.rs'), 'utf8');
  const engineMethodPattern = /\((?:true|false),\s*"([a-z0-9_]+)",/g;
  let em;
  while ((em = engineMethodPattern.exec(engineRs)) !== null) {
    if (!declared.has(em[1])) {
      unknown.push(`src-tauri/src/engine.rs: ${em[1]}`);
    }
  }
  assert.deepEqual(
    unknown,
    [],
    `Desktop calls RPC method(s) missing from dial-engine/docs/rpc-schema.json:\n${unknown.join('\n')}`,
  );
});
