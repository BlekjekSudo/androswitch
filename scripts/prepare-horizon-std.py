"""Prepare an isolated libc adaptation; never modify rustup or the Cargo registry."""
import argparse
import difflib
import hashlib
import json
import re
from pathlib import Path
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
SHA256 = "3eaf3ede3fee6db1a4c2ee091bf8a8b4dccdc6d17f656fb07896ee72867612f2"
URL = "https://static.crates.io/crates/libc/libc-0.2.189.crate"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "local-artifacts/horizon-std")
    args = parser.parse_args()
    base = args.output.resolve()
    base.mkdir(parents=True, exist_ok=True)
    archive = base / "libc-0.2.189.crate"
    if not archive.exists():
        candidates = list((Path.home() / ".cargo/registry/cache").glob("*/libc-0.2.189.crate"))
        archive.write_bytes(candidates[0].read_bytes() if candidates else urllib.request.urlopen(URL).read())
    if hashlib.sha256(archive.read_bytes()).hexdigest() != SHA256:
        raise SystemExit("Pinned libc archive checksum mismatch")
    # Only normal files within the pinned package; no archive links or traversal.
    with tarfile.open(archive) as package:
        for member in package:
            parts = Path(member.name).parts
            if member.isfile():
                if parts[0] != "libc-0.2.189" or ".." in parts or Path(member.name).is_absolute():
                    raise SystemExit("Unsafe archive member")
                path = base.joinpath(*parts)
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(package.extractfile(member).read())
    path = base / "libc-0.2.189/src/unix/newlib/mod.rs"
    original = path.read_text(encoding="utf-8")
    adapted = original
    # ARM64 devkitA64/newlib headers differ from the inherited 3DS definitions.
    # The runtime probe compares sizes, offsets and constants against C first.
    for before, after in [
        ("pub type blkcnt_t = i32;", "pub type blkcnt_t = c_long;"),
        ("pub type blksize_t = i32;", "pub type blksize_t = c_long;"),
        ("pub type dev_t = u32;", "pub type dev_t = c_short;"),
        ("pub type ino_t = u32;", "pub type ino_t = c_ushort;"),
    ]:
        if adapted.count(before) != 1:
            raise SystemExit(f"Unexpected libc source: {before}")
        adapted = adapted.replace(before, after)
    # Match the actual pthread ABI supplied by devkitA64 libsysbase/libnx.
    adapted = adapted.replace("    pub struct pthread_attr_t {", "    #[repr(align(8))]\n    pub struct pthread_attr_t {")
    pattern = r"    #\[cfg_attr\(\n.*?    pub struct pthread_mutex_t \{"
    adapted, count = re.subn(pattern, "    #[repr(align(4))]\n    pub struct pthread_mutex_t {", adapted, count=1, flags=re.S)
    if count != 1:
        raise SystemExit("pthread mutex alignment declaration changed")
    attr_pos = adapted.index("    pub struct pthread_mutexattr_t {")
    attr_start = adapted.rfind("    #[cfg_attr(", 0, attr_pos)
    if "repr(align(8))" not in adapted[attr_start:attr_pos]:
        raise SystemExit("pthread mutex attribute alignment changed")
    adapted = adapted[:attr_start] + "    #[repr(align(4))]\n" + adapted[attr_pos:]
    adapted = adapted.replace("    #[repr(align(4))]\n    pub struct pthread_condattr_t", "    #[repr(align(8))]\n    pub struct pthread_condattr_t")
    for before, after in [
        ("pub const __SIZEOF_PTHREAD_ATTR_T: usize = 56;", "pub const __SIZEOF_PTHREAD_ATTR_T: usize = 24;"),
        ("pub const __SIZEOF_PTHREAD_MUTEX_T: usize = 40;", "pub const __SIZEOF_PTHREAD_MUTEX_T: usize = 12;"),
        ("pub const __SIZEOF_PTHREAD_COND_T: usize = 48;", "pub const __SIZEOF_PTHREAD_COND_T: usize = 16;"),
    ]:
        if adapted.count(before) != 1:
            raise SystemExit(f"Unexpected pthread source: {before}")
        adapted = adapted.replace(before, after)
    # Only the generic/default branch is selected by this custom target.
    marker = "        pub const __SIZEOF_PTHREAD_COND_T: usize = 16;\n        pub const __SIZEOF_PTHREAD_CONDATTR_T: usize = 4;"
    if adapted.count(marker) != 1:
        raise SystemExit("Unexpected condition attribute size")
    adapted = adapted.replace(marker, marker.replace("CONDATTR_T: usize = 4;", "CONDATTR_T: usize = 8;"))
    adapted += "\n// Standalone devkitA64/newlib std experiment only.\npub const AT_FDCWD: c_int = -2;\n"
    path.write_text(adapted, encoding="utf-8", newline="\n")
    patch = "".join(difflib.unified_diff(original.splitlines(True), adapted.splitlines(True),
                                       fromfile="libc-0.2.189/src/unix/newlib/mod.rs",
                                       tofile="libc-0.2.189/src/unix/newlib/mod.rs"))
    (base / "libc-adaptation.patch").write_text(patch, encoding="utf-8", newline="\n")
    receipt = {"url": URL, "sha256": SHA256, "source": str(path),
               "adapted_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
               "scope": "standalone experiment only; not the product dependency graph"}
    (base / "libc-source.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(receipt))


if __name__ == "__main__":
    main()
