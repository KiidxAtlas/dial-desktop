// Staging and verification checks for the optional QEMU runtime bundle. Fixtures are generated in
// a temporary directory per test and removed afterwards. No QEMU binary is needed: the "binary"
// is a small shell script, so these tests cover the staged layout and the validator, not boot.
//
// Run: node --test packaging/runtime/stage-runtime.test.mjs

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { parseArgs } from "./stage-runtime.mjs";
import {
  MANIFEST_NAME,
  RuntimeBundleError,
  canonicalManifestText,
  checkRelativePath,
  expectedBinary,
  requiredFirmware,
  stageRuntime,
  verifyRuntimeDir,
} from "./runtime-bundle.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const LINUX = "x86_64-unknown-linux-gnu";
const WINDOWS = "x86_64-pc-windows-msvc";
const MAC_ARM = "aarch64-apple-darwin";
const posix = process.platform !== "win32";

// Symlink creation needs a privilege on many Windows setups. Probe once; the symlink cases skip
// (with a reason in the test report) when the host cannot create one.
const canSymlink = await (async () => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "dial-runtime-probe-"));
  try {
    await fs.writeFile(path.join(dir, "target"), "");
    await fs.symlink(path.join(dir, "target"), path.join(dir, "link"));
    return true;
  } catch {
    return false;
  } finally {
    await fs.rm(dir, { recursive: true, force: true });
  }
})();
const symlinkSkip = !canSymlink && "this host cannot create symbolic links";

const sha256 = (buf) => createHash("sha256").update(buf).digest("hex");

async function withTmp(fn) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "dial-runtime-"));
  try {
    return await fn(root);
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
}

// Writes a complete, valid bundle for `target` under `dir` and returns its manifest object.
async function makeBundle(dir, target = LINUX) {
  const binary = expectedBinary(target);
  const [code, vars] = requiredFirmware(target);
  const files = {
    [binary]: { content: "#!/bin/sh\necho fake-qemu\n", executable: true },
    [code]: { content: "firmware-code-bytes", executable: false },
    [vars]: { content: "firmware-vars-bytes", executable: false },
    "licenses/COPYING": { content: "GPL text fixture", executable: false },
  };
  await fs.mkdir(dir, { recursive: true });
  const entries = [];
  for (const [rel, spec] of Object.entries(files)) {
    const full = path.join(dir, ...rel.split("/"));
    await fs.mkdir(path.dirname(full), { recursive: true });
    await fs.writeFile(full, spec.content);
    if (posix) await fs.chmod(full, spec.executable ? 0o755 : 0o644);
    entries.push({
      path: rel,
      sha256: sha256(Buffer.from(spec.content)),
      size: Buffer.byteLength(spec.content),
      executable: spec.executable,
    });
  }
  const manifest = {
    schema: 1,
    target,
    binary,
    qemu: {
      version: "9.2.1",
      sourceUrl: "https://download.qemu.org/qemu-9.2.1.tar.xz",
      sourceSha256: sha256(Buffer.from("qemu source fixture")),
    },
    files: entries,
  };
  await fs.writeFile(path.join(dir, MANIFEST_NAME), canonicalManifestText(manifest));
  return manifest;
}

const readManifest = async (dir) => JSON.parse(await fs.readFile(path.join(dir, MANIFEST_NAME), "utf8"));
const writeManifest = (dir, manifest) => fs.writeFile(path.join(dir, MANIFEST_NAME), JSON.stringify(manifest, null, 2));

// Collects {relative path: sha256} for every file under dir, for comparing staged trees.
async function treeDigest(dir) {
  const out = {};
  async function walk(current, prefix) {
    for (const entry of await fs.readdir(current, { withFileTypes: true })) {
      const rel = prefix ? `${prefix}/${entry.name}` : entry.name;
      const full = path.join(current, entry.name);
      assert.ok(!entry.isSymbolicLink(), `symlink in staged tree: ${rel}`);
      if (entry.isDirectory()) await walk(full, rel);
      else out[rel] = sha256(await fs.readFile(full));
    }
  }
  await walk(dir, "");
  return out;
}

