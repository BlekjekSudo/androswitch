"""Isolate the pinned Mesa shader compiler's Rust exception personality.

Rust symbols with crate hashes already distinguish the SDK's compiler version.
The unmangled personality and its weak DWARF indirection require explicit
namespacing. Rewrite definitions AND references in copies of every archive;
never use --allow-multiple-definition or alter the downloaded SDK.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import shutil
import tempfile

PERSONALITY = {"rust_eh_personality": "as_mesa_rust_eh_personality",
               "DW.ref.rust_eh_personality": "DW.ref.as_mesa_rust_eh_personality"}
LINUX_ABI = {"clock_gettime": "as_mesa_clock_gettime",
             "clock_getres": "as_mesa_clock_getres", "sysconf": "as_mesa_sysconf"}


def rewrite_object(objcopy, source, target, mapping):
    flags = [arg for old, new in mapping.items() for arg in ("--redefine-sym", old + "=" + new)]
    subprocess.run([str(objcopy), *flags, str(source), str(target)], check=True)


def rewrite_public_archive(source, target, objcopy, scratch):
    """Keep Horizon C's newlib clock ABI separate from GNU/Linux Rust objects.

    Public Mesa archives merge C objects and Rust .rcgu.o objects. Preserve
    their names/order without extracting Meson paths; regenerate the archive
    index after object sizes change. Both 32/64-bit GNU indices are replaced.
    """
    raw = source.read_bytes()
    if not raw.startswith(b"!<arch>\n"):
        raise ValueError("Expected a full public SDK archive, not a thin archive")
    position, names, members, changed = 8, b"", [], 0
    while position < len(raw):
        header = raw[position:position + 60]
        if len(header) != 60 or header[58:] != b"`\n":
            raise ValueError("Malformed SDK archive header")
        size = int(header[48:58])
        payload = raw[position + 60:position + 60 + size]
        if len(payload) != size:
            raise ValueError("Truncated SDK archive member")
        token = header[:16].decode("ascii").strip()
        position += 60 + size + size % 2
        if token in ("/", "/SYM64/"):
            continue
        prefix = b""
        if token == "//":
            names = payload
            name = token
        elif token.startswith("#1/"):
            length = int(token[3:])
            prefix, payload = payload[:length], payload[length:]
            name = prefix.rstrip(b"\0").decode("utf-8")
        elif token.startswith("/") and token[1:].isdigit():
            name = names[int(token[1:]):].split(b"/\n", 1)[0].decode("utf-8")
        else:
            name = token.rstrip("/")
        if name.endswith(".rcgu.o"):
            original, adapted = scratch / "object.o", scratch / "adapted.o"
            original.write_bytes(payload)
            rewrite_object(objcopy, original, adapted, LINUX_ABI)
            payload = adapted.read_bytes()
            changed += 1
        payload = prefix + payload
        members.append(header[:48] + f"{len(payload):<10}".encode() + header[58:] + payload
                       + (b"\n" if len(payload) % 2 else b""))
    target.write_bytes(b"!<arch>\n" + b"".join(members))
    ar = objcopy.with_name("llvm-ar" + objcopy.suffix)
    subprocess.run([str(ar), "s", str(target)], check=True)
    return changed


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sdk", type=Path, required=True)
    parser.add_argument("--rust-libs", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--objcopy", type=Path, required=True)
    parser.add_argument("--portlibs", type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    # Link-time public archives only. SDK build-intermediate archives can
    # contain Meson relative member paths and are not product link inputs.
    libraries = ("EGL", "GLESv2", "glapi", "vulkan", "mesa_util_c11", "blake3",
                 "mesa_util", "mesa_util_simd", "expat", "zstd", "z")
    inputs = [(args.sdk / "lib" / f"lib{name}.a", output / "lib" / f"lib{name}.a")
              for name in libraries]
    inputs = [(source if source.is_file() else args.portlibs / source.name, target)
              for source, target in inputs]
    inputs += [(p, output / "rust" / p.name) for p in sorted(args.rust_libs.glob("*.rlib"))
               if not p.name.startswith("libpanic_abort-")]
    if not inputs:
        raise SystemExit("Missing Mesa link inputs")
    records = []
    for source, target in inputs:
        source = source.resolve()
        if output == source.parent or output in source.parents:
            raise SystemExit("Output must not overwrite SDK inputs")
        target.parent.mkdir(parents=True, exist_ok=True)
        original = digest(source)
        # GNU objcopy's internal archive temporary paths still obey MAX_PATH
        # on Windows. Keep only this owned intermediate directory short.
        with tempfile.TemporaryDirectory(prefix="as-mesa-") as scratch:
            temporary = Path(scratch) / "lib.a"
            changed = None
            if source.suffix == ".rlib":
                rewrite_object(args.objcopy, source, temporary, {**PERSONALITY, **LINUX_ABI})
            else:
                personality = Path(scratch) / "personality.a"
                rewrite_object(args.objcopy, source, personality, PERSONALITY)
                changed = rewrite_public_archive(personality, temporary, args.objcopy, Path(scratch))
            shutil.copyfile(temporary, target)
        if digest(source) != original:
            raise SystemExit("Mesa SDK changed during adaptation")
        records.append({"source": str(source), "source_sha256": original,
                        "output": str(target), "output_sha256": digest(target),
                        "linux_rust_objects": changed if changed is not None else "compiler-matched Rust runtime archive"})
    (output / "link-adaptation.json").write_text(json.dumps({
        "schema_version": 1,
        "policy": "separate Mesa Rust personality/Linux clock/sysconf ABI; Horizon C and AndroSwitch std keep newlib ABI",
        "personality_symbols": PERSONALITY, "linux_rust_symbols": LINUX_ABI,
        "archives": records,
    }, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
