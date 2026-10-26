"""Build a task-local std sysroot source overlay; never patch rustup's files."""
import argparse
import difflib
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
COMMIT = "787af2b8c80638c51a4fc8e44f84e6891f243ec7"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--toolchain", default="nightly")
    parser.add_argument("--output", type=Path, default=Path.home() / ".cache/androswitch/horizon-std-toolchain-787af2b")
    args = parser.parse_args()
    version = subprocess.check_output(["rustc", "+" + args.toolchain, "-vV"], text=True)
    if COMMIT not in version:
        raise SystemExit("Pinned compiler commit required")
    origin = Path(subprocess.check_output(["rustc", "+" + args.toolchain, "--print", "sysroot"], text=True).strip())
    output = args.output.resolve()
    if output == origin.resolve() or origin.resolve() in output.parents:
        raise SystemExit("Overlay must be outside the installed toolchain")
    source = origin / "lib/rustlib/src/rust/library"
    lock = json.loads((ROOT / "platform/horizon-std/rust-source.lock.json").read_text())
    for name, expected in lock["source_sha256"].items():
        if hashlib.sha256((source / name).read_bytes()).hexdigest() != expected:
            raise SystemExit(f"Rust source mismatch: {name}")
    # Immutable compiler/runtime files may be hard-linked. All source files are
    # independent copies: no source write can reach the installed rustup tree.
    for parent, dirs, files in os.walk(origin):
        relative = Path(parent).relative_to(origin)
        if relative.as_posix() == "lib/rustlib":
            dirs[:] = [d for d in dirs if d != "src"]
        destination = output / relative
        destination.mkdir(parents=True, exist_ok=True)
        for name in files:
            target = destination / name
            if not target.exists():
                os.link(Path(parent) / name, target)
    copied = output / "lib/rustlib/src/rust/library"
    if not (copied / "Cargo.lock").exists():
        shutil.copytree(source, copied, dirs_exist_ok=True)
    source_tree = []
    for original in sorted(source.rglob("*")):
        if not original.is_file():
            continue
        name = original.relative_to(source).as_posix()
        contents = original.read_bytes()
        source_tree.append(name + "\0" + hashlib.sha256(contents).hexdigest() + "\n")
        if name not in ("std/src/sys/fs/unix.rs", "std/src/sys/thread/unix.rs"):
            local = copied / name
            if not local.is_file() or local.read_bytes() != contents:
                raise SystemExit(f"Unexpected std overlay source drift outside the two adaptations: {name}")
    source_tree_sha256 = hashlib.sha256("".join(source_tree).encode()).hexdigest()
    if source_tree_sha256 != lock["library_tree_sha256"] or len(source_tree) != lock["library_file_count"]:
        raise SystemExit("Installed Rust library source tree differs from the pinned input")
    patches = []
    # Horizon SD mounts have no Unix chmod contract. As for std's existing
    # ESP-IDF/WASI copy ports, copy bytes without claiming chmod succeeded.
    # Explicit set_permissions remains an honest unsupported operation.
    path = copied / "std/src/sys/fs/unix.rs"
    before = (source / "std/src/sys/fs/unix.rs").read_text(encoding="utf-8")
    old = '#[cfg(any(target_os = "espidf", target_os = "wasi"))]\nfn open_to_and_set_permissions'
    new = '#[cfg(any(target_os = "espidf", target_os = "wasi", target_os = "horizon"))]\nfn open_to_and_set_permissions'
    inverse = '#[cfg(not(any(target_os = "espidf", target_os = "wasi")))]\nfn open_to_and_set_permissions'
    if before.count(old) != 1 or before.count(inverse) != 1:
        raise SystemExit("Unexpected std filesystem copy implementation")
    after = before.replace(old, new).replace(inverse, inverse.replace('target_os = "wasi"', 'target_os = "wasi", target_os = "horizon"'))
    if path.read_bytes() != after.encode("utf-8"):
        path.write_text(after, encoding="utf-8", newline="\n")
    patches.append((path.relative_to(copied).as_posix(), before, after))
    path = copied / "std/src/sys/thread/unix.rs"
    before = (source / "std/src/sys/thread/unix.rs").read_text(encoding="utf-8")
    anchor = 'pub fn available_parallelism() -> io::Result<NonZero<usize>> {\n    cfg_select! {\n'
    if before.count(anchor) != 1:
        raise SystemExit("Unexpected std parallelism implementation")
    branch = '''        target_os = "horizon" => {
            // The PAL queries the current process's actual allowed core mask.
            let cpus = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
            if cpus < 0 { Err(io::Error::last_os_error()) }
            else { NonZero::new(cpus as usize).ok_or(io::Error::UNKNOWN_THREAD_COUNT) }
        }
'''
    after = before.replace(anchor, anchor + branch)
    if path.read_bytes() != after.encode("utf-8"):
        path.write_text(after, encoding="utf-8", newline="\n")
    patches.append((path.relative_to(copied).as_posix(), before, after))
    receipt = {"compiler_commit": COMMIT, "original_sysroot": str(origin), "overlay_sysroot": str(output),
               "original_library_tree_sha256": source_tree_sha256,
               "original_library_file_count": len(source_tree),
               "files": [{"path": name, "original_sha256": hashlib.sha256(old.encode()).hexdigest(),
                          "adapted_sha256": hashlib.sha256(new.encode()).hexdigest()} for name, old, new in patches]}
    evidence = ROOT / "local-artifacts/horizon-std"
    evidence.mkdir(parents=True, exist_ok=True)
    (evidence / "rust-std-adaptation.patch").write_text("".join(
        "".join(difflib.unified_diff(old.splitlines(True), new.splitlines(True), fromfile=name, tofile=name))
        for name, old, new in patches), encoding="utf-8", newline="\n")
    (evidence / "toolchain.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(receipt))


if __name__ == "__main__":
    main()