test("a complete bundle stages into a verified, canonical tree", async () => {
  await withTmp(async (root) => {
    const from = path.join(root, "bundle");
    await makeBundle(from, LINUX);
    const out = path.join(root, "out", "qemu");
    const result = await stageRuntime({ from, target: LINUX, out });
    assert.equal(result.files, 4);
    assert.equal(result.binary, "bin/qemu-system-x86_64");
    const manifest = await verifyRuntimeDir(out, { target: LINUX });
    assert.equal(manifest.target, LINUX);
    if (posix) {
      const stat = await fs.stat(path.join(out, "bin", "qemu-system-x86_64"));
      assert.equal(stat.mode & 0o777, 0o755);
    }
  });
});

test("staging the same bundle twice yields byte-identical trees", async () => {
  await withTmp(async (root) => {
    const from = path.join(root, "bundle");
    await makeBundle(from, LINUX);
    const first = path.join(root, "first");
    const second = path.join(root, "second");
    await stageRuntime({ from, target: LINUX, out: first });
    await stageRuntime({ from, target: LINUX, out: second });
    assert.deepEqual(await treeDigest(first), await treeDigest(second));
  });
});

test("a Windows bundle must ship an .exe binary and stage", async () => {
  await withTmp(async (root) => {
    const from = path.join(root, "bundle");
    await makeBundle(from, WINDOWS);
    assert.equal(expectedBinary(WINDOWS), "bin/qemu-system-x86_64.exe");
    await stageRuntime({ from, target: WINDOWS, out: path.join(root, "out") });
    await verifyRuntimeDir(path.join(root, "out"), { target: WINDOWS });
  });
});

