#!/usr/bin/env python3
"""Download the Dial engine release that ships inside Dial Desktop.

Usage: fetch.py TARGET [--version vX.Y.Z] [--out DIR]

Fetches dial-<version>-<TARGET>.tar.gz (.zip on Windows) and SHA256SUMS from
KiidxAtlas/dial, verifies the archive's checksum and, with DIAL_RELEASE_PUBLIC_KEY (the
release Ed25519 public key PEM), the SHA256SUMS.sig signature through OpenSSL 3. It writes
the binary as OUT/dial-<TARGET>[.exe], the name Tauri's `bundle.externalBin` expects.
Without the key it refuses unless --allow-unsigned is given.
"""

import argparse
import base64
import hashlib
import io
import os
import pathlib
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
import zipfile

REPO = "https://github.com/KiidxAtlas/dial/releases/download"
HERE = pathlib.Path(__file__).resolve().parent


def archive_name(version: str, target: str) -> str:
    ext = "zip" if "windows" in target else "tar.gz"
    return f"dial-{version}-{target}.{ext}"


def expected_sum(sums: str, name: str) -> str:
    for line in sums.splitlines():
        parts = line.split()
        if len(parts) == 2 and parts[1].lstrip("*") == name:
            digest = parts[0].lower()
            if len(digest) == 64 and all(c in "0123456789abcdef" for c in digest):
                return digest
    raise SystemExit(f"SHA256SUMS lists no valid checksum for {name}")


def extract(data: bytes, name: str, version: str, target: str) -> bytes:
    package = f"dial-{version}-{target}"
    binary = "dial.exe" if "windows" in target else "dial"
    member = f"{package}/{binary}"
    if name.endswith(".zip"):
        with zipfile.ZipFile(io.BytesIO(data)) as z:
            return z.read(member)
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as t:
        info = t.getmember(member)
        if not info.isfile():
            raise SystemExit(f"{member} is not a regular file")
        return t.extractfile(info).read()


def verify_signature(sums: bytes, sig_b64: bytes, pem: str) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        d = pathlib.Path(tmp)
        (d / "key.pem").write_text(pem)
        (d / "SHA256SUMS").write_bytes(sums)
        (d / "sig").write_bytes(base64.b64decode(b"".join(sig_b64.split())))
        done = subprocess.run(
            ["openssl", "pkeyutl", "-verify", "-pubin", "-inkey", str(d / "key.pem"), "-rawin",
             "-in", str(d / "SHA256SUMS"), "-sigfile", str(d / "sig")],
            capture_output=True,
        )
        if done.returncode != 0:
            raise SystemExit("SHA256SUMS signature verification failed")


def fetch(url: str) -> bytes:
    with urllib.request.urlopen(url, timeout=120) as response:
        return response.read()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("target")
    parser.add_argument("--version", default=(HERE / "VERSION").read_text().strip())
    parser.add_argument("--out", default=str(HERE.parent.parent / "src-tauri" / "binaries"))
    parser.add_argument("--allow-unsigned", action="store_true")
    args = parser.parse_args()
    base = f"{REPO}/{args.version}"
    name = archive_name(args.version, args.target)
    sums = fetch(f"{base}/SHA256SUMS")
    pem = os.environ.get("DIAL_RELEASE_PUBLIC_KEY", "").strip()
    if pem:
        verify_signature(sums, fetch(f"{base}/SHA256SUMS.sig"), pem + "\n")
    elif not args.allow_unsigned:
        raise SystemExit("Set DIAL_RELEASE_PUBLIC_KEY to verify the engine release (or pass --allow-unsigned)")
    data = fetch(f"{base}/{name}")
    if hashlib.sha256(data).hexdigest() != expected_sum(sums.decode(), name):
        raise SystemExit(f"{name} does not match SHA256SUMS")
    binary = extract(data, name, args.version, args.target)
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    dest = out / (f"dial-{args.target}" + (".exe" if "windows" in args.target else ""))
    tmp_path = None
    try:
        with tempfile.NamedTemporaryFile(dir=out, prefix=".dial-fetch-", delete=False) as tmp:
            tmp_path = pathlib.Path(tmp.name)
            tmp.write(binary)
            tmp.flush()
            os.fsync(tmp.fileno())
        tmp_path.chmod(0o755)
        os.replace(tmp_path, dest)
        tmp_path = None
    finally:
        if tmp_path is not None:
            tmp_path.unlink(missing_ok=True)
    print(f"{dest} ({args.version}, {len(binary)} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
