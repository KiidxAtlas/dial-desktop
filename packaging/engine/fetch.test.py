"""Known-answer checks for fetch.py's offline parts: python3 packaging/engine/fetch.test.py"""

import hashlib
import importlib.util
import io
import pathlib
import tarfile
import zipfile

spec = importlib.util.spec_from_file_location("fetch", pathlib.Path(__file__).with_name("fetch.py"))
fetch = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fetch)

assert fetch.archive_name("v1.2.3", "aarch64-apple-darwin") == "dial-v1.2.3-aarch64-apple-darwin.tar.gz"
assert fetch.archive_name("v1.2.3", "x86_64-pc-windows-msvc") == "dial-v1.2.3-x86_64-pc-windows-msvc.zip"

digest = hashlib.sha256(b"x").hexdigest()
sums = f"{'0' * 64}  other.tar.gz\n{digest} *dial-v1-t.tar.gz\n"
assert fetch.expected_sum(sums, "dial-v1-t.tar.gz") == digest
try:
    fetch.expected_sum(sums, "missing.tar.gz")
    raise AssertionError("missing entry accepted")
except SystemExit:
    pass

buf = io.BytesIO()
with tarfile.open(fileobj=buf, mode="w:gz") as t:
    info = tarfile.TarInfo("dial-v1-t/dial")
    info.size = 3
    t.addfile(info, io.BytesIO(b"bin"))
assert fetch.extract(buf.getvalue(), "a.tar.gz", "v1", "t") == b"bin"

buf = io.BytesIO()
with zipfile.ZipFile(buf, "w") as z:
    z.writestr("dial-v1-x-windows/dial.exe", b"exe")
assert fetch.extract(buf.getvalue(), "a.zip", "v1", "x-windows") == b"exe"
print("fetch.py: ok")