// Each row breaks one property of a valid bundle; the validator must refuse it with the message.
const REJECTIONS = [
  {
    name: "a symlink inside the bundle",
    skip: symlinkSkip,
    mutate: async (dir) => {
      await fs.symlink(path.join(dir, "licenses", "COPYING"), path.join(dir, "share", "link.fd"));
    },
    message: /symbolic link/,
  },
  {
    name: "an undeclared extra file",
    mutate: (dir) => fs.writeFile(path.join(dir, "share", "extra.bin"), "x"),
    message: /undeclared file/,
  },
  {
    name: "a manifest entry whose file is missing",
    mutate: (dir) => fs.rm(path.join(dir, "share", "edk2-x86_64-code.fd")),
    message: /absent/,
  },
  {
    name: "the code firmware missing from the manifest",
    mutate: async (dir) => {
      const m = await readManifest(dir);
      m.files = m.files.filter((f) => f.path !== "share/edk2-x86_64-code.fd");
      await writeManifest(dir, m);
    },
    message: /edk2-x86_64-code\.fd/,
  },
  {
    name: "the vars firmware missing from the manifest",
    mutate: async (dir) => {
      const m = await readManifest(dir);
      m.files = m.files.filter((f) => f.path !== "share/edk2-x86_64-vars.fd");
      await writeManifest(dir, m);
    },
    message: /edk2-x86_64-vars\.fd/,
  },
  {
    name: "firmware for another architecture under the target's names",
    mutate: async (dir) => {
      await fs.rename(path.join(dir, "share", "edk2-x86_64-code.fd"), path.join(dir, "share", "edk2-aarch64-code.fd"));
      const m = await readManifest(dir);
      m.files.find((f) => f.path === "share/edk2-x86_64-code.fd").path = "share/edk2-aarch64-code.fd";
      await writeManifest(dir, m);
    },
    message: /edk2-x86_64-code\.fd/,
  },
  {
    name: "a Windows device name in a manifest path",
    mutate: async (dir) => {
      const m = await readManifest(dir);
      m.files.find((f) => f.path === "licenses/COPYING").path = "licenses/nul.txt";
      await writeManifest(dir, m);
    },
    message: /device name/,
  },
  {
    name: "an alternate data stream in a manifest path",
    mutate: async (dir) => {
      const m = await readManifest(dir);
      m.files.find((f) => f.path === "licenses/COPYING").path = "licenses/COPYING:Zone.Identifier";
      await writeManifest(dir, m);
    },
    message: /forbids/,
  },
  {
    name: "a trailing dot in a manifest path",
    mutate: async (dir) => {
      const m = await readManifest(dir);
      m.files.find((f) => f.path === "licenses/COPYING").path = "licenses/COPYING.";
      await writeManifest(dir, m);
    },
    message: /dot or space/,
  },
  {
    name: "a trailing space in a manifest path",
    mutate: async (dir) => {
      const m = await readManifest(dir);
      m.files.find((f) => f.path === "licenses/COPYING").path = "licenses/COPYING ";
      await writeManifest(dir, m);
    },
    message: /dot or space/,
  },
  {
    name: "content changed at the same size",
    mutate: (dir) => fs.writeFile(path.join(dir, "share", "edk2-x86_64-code.fd"), "firmware-code-bytex"),
    message: /sha256/,
  },
  {
    name: "content whose size differs from the manifest",
    mutate: (dir) => fs.writeFile(path.join(dir, "share", "edk2-x86_64-code.fd"), "firmware-code"),
    message: /size/,
  },
  {
    name: "a parent-directory segment in a manifest path",
    mutate: async (dir) => {
      const m = await readManifest(dir);
      m.files[0].path = "bin/../../etc/passwd";
      await writeManifest(dir, m);
    },
    message: /"\.\." segment|empty/,
  },
  {
    name: "an absolute manifest path",
    mutate: async (dir) => {
      const m = await readManifest(dir);
      m.files.find((f) => f.path.startsWith("share/")).path = "/usr/share/firmware.fd";
      await writeManifest(dir, m);
    },
    message: /absolute|must live under/,
  },
  {
    name: "a backslash in a manifest path",
    mutate: async (dir) => {
      const m = await readManifest(dir);
      m.files.find((f) => f.path.startsWith("share/")).path = "share\\firmware.fd";
      await writeManifest(dir, m);
    },
    message: /backslash/,
  },
  {
    name: "a manifest path outside the allowed roots",
    mutate: async (dir) => {
      const m = await readManifest(dir);
      m.files.find((f) => f.path.startsWith("licenses/")).path = "etc/COPYING";
      await writeManifest(dir, m);
    },
    message: /must live under/,
  },
  {
    name: "the binary not marked executable in the manifest",
    mutate: async (dir) => {
      const m = await readManifest(dir);
      m.files.find((f) => f.path === m.binary).executable = false;
      await writeManifest(dir, m);
    },
    message: /must be marked executable|not declared so/,
  },
  {
    name: "the binary file not executable on disk",
    skip: !posix,
    mutate: (dir) => fs.chmod(path.join(dir, "bin", "qemu-system-x86_64"), 0o644),
    message: /must be executable/,
  },
  {
    name: "an executable file the manifest does not mark executable",
    skip: !posix,
    mutate: (dir) => fs.chmod(path.join(dir, "share", "edk2-x86_64-code.fd"), 0o755),
    message: /executable but not declared/,
  },
  {
    name: "a binary name that does not match the target architecture",
    mutate: async (dir) => {
      const m = await readManifest(dir);
      m.binary = "bin/qemu-system-aarch64";
      await writeManifest(dir, m);
    },
    message: /binary must be/,
  },
  {
    name: "no license file",
    mutate: async (dir) => {
      await fs.rm(path.join(dir, "licenses"), { recursive: true });
      const m = await readManifest(dir);
      m.files = m.files.filter((f) => !f.path.startsWith("licenses/"));
      await writeManifest(dir, m);
    },
    message: /license/,
  },
  {
    name: "no firmware under share/",
    mutate: async (dir) => {
      await fs.rm(path.join(dir, "share"), { recursive: true });
      const m = await readManifest(dir);
      m.files = m.files.filter((f) => !f.path.startsWith("share/"));
      await writeManifest(dir, m);
    },
    message: /edk2-x86_64-code\.fd/,
  },
  {
    name: "two manifest paths that differ only in case",
    mutate: async (dir) => {
      const m = await readManifest(dir);
      const copy = { ...m.files.find((f) => f.path.startsWith("share/")), path: "share/EDK2-X86_64-CODE.FD" };
      m.files.push(copy);
      await writeManifest(dir, m);
    },
    message: /twice/,
  },
  {
    name: "an unknown manifest key",
    mutate: async (dir) => {
      const m = await readManifest(dir);
      m.extra = true;
      await writeManifest(dir, m);
    },
    message: /unknown key "extra"/,
  },
  {
    name: "a manifest built for a different target",
    mutate: async (dir) => {
      const m = await readManifest(dir);
      m.target = MAC_ARM;
      m.binary = expectedBinary(MAC_ARM);
      await writeManifest(dir, m);
    },
    message: /built for|expected/,
  },
  {
    name: "a non-https QEMU source URL",
    mutate: async (dir) => {
      const m = await readManifest(dir);
      m.qemu.sourceUrl = "http://download.qemu.org/qemu-9.2.1.tar.xz";
      await writeManifest(dir, m);
    },
    message: /https/,
  },
];

