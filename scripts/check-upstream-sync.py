#!/usr/bin/env python3
"""Audit the entire pinned upstream tree, adaptations, and device exclusions.

Source synchronization is not execution parity. --require-device-parity is a
deliberately stricter gate and fails while any original runtime module is still
excluded, replaced only partially, or lacks target behavior evidence.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LOCK = Path("data/rustydroid-upstream.json")
TEXT = {".rs", ".toml", ".json", ".md", ".py", ".yml", ".yaml", ".txt", ".c", ".h", ".sh"}


def git(root: Path, *args: str) -> str:
    return subprocess.check_output(
        ["git", "-c", "core.fsmonitor=false", *args], cwd=root, text=True, encoding="utf-8"
    ).strip()


def canonical(path: Path) -> bytes:
    data = path.read_bytes()
    if path.suffix in TEXT or path.name.startswith("."):
        data = data.replace(b"\r\n", b"\n")
    return data


def blob_oid(data: bytes) -> str:
    return hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()


def safe_path(root: Path, relative: str) -> Path:
    path = (root / relative).resolve()
    if not path.is_relative_to(root.resolve()):
        raise ValueError(f"path escapes repository: {relative}")
    return path


def verify_entries(root: Path, entries: list[dict]) -> tuple[list[dict], list[str]]:
    result, errors, seen = [], [], set()
    for entry in entries:
        source = entry["path"]
        if source in seen:
            errors.append(f"DUPLICATE_UPSTREAM_PATH: {source}")
        seen.add(source)
        local = entry.get("local_path", source)
        path = safe_path(root, local)
        if not path.is_file():
            errors.append(f"UPSTREAM_SOURCE_MISSING: {source} -> {local}")
            continue
        data = canonical(path)
        oid = blob_oid(data)
        sha = hashlib.sha256(data).hexdigest()
        status = "identical" if oid == entry["blob"] else "adapted"
        if status == "adapted":
            if not entry.get("reason") or not entry.get("sha256"):
                errors.append(f"UNREVIEWED_ADAPTATION: {source}")
            elif sha != entry["sha256"]:
                errors.append(f"ADAPTATION_DRIFT: {source}; review diff and update its receipt")
        result.append({"path": source, "local_path": local, "status": status, "sha256": sha})
    return result, errors


def shipping_cfg(expression: str, features: frozenset[str] = frozenset({"std"})) -> bool:
    """Evaluate declaration feature cfgs for the actual shipping std profile.

    Unknown predicates stay excluded. This is source selection only, never an
    execution/behavior verification result.
    """
    tokens = re.findall(r'"[^"\\]*"|[A-Za-z_][A-Za-z_0-9]*|[=(),]', expression)
    position = 0

    def parse():
        nonlocal position
        if position >= len(tokens):
            raise ValueError("incomplete cfg")
        name = tokens[position]
        position += 1
        if position < len(tokens) and tokens[position] == "=":
            position += 1
            value = tokens[position].strip('"')
            position += 1
            return name == "feature" and value in features
        if position < len(tokens) and tokens[position] == "(":
            position += 1
            values = []
            while tokens[position] != ")":
                values.append(parse())
                if tokens[position] == ",":
                    position += 1
            position += 1
            if name == "all": return all(values)
            if name == "any": return any(values)
            if name == "not" and len(values) == 1: return not values[0]
        return False

    try:
        result = parse()
        return result and position == len(tokens)
    except (ValueError, IndexError):
        return False


def vm_routes(root: Path, upstream_paths: set[str]) -> list[dict]:
    """Read cfg declarations, not class-name presence or an API coverage claim."""
    text = (root / "crates/rd-vm/src/lib.rs").read_text(encoding="utf-8")
    declarations: dict[str, list[str]] = {}
    pattern = r'((?:(?:#\[[^\n]+\]|///[^\n]*)\n)*)pub(?:\(crate\))? mod (\w+);'
    for match in re.finditer(pattern, text):
        attrs, name = match.groups()
        if '#[path = "portable/' in attrs:
            continue
        original_path = re.search(r'#\[path = "(\w+)\.rs"\]', attrs)
        if original_path:
            name = original_path[1]
        declarations.setdefault(name, []).append(attrs)
    routes = []
    for path in sorted(upstream_paths):
        prefix = "crates/rd-vm/src/"
        if not path.startswith(prefix) or not path.endswith(".rs"):
            continue
        relative = path[len(prefix):]
        name = relative.split("/", 1)[0].removesuffix(".rs")
        attrs = declarations.get(name, [])
        shared = name == "lib" or any(all(
            shipping_cfg(expression) for expression in re.findall(r'#\[cfg\((.*?)\)\]', a)
        ) for a in attrs)
        status = "shared-source-with-adapters" if shared else "host-only-original"
        routes.append({"path": path, "device_route": status,
                       "cfg": attrs,
                       "experimental_original_declared": shared or any(
                           any(feature in a for feature in (
                               'feature = "upstream-runtime"',
                               'feature = "advanced-components"',
                               'feature = "virtual-devices"')) for a in attrs),
                       "experimental_scope": "cfg declaration only; compile and runtime evidence are separate",
                       "behavior_parity": "unverified"})
    return routes


def verify_vm_boundary(root: Path) -> list[str]:
    errors = []
    for package, expected in [("as-vm", "../rd-vm/src/lib.rs"),
                              ("as-vm-testsupport", "../rd-testsupport/src/lib.rs")]:
        manifest = root / "crates" / package / "Cargo.toml"
        if not manifest.is_file():
            errors.append(f"PORTABLE_PACKAGE_MISSING: {package}")
            continue
        data = tomllib.loads(manifest.read_text(encoding="utf-8"))
        if data.get("lib", {}).get("path") != expected:
            errors.append(f"DUPLICATED_UPSTREAM_SOURCE: {package} must use {expected}")
        if package == "as-vm" and "desktop-runtime" in data.get("features", {}):
            errors.append("DESKTOP_FEATURE_EXPOSED_ON_DEVICE_VM")
    for package in ["as-framework", "as-switch-smoke", "as-vm-testsupport"]:
        manifest = root / "crates" / package / "Cargo.toml"
        if not manifest.is_file():
            errors.append(f"PORTABLE_DEPENDENCY_MANIFEST_MISSING: {package}")
            continue
        dependency = tomllib.loads(manifest.read_text(encoding="utf-8")).get("dependencies", {}).get("rd-vm", {})
        if not isinstance(dependency, dict) or dependency.get("package") != "as-vm":
            errors.append(f"DESKTOP_VM_DEPENDENCY_LEAK: {package}")
    return errors


def verify_fork_core(root: Path, upstream: set[str], entries: list[dict]) -> list[str]:
    # Extracted helpers must not disappear from the audit just because their
    # new filenames did not exist in the original tree.
    actual = set()
    for package in ["rd-vm", "rd-dex", "rd-apk", "rd-jni", "rd-ndk"]:
        for path in (root / "crates" / package / "src").rglob("*.rs"):
            relative = path.relative_to(root).as_posix()
            if relative not in upstream:
                actual.add(relative)
    declared = {entry["path"] for entry in entries}
    errors = []
    if actual != declared or len(declared) != len(entries):
        errors.append("FORK_CORE_MANIFEST_INCOMPLETE_OR_CHANGED")
    for entry in entries:
        path = safe_path(root, entry["path"])
        if not path.is_file():
            errors.append(f"FORK_CORE_SOURCE_MISSING: {entry['path']}")
        elif hashlib.sha256(canonical(path)).hexdigest() != entry["sha256"]:
            errors.append(f"FORK_CORE_ADAPTATION_DRIFT: {entry['path']}")
    return errors


def upstream_test_routes(root: Path, upstream: set[str]) -> tuple[list[dict], list[str]]:
    """Every original integration suite must be registered, including backends.

    Registration is only a routing check. Zero-test targets, compilation, host
    execution, and Horizon execution are separate evidence and never a pass here.
    """
    expected = sorted(p for p in upstream if re.fullmatch(r"crates/rd-vm/tests/[^/]+\.rs", p))
    manifest = root / "crates/as-vm/Cargo.toml"
    if not manifest.is_file():
        return [], ["PORTABLE_TEST_MANIFEST_MISSING"] if expected else []
    declarations = tomllib.loads(manifest.read_text(encoding="utf-8")).get("test", [])
    by_path: dict[str, list[dict]] = {}
    for entry in declarations:
        target = (manifest.parent / entry.get("path", "")).resolve()
        if target.is_relative_to(root.resolve()):
            by_path.setdefault(target.relative_to(root.resolve()).as_posix(), []).append(entry)
    routes, errors = [], []
    for path in expected:
        entries = by_path.get(path, [])
        if len(entries) != 1:
            errors.append(f"UPSTREAM_TEST_ROUTE_MISSING_OR_DUPLICATED: {path}")
        elif "upstream-parity-tests" not in entries[0].get("required-features", []):
            errors.append(f"UPSTREAM_TEST_ROUTE_NOT_ISOLATED: {path}")
        routes.append({"path": path, "targets": [e["name"] for e in entries],
                       "execution": "unverified"})
    return routes, errors


def audit(root: Path, lock: dict, check_latest: bool = False) -> dict:
    revision = lock["revision"]
    # Checking the actual Git tree prevents an accidentally shortened manifest
    # from turning missing files into a green source gate.
    tree = {}
    for line in git(root, "ls-tree", "-r", revision).splitlines():
        meta, path = line.split("\t", 1)
        tree[path] = meta.split()[2]
    declared = {entry["path"]: entry["blob"] for entry in lock["files"]}
    files, errors = verify_entries(root, lock["files"])
    errors.extend(verify_vm_boundary(root))
    errors.extend(verify_fork_core(root, set(tree), lock.get("fork_core_files", [])))
    test_routes, test_errors = upstream_test_routes(root, set(tree))
    errors.extend(test_errors)
    if tree != declared:
        errors.append("UPSTREAM_MANIFEST_INCOMPLETE_OR_CHANGED")
    if git(root, "show", "-s", "--format=%T", revision) != lock["tree"]:
        errors.append("UPSTREAM_TREE_ID_MISMATCH")
    if check_latest:
        latest = git(root, "ls-remote", lock["repository"], "refs/heads/main").split()[0]
        if latest != revision:
            errors.append(f"NEW_UPSTREAM_REVISION: {latest}; pinned {revision}")
    modules = vm_routes(root, set(tree))
    excluded = [m["path"] for m in modules if m["device_route"] == "host-only-original"]
    return {
        "schema": 1, "upstream": revision, "source_sync_pass": not errors,
        "source_files": len(tree),
        "identical_files": sum(f["status"] == "identical" for f in files),
        "adapted_files": sum(f["status"] == "adapted" for f in files),
        "relocated_files": sum(f["path"] != f["local_path"] for f in files),
        "errors": errors, "files": files, "vm_modules": modules,
        "fork_core_files": lock.get("fork_core_files", []),
        "host_only_originals": excluded,
        "host_only_originals_scope": "Exclusions in the shipping/default device profile, not the experimental upstream-runtime profile",
        "upstream_test_routes": test_routes,
        # Compilation, source equality and aggregate hardware test counts are
        # insufficient for claiming all upstream behavior works on Horizon.
        "device_behavior_parity_pass": not excluded and bool(modules) and all(m["behavior_parity"] == "verified" for m in modules),
        "device_behavior_parity_reason": "Target adapters and complete upstream behavior corpus require verification",
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path)
    parser.add_argument("--check-latest", action="store_true")
    parser.add_argument("--require-device-parity", action="store_true")
    args = parser.parse_args()
    lock = json.loads((ROOT / LOCK).read_text(encoding="utf-8"))
    result = audit(ROOT, lock, args.check_latest)
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(f"Upstream {result['upstream']}: {result['source_files']} files; "
          f"{result['identical_files']} identical, {result['adapted_files']} reviewed adaptations, "
          f"{result['relocated_files']} relocated.")
    for error in result["errors"]:
        print(error)
    print(f"Original VM source excluded on device: {len(result['host_only_originals'])}. "
          "Device behavior parity: NOT VERIFIED.")
    return int(not result["source_sync_pass"] or (args.require_device_parity and not result["device_behavior_parity_pass"]))


if __name__ == "__main__":
    raise SystemExit(main())
