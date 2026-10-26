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
            subprocess.run([str(args.objcopy),
                            "--redefine-sym", "rust_eh_personality=as_mesa_rust_eh_personality",
                            "--redefine-sym", "DW.ref.rust_eh_personality=DW.ref.as_mesa_rust_eh_personality",
                            str(source), str(temporary)], check=True, cwd=scratch)
            shutil.copyfile(temporary, target)
        if digest(source) != original:
            raise SystemExit("Mesa SDK changed during adaptation")
        records.append({"source": str(source), "source_sha256": original,
                        "output": str(target), "output_sha256": digest(target)})
    (output / "link-adaptation.json").write_text(json.dumps({
        "schema_version": 1,
        "policy": "separate Mesa personality; no shared Rust ABI or ignored duplicate symbols",
        "archives": records,
    }, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