for (const row of REJECTIONS) {
  test(`refuses ${row.name}`, { skip: row.skip ?? false }, async () => {
    await withTmp(async (root) => {
      const from = path.join(root, "bundle");
      await makeBundle(from, LINUX);
      await row.mutate(from);
      await assert.rejects(
        () => stageRuntime({ from, target: LINUX, out: path.join(root, "out") }),
        (error) => error instanceof RuntimeBundleError && row.message.test(error.message),
      );
      await assert.rejects(fs.stat(path.join(root, "out")), { code: "ENOENT" });
    });
  });
}

test("refuses to stage when the source directory is a symlink", { skip: symlinkSkip }, async () => {
  await withTmp(async (root) => {
    const real = path.join(root, "real");
    await makeBundle(real, LINUX);
    const link = path.join(root, "link");
    await fs.symlink(real, link);
    await assert.rejects(
      () => stageRuntime({ from: link, target: LINUX, out: path.join(root, "out") }),
      /symbolic link/,
    );
  });
});

test("refuses an output that is a symlink", { skip: symlinkSkip }, async () => {
  await withTmp(async (root) => {
    const from = path.join(root, "bundle");
    await makeBundle(from, LINUX);
    const elsewhere = path.join(root, "elsewhere");
    await fs.mkdir(elsewhere);
    const out = path.join(root, "out");
    await fs.symlink(elsewhere, out);
    await assert.rejects(() => stageRuntime({ from, target: LINUX, out }), /symbolic link/);
    assert.deepEqual(await fs.readdir(elsewhere), []);
  });
});

test("never removes unrelated content from the output directory", async () => {
  await withTmp(async (root) => {
    const from = path.join(root, "bundle");
    await makeBundle(from, LINUX);
    const out = path.join(root, "out");
    await fs.mkdir(out);
    await fs.writeFile(path.join(out, "user-notes.txt"), "keep me");
    await assert.rejects(() => stageRuntime({ from, target: LINUX, out, force: true }), /not empty|not created by staging/);
    assert.equal(await fs.readFile(path.join(out, "user-notes.txt"), "utf8"), "keep me");
  });
});

test("replaces a previous staging only with force", async () => {
  await withTmp(async (root) => {
    const from = path.join(root, "bundle");
    await makeBundle(from, LINUX);
    const out = path.join(root, "out");
    await stageRuntime({ from, target: LINUX, out });
    await assert.rejects(() => stageRuntime({ from, target: LINUX, out }), /--force/);
    await stageRuntime({ from, target: LINUX, out, force: true });
    await verifyRuntimeDir(out, { target: LINUX });
  });
});

test("verification of a staged tree also refuses a tampered binary", { skip: !posix }, async () => {
  await withTmp(async (root) => {
    const from = path.join(root, "bundle");
    await makeBundle(from, LINUX);
    const out = path.join(root, "out");
    await stageRuntime({ from, target: LINUX, out });
    await fs.writeFile(path.join(out, "bin", "qemu-system-x86_64"), "#!/bin/sh\necho evil\n");
    await assert.rejects(() => verifyRuntimeDir(out, { target: LINUX }), /sha256|size/);
  });
});

test("manifest paths: allowed shapes pass, unsafe and Windows-hostile forms fail", () => {
  const ok = [
    "bin/qemu-system-x86_64",
    "share/edk2-x86_64-code.fd",
    "lib/libfoo.dll",
    "licenses/COPYING",
    "share/a/b/c.fd",
    "licenses/CONTRIB.txt", // stem "CONTRIB" is not the reserved "CON"
    "lib/conaaa.dll",
    "licenses/COPYING-2.0",
  ];
  for (const p of ok) assert.equal(checkRelativePath(p), p);
  const bad = [
    "", "bin", "bin/", "/bin/x", "bin//x", "bin/./x", "bin/../x", "C:/x", "bin\\x", "etc/x", "bin/x\0",
    "licenses/CON", "licenses/aux.txt", "licenses/nul.tar.gz", "licenses/COM1.log", "licenses/lpt9",
    "licenses/COM\u00b9.txt",
    "licenses/COPYING.", "licenses/COPYING ", "licenses/dir./x",
    "licenses/COPYING:Zone.Identifier", "licenses/a:b",
    'bin/a<b', 'bin/a>b', 'bin/a"b', 'bin/a|b', 'bin/a?b', 'bin/a*b', "bin/a\u0001b",
  ];
  for (const p of bad) assert.throws(() => checkRelativePath(p), RuntimeBundleError, `accepted ${JSON.stringify(p)}`);
});

test("CLI argument parsing requires an explicit target and source", () => {
  const table = [
    { argv: ["--from", "d"], message: /--target is required/ },
    { argv: ["--target", LINUX], message: /--from <bundle-dir> is required/ },
    { argv: ["--target", LINUX, "--from"], message: /--from needs a value/ },
    { argv: ["--target", LINUX, "--from", "d", "--fetch"], message: /unknown argument --fetch/ },
  ];
  for (const { argv, message } of table) assert.throws(() => parseArgs(argv), message);
  const ok = parseArgs(["--target", LINUX, "--check", "d"]);
  assert.equal(ok.check, true);
  assert.equal(ok.from, "d");
  assert.ok(path.isAbsolute(parseArgs(["--target", LINUX, "--from", "d"]).out));
});

test("the firmware names match the runner's Arch::firmware_code/vars", () => {
  assert.deepEqual(requiredFirmware(LINUX), ["share/edk2-x86_64-code.fd", "share/edk2-x86_64-vars.fd"]);
  assert.deepEqual(requiredFirmware(MAC_ARM), ["share/edk2-aarch64-code.fd", "share/edk2-aarch64-vars.fd"]);
});

test("the opt-in Tauri overlay maps only the staged runtime directory", async () => {
  const overlay = JSON.parse(await fs.readFile(path.join(here, "..", "..", "src-tauri", "tauri.runtime.conf.json"), "utf8"));
  assert.deepEqual(overlay.bundle.resources, { "resources/qemu/": "qemu/" });
});
