"""Static and differential source audit for RD Sentinel."""

from __future__ import annotations

import json
import os
import posixpath
import re
import subprocess
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Iterable, Mapping, Sequence

try:
    import tomllib
except ModuleNotFoundError:  # Python < 3.11 fallback uses narrow parsers below.
    tomllib = None  # type: ignore[assignment]

from .model import (
    CheckResult,
    Finding,
    apply_exceptions,
    detect_secret_kind,
    match_any,
    sha256_file,
    validate_baseline,
    validate_policy,
)

HIGH_RISK_AREAS = {
    "parsers",
    "vm-core",
    "native-security",
    "agent-protocol",
    "performance",
    "dependencies-build",
    "ci-security",
}

AREA_WEIGHTS = {
    "native-security": 28,
    "vm-core": 24,
    "parsers": 22,
    "agent-protocol": 20,
    "dependencies-build": 18,
    "performance": 18,
    "render-ui": 14,
    "cli-contract": 12,
    "ci-security": 16,
    "fuzz": 10,
    "tests": 4,
    "docs": 1,
    "sentinel": 20,
}


@dataclass(slots=True)
class ChangedFile:
    status: str
    path: str
    old_path: str | None = None
    added: int | None = None
    deleted: int | None = None


@dataclass(slots=True)
class DiffLine:
    path: str
    line: int | None
    text: str
    kind: str  # added | removed


@dataclass(slots=True)
class AuditContext:
    event_name: str = "local"
    campaign: str = "local"
    profile: str = "auto"
    actor: str = ""
    author: str = ""
    head_ref: str = ""
    labels: list[str] = field(default_factory=list)
    pr_number: int | None = None
    run_url: str = ""
    repository: str = ""


def git_output(repo: Path, args: Sequence[str], *, check: bool = False) -> str:
    proc = subprocess.run(
        ["git", "-C", str(repo), *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    if check and proc.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} failed: {proc.stderr.strip()}")
    return proc.stdout if proc.returncode == 0 else ""


def _safe_policy_int(value: Any, default: int) -> int:
    if isinstance(value, bool):
        return default
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


def resolve_rev(repo: Path, rev: str) -> str | None:
    out = git_output(repo, ["rev-parse", "--verify", f"{rev}^{{commit}}"])
    value = out.strip()
    return value or None


def read_at_rev(repo: Path, rev: str | None, path: str) -> str | None:
    if rev is None or rev == "WORKTREE":
        target = repo / path
        try:
            return target.read_text(encoding="utf-8", errors="replace")
        except OSError:
            return None
    out = git_output(repo, ["show", f"{rev}:{path}"])
    return out if out else None


def list_files_at_rev(repo: Path, rev: str | None, prefix: str = "") -> list[str]:
    if rev is None or rev == "WORKTREE":
        out = git_output(repo, ["ls-files", "--cached", "--others", "--exclude-standard"])
        files = [line.strip() for line in out.splitlines() if line.strip()]
    else:
        args = ["ls-tree", "-r", "--name-only", rev]
        if prefix:
            args.extend(["--", prefix])
        out = git_output(repo, args)
        files = [line.strip() for line in out.splitlines() if line.strip()]
    if prefix:
        files = [f for f in files if f.startswith(prefix.rstrip("/") + "/")]
    return sorted(set(files))


def collect_changed_files(repo: Path, base_rev: str, head: str) -> list[ChangedFile]:
    if head == "WORKTREE":
        name_status = git_output(repo, ["diff", "--name-status", "--find-renames", base_rev])
        numstat = git_output(repo, ["diff", "--numstat", "--find-renames", base_rev])
    else:
        spec = f"{base_rev}...{head}"
        name_status = git_output(repo, ["diff", "--name-status", "--find-renames", spec])
        numstat = git_output(repo, ["diff", "--numstat", "--find-renames", spec])

    counts: dict[str, tuple[int | None, int | None]] = {}
    for raw in numstat.splitlines():
        parts = raw.split("\t")
        if len(parts) < 3:
            continue
        path_field = parts[-1]
        path = path_field.rsplit(" -> ", 1)[-1].strip()
        try:
            counts[path] = (int(parts[0]), int(parts[1]))
        except ValueError:
            counts[path] = (None, None)

    changed: list[ChangedFile] = []
    seen: set[str] = set()
    for raw in name_status.splitlines():
        parts = raw.split("\t")
        if len(parts) < 2:
            continue
        status = parts[0]
        old_path = None
        path = parts[-1]
        if status.startswith(("R", "C")) and len(parts) >= 3:
            old_path = parts[1]
            path = parts[2]
        added_deleted = counts.get(path, (None, None))
        changed.append(ChangedFile(status, path, old_path, added_deleted[0], added_deleted[1]))
        seen.add(path)

    if head == "WORKTREE":
        untracked = git_output(repo, ["ls-files", "--others", "--exclude-standard"])
        for path in (p.strip() for p in untracked.splitlines() if p.strip()):
            if path in seen:
                continue
            line_count = None
            try:
                line_count = len((repo / path).read_text(encoding="utf-8", errors="ignore").splitlines())
            except OSError:
                pass
            changed.append(ChangedFile("A", path, None, line_count, 0))
    return sorted(changed, key=lambda c: c.path)


def collect_diff_lines(repo: Path, base_rev: str, head: str, changed: Sequence[ChangedFile]) -> list[DiffLine]:
    if head == "WORKTREE":
        diff = git_output(repo, ["diff", "--find-renames", "--unified=0", base_rev])
    else:
        diff = git_output(repo, ["diff", "--find-renames", "--unified=0", f"{base_rev}...{head}"])

    lines: list[DiffLine] = []
    current_old_path = ""
    current_new_path = ""
    new_line: int | None = None
    old_line: int | None = None
    hunk_re = re.compile(r"^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@")
    header_re = re.compile(r"^diff --git a/(.*) b/(.*)$")
    for raw in diff.splitlines():
        header = header_re.match(raw)
        if header:
            current_old_path = header.group(1)
            current_new_path = header.group(2)
            old_line = None
            new_line = None
            continue
        if raw.startswith("--- "):
            value = raw[4:].strip()
            current_old_path = "" if value == "/dev/null" else value[2:] if value.startswith("a/") else value
            continue
        if raw.startswith("+++ "):
            value = raw[4:].strip()
            current_new_path = "" if value == "/dev/null" else value[2:] if value.startswith("b/") else value
            continue
        m = hunk_re.match(raw)
        if m:
            old_line = int(m.group(1))
            new_line = int(m.group(2))
            continue
        if raw.startswith("+") and not raw.startswith("+++"):
            lines.append(DiffLine(current_new_path or current_old_path, new_line, raw[1:], "added"))
            if new_line is not None:
                new_line += 1
        elif raw.startswith("-") and not raw.startswith("---"):
            lines.append(DiffLine(current_old_path or current_new_path, old_line, raw[1:], "removed"))
            if old_line is not None:
                old_line += 1

    if head == "WORKTREE":
        tracked = set(git_output(repo, ["ls-files"]).splitlines())
        for item in changed:
            if item.path in tracked or item.status not in {"A", "?"}:
                continue
            target = repo / item.path
            try:
                if target.stat().st_size > 2_000_000:
                    continue
                text = target.read_text(encoding="utf-8", errors="ignore")
            except OSError:
                continue
            for idx, text_line in enumerate(text.splitlines()[:5000], 1):
                lines.append(DiffLine(item.path, idx, text_line, "added"))
    return lines


def classify_path(path: str) -> set[str]:
    p = path.replace("\\", "/")
    areas: set[str] = set()
    lower = p.lower()
    if p.startswith(("crates/rd-apk/", "crates/rd-dex/")):
        areas.update({"parsers", "runtime"})
    if p.startswith("crates/rd-vm/"):
        areas.update({"vm-core", "runtime"})
    if p.startswith(("crates/rd-jni/", "crates/rd-ndk/")) or "/native" in lower or "jni" in lower:
        areas.update({"native-security", "runtime"})
    if p.startswith("crates/rd-render/") or any(x in lower for x in ("inflate", "ui_", "view", "graphics", "render")):
        areas.update({"render-ui", "runtime"})
    if p.startswith("crates/rd-agent/"):
        areas.update({"agent-protocol", "runtime"})
    if p.startswith(("crates/rd-cli/", "crates/rd-framework/")):
        areas.update({"cli-contract", "runtime"})
    if p.endswith(("Cargo.toml", "Cargo.lock")) or "/fuzz/Cargo.toml" in p:
        areas.add("dependencies-build")
    if p.startswith(".github/workflows/"):
        areas.add("ci-security")
    if "/fuzz/" in p or p.startswith("crates/rd-apk/fuzz") or p.startswith("crates/rd-dex/fuzz"):
        areas.add("fuzz")
    if any(x in lower for x in ("perf", "cold26", "snapshot", "gc.rs", "jit", "efficiency", "benchmark")):
        areas.add("performance")
    if p.startswith(("tests/", "golden/", "golden-apks/")) or "/tests/" in p or p.endswith(("_test.rs", "_tests.rs")):
        areas.add("tests")
    if p.startswith("docs/") or p.endswith(".md"):
        areas.add("docs")
    if p.startswith(("tools/rd-sentinel/", ".github/rd-sentinel/")) or "rd-sentinel" in p:
        areas.add("sentinel")
    return areas


def areas_for_files(files: Sequence[ChangedFile]) -> list[str]:
    found: set[str] = set()
    for item in files:
        found.update(classify_path(item.path))
        if item.old_path:
            found.update(classify_path(item.old_path))
    return sorted(found)


def risk_assessment(
    files: Sequence[ChangedFile],
    areas: Sequence[str],
    added: int,
    deleted: int,
    *,
    downstream_packages: int = 0,
) -> tuple[int, list[dict[str, Any]]]:
    """Return an explainable 0-100 risk score.

    The factors are part of the fingerprinted report. A reviewer should be
    able to see whether risk came from a sensitive area, raw churn, deletion,
    a lockfile, or downstream workspace blast radius instead of trusting one
    opaque number.
    """

    factors: list[dict[str, Any]] = []
    score = 0

    def add(fid: str, label: str, points: int, evidence: str) -> None:
        nonlocal score
        if points <= 0:
            return
        score += points
        factors.append(
            {"id": fid, "label": label, "points": int(points), "evidence": evidence}
        )

    for area in areas:
        add(f"area:{area}", f"Sensitive area: {area}", AREA_WEIGHTS.get(area, 0), area)
    add("file-churn", "Changed-file count", min(20, len(files) // 8), f"{len(files)} files")
    add(
        "line-churn",
        "Changed-line count",
        min(20, (added + deleted) // 1000),
        f"+{added}/-{deleted} lines",
    )
    deleted_files = [f.path for f in files if f.status.startswith("D")]
    if deleted_files:
        add("file-deletion", "Files deleted", 8, ", ".join(deleted_files[:8]))
    if any(f.path.endswith("Cargo.lock") for f in files):
        add("lockfile", "Cargo.lock changed", 10, "resolved dependency graph moved")
    if downstream_packages:
        add(
            "downstream-blast-radius",
            "Downstream workspace packages affected",
            min(12, downstream_packages * 2),
            f"{downstream_packages} downstream package(s)",
        )
    capped = max(0, min(100, score))
    if score > capped:
        factors.append(
            {
                "id": "score-cap",
                "label": "Risk score cap",
                "points": capped - score,
                "evidence": "raw factor total exceeds the 100-point report scale",
            }
        )
    return capped, sorted(factors, key=lambda f: (-int(f["points"]), str(f["id"])))


def risk_score(files: Sequence[ChangedFile], areas: Sequence[str], added: int, deleted: int) -> int:
    return risk_assessment(files, areas, added, deleted)[0]


def targeted_packages(files: Sequence[ChangedFile]) -> list[str]:
    mapping = {
        "crates/rd-apk/": "rd-apk",
        "crates/rd-dex/": "rd-dex",
        "crates/rd-vm/": "rd-vm",
        "crates/rd-jni/": "rd-jni",
        "crates/rd-ndk/": "rd-ndk",
        "crates/rd-render/": "rd-render",
        "crates/rd-engine-registry/": "rd-engine-registry",
        "crates/rd-agent/": "rd-agent",
        "crates/rd-cli/": "rd-cli",
        "crates/rd-framework/": "rd-framework",
        "crates/rd-testsupport/": "rd-testsupport",
    }
    found: list[str] = []
    for item in files:
        for prefix, pkg in mapping.items():
            if item.path.startswith(prefix) and pkg not in found:
                found.append(pkg)
    return found


def _manifest_dependency_package_name(trail_key: str, spec: Any) -> str:
    if isinstance(spec, Mapping) and spec.get("package"):
        return str(spec["package"])
    return trail_key.rsplit(".", 1)[-1]


def workspace_graph_from_manifests(
    manifests: Mapping[str, Mapping[str, Any]],
) -> dict[str, set[str]]:
    """Build package -> workspace-dependency edges from parsed manifests."""

    package_names: set[str] = set()
    parsed_by_package: dict[str, Mapping[str, Any]] = {}
    for manifest in manifests.values():
        package = manifest.get("package", {})
        if isinstance(package, Mapping) and package.get("name"):
            name = str(package["name"])
            package_names.add(name)
            parsed_by_package[name] = manifest

    graph: dict[str, set[str]] = {name: set() for name in package_names}
    for name, manifest in parsed_by_package.items():
        for key, spec in _collect_dependencies(manifest).items():
            dep_name = _manifest_dependency_package_name(str(key), spec)
            if dep_name in package_names and dep_name != name:
                graph[name].add(dep_name)
    return graph


def workspace_dependency_graph(repo: Path, rev: str | None) -> dict[str, set[str]]:
    manifests: dict[str, Mapping[str, Any]] = {}
    for path in list_files_at_rev(repo, rev):
        if not path.endswith("Cargo.toml"):
            continue
        parsed = parse_toml(read_at_rev(repo, rev, path))
        if parsed is not None:
            manifests[path] = parsed

    root = manifests.get("Cargo.toml", {})
    workspace = root.get("workspace", {}) if isinstance(root, Mapping) else {}
    members = workspace.get("members", []) if isinstance(workspace, Mapping) else []
    excludes = workspace.get("exclude", []) if isinstance(workspace, Mapping) else []
    if isinstance(members, list) and members:
        member_patterns = [str(item).strip("/") for item in members if str(item).strip("/")]
        exclude_patterns = (
            [str(item).strip("/") for item in excludes if str(item).strip("/")]
            if isinstance(excludes, list)
            else []
        )
        selected: dict[str, Mapping[str, Any]] = {}
        for path, parsed in manifests.items():
            if path == "Cargo.toml":
                continue
            directory = path[: -len("/Cargo.toml")] if path.endswith("/Cargo.toml") else ""
            if not directory:
                continue
            if directory in member_patterns or match_any(directory, member_patterns):
                if exclude_patterns and (
                    directory in exclude_patterns or match_any(directory, exclude_patterns)
                ):
                    continue
                selected[path] = parsed
        if selected:
            return workspace_graph_from_manifests(selected)

    # Fallback for a manifest shape we cannot interpret: exclude nested fuzz
    # workspaces, which are not selectable with `cargo test -p` from the root.
    fallback = {
        path: parsed
        for path, parsed in manifests.items()
        if "/fuzz/" not in f"/{path}"
    }
    return workspace_graph_from_manifests(fallback)


def affected_packages(
    repo: Path, rev: str | None, files: Sequence[ChangedFile]
) -> tuple[list[str], list[str]]:
    """Return (direct packages, direct + downstream workspace dependents).

    A parser fix in ``rd-dex`` can regress every workspace consumer above it,
    even when no consumer file changed. Sentinel's targeted campaign therefore
    follows reverse workspace edges instead of only testing the edited crate.
    """

    direct = targeted_packages(files)
    graph = workspace_dependency_graph(repo, rev)
    if not graph:
        return direct, direct
    if any(f.path in {"Cargo.toml", "Cargo.lock"} for f in files):
        return direct, sorted(graph)

    reverse: dict[str, set[str]] = {name: set() for name in graph}
    for package, deps in graph.items():
        for dep in deps:
            reverse.setdefault(dep, set()).add(package)

    ordered: list[str] = []
    seen: set[str] = set()

    def push(name: str) -> None:
        if name not in seen:
            seen.add(name)
            ordered.append(name)

    for package in direct:
        push(package)
    frontier = list(direct)
    while frontier:
        next_frontier: list[str] = []
        for package in frontier:
            for dependent in sorted(reverse.get(package, ())):
                if dependent not in seen:
                    push(dependent)
                    next_frontier.append(dependent)
        frontier = next_frontier
    return direct, ordered


def build_plan(
    *,
    profile: str,
    areas: Sequence[str],
    files: Sequence[ChangedFile],
    labels: Sequence[str],
    risk: int,
    dependency_bot: bool,
) -> list[dict[str, Any]]:
    plan: list[dict[str, Any]] = []

    def add(component: str, reason: str, required: bool = True) -> None:
        if not any(p["component"] == component for p in plan):
            plan.append({"component": component, "reason": reason, "required": required})

    add("static", "Trusted policy, diff, contract, and evidence audit")
    area_set = set(areas)
    labels_set = set(labels)
    code_changed = any(not f.path.endswith((".md", ".txt", ".yml", ".yaml")) for f in files)

    if profile in {"plan", "static"}:
        return plan

    if profile == "auto":
        if code_changed:
            add("core", "Changed code selects targeted package tests")
        if area_set & {"dependencies-build", "vm-core", "agent-protocol", "native-security", "parsers"}:
            add("features", "Runtime or dependency change selects relevant feature combinations")
        if area_set & {"vm-core", "agent-protocol", "parsers", "render-ui", "native-security"}:
            add("determinism", "Runtime-visible change needs repeated-output evidence")
        if "dependencies-build" in area_set or dependency_bot:
            add("deps", "Manifest/lockfile change needs supply-chain vetting")
        if "parsers" in area_set or "fuzz" in area_set:
            add("fuzz", "Parser/fuzz surface changed; CI only builds fuzz targets")
        if "sentinel:mutants" in labels_set:
            add("mutation", "Maintainer requested changed-code mutation testing")
        if "performance" in area_set or "sentinel:deep" in labels_set or "sentinel:perf" in labels_set:
            add("perf", "Performance-sensitive area or maintainer deep request")
        if risk >= 75 or "sentinel:deep" in labels_set:
            add("compare", "High diff risk needs base-versus-head differential evidence")
            add("real-apk", "High diff risk needs pinned real-APK evidence")
        return plan

    if profile in {"deep", "nightly", "weekly"}:
        add("core", "Deep campaign runs broad and all-feature test surfaces")
        add("features", "Deep campaign enumerates feature combinations")
        add("determinism", "Deep campaign hunts flakes and output drift")
        add("deps", "Deep campaign audits dependencies, licenses, sources, and MSRV")
        add("future", "Deep campaign trials beta/future and MSRV toolchains")
        add("perf", "Deep campaign measures absolute performance/resource budgets")
        add("real-apk", "Deep campaign exercises pinned real APKs")
        add("fuzz", "Deep campaign executes fuzz targets; CI only builds them")
        if profile in {"deep", "weekly"}:
            add("mutation", "Deep/weekly campaign checks whether tests catch injected changed-code bugs")
            add("compare", "Deep campaign compares candidate behavior to base")
        if profile == "weekly":
            add("soak", "Weekly campaign runs bounded native/MCP endurance")
            add("golden", "Weekly campaign repeats the JVM golden comparison")
            add("dep-trial", "Weekly campaign trials semver-compatible dependency updates in isolation")
        return plan

    mapping = {
        "perf": ["core", "perf", "compare", "real-apk"],
        "fuzz": ["core", "fuzz"],
        "deps": ["core", "features", "deps", "dep-trial"],
        "determinism": ["core", "determinism"],
        "mutants": ["core", "mutation"],
        "soak": ["core", "real-apk", "soak"],
    }
    for component in mapping.get(profile, []):
        add(component, f"Maintainer-selected {profile} campaign")
    return plan


def parse_toml(text: str | None) -> dict[str, Any] | None:
    if text is None or tomllib is None:
        return None
    try:
        return tomllib.loads(text)
    except Exception:
        return None


def parse_cargo_lock(text: str | None) -> dict[tuple[str, str], dict[str, Any]]:
    result: dict[tuple[str, str], dict[str, Any]] = {}
    if not text:
        return result
    parsed = parse_toml(text)
    if parsed is not None:
        for pkg in parsed.get("package", []):
            if isinstance(pkg, Mapping) and pkg.get("name") and pkg.get("version"):
                result[(str(pkg["name"]), str(pkg["version"]))] = dict(pkg)
        return result
    current: dict[str, Any] | None = None
    for line in text.splitlines():
        if line.strip() == "[[package]]":
            if current and current.get("name") and current.get("version"):
                result[(str(current["name"]), str(current["version"]))] = current
            current = {}
            continue
        if current is not None:
            m = re.match(r'^(name|version|source|checksum)\s*=\s*"([^"]*)"', line.strip())
            if m:
                current[m.group(1)] = m.group(2)
    if current and current.get("name") and current.get("version"):
        result[(str(current["name"]), str(current["version"]))] = current
    return result


def lock_version(text: str | None) -> int | None:
    parsed = parse_toml(text)
    if parsed is not None and isinstance(parsed.get("version"), int):
        return int(parsed["version"])
    if text:
        m = re.search(r"^version\s*=\s*(\d+)", text, re.M)
        if m:
            return int(m.group(1))
    return None


def parse_semver(version: str) -> tuple[tuple[int, int, int], str] | None:
    core = str(version).split("+", 1)[0]
    main, _, prerelease = core.partition("-")
    parts = main.split(".")
    if not parts or any(not p.isdigit() for p in parts):
        return None
    numbers = [int(p) for p in parts[:3]]
    while len(numbers) < 3:
        numbers.append(0)
    return (numbers[0], numbers[1], numbers[2]), prerelease


def classify_semver_change(old: str, new: str) -> str:
    if old == new:
        return "none"
    a = parse_semver(old)
    b = parse_semver(new)
    if a is None or b is None:
        return "unknown"
    (a_major, a_minor, a_patch), a_pre = a
    (b_major, b_minor, b_patch), b_pre = b
    if a_major != b_major:
        return "major"
    if a_major == 0 and a_minor != b_minor:
        return "major"
    if a_minor != b_minor:
        return "minor"
    if a_patch != b_patch:
        return "patch"
    if a_pre != b_pre:
        return "prerelease"
    return "metadata"


def compare_cargo_locks(
    base_text: str | None, head_text: str | None
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    base = parse_cargo_lock(base_text)
    head = parse_cargo_lock(head_text)
    base_by_name: dict[str, list[str]] = {}
    head_by_name: dict[str, list[str]] = {}
    for name, version in base:
        base_by_name.setdefault(name, []).append(version)
    for name, version in head:
        head_by_name.setdefault(name, []).append(version)

    changes: list[dict[str, Any]] = []
    for name in sorted(set(base_by_name) | set(head_by_name)):
        old_versions = sorted(base_by_name.get(name, []))
        new_versions = sorted(head_by_name.get(name, []))
        if old_versions == new_versions:
            for version in old_versions:
                old_pkg = base.get((name, version), {})
                new_pkg = head.get((name, version), {})
                if old_pkg.get("source") != new_pkg.get("source") or old_pkg.get("checksum") != new_pkg.get("checksum"):
                    changes.append(
                        {
                            "name": name,
                            "old": version,
                            "new": version,
                            "kind": "source-or-checksum",
                            "old_source": old_pkg.get("source"),
                            "new_source": new_pkg.get("source"),
                            "old_checksum": old_pkg.get("checksum"),
                            "new_checksum": new_pkg.get("checksum"),
                        }
                    )
            continue
        common_old = old_versions[0] if old_versions else None
        common_new = new_versions[0] if new_versions else None
        kind = "added" if not old_versions else "removed" if not new_versions else classify_semver_change(common_old or "", common_new or "")
        old_pkg = base.get((name, common_old), {}) if common_old else {}
        new_pkg = head.get((name, common_new), {}) if common_new else {}
        changes.append(
            {
                "name": name,
                "old": common_old,
                "new": common_new,
                "old_versions": old_versions,
                "new_versions": new_versions,
                "kind": kind,
                "old_source": old_pkg.get("source"),
                "new_source": new_pkg.get("source"),
                "old_checksum": old_pkg.get("checksum"),
                "new_checksum": new_pkg.get("checksum"),
            }
        )
    base_duplicates = sum(1 for versions in base_by_name.values() if len(set(versions)) > 1)
    head_duplicates = sum(1 for versions in head_by_name.values() if len(set(versions)) > 1)
    metrics = {
        "dependency_changes": len(changes),
        "dependency_major_changes": sum(1 for c in changes if c.get("kind") == "major"),
        "dependency_added": sum(1 for c in changes if c.get("kind") == "added"),
        "dependency_removed": sum(1 for c in changes if c.get("kind") == "removed"),
        "dependency_base_packages": len(base),
        "dependency_head_packages": len(head),
        "dependency_duplicate_names_base": base_duplicates,
        "dependency_duplicate_names_head": head_duplicates,
        "dependency_duplicate_names_delta": head_duplicates - base_duplicates,
    }
    return changes, metrics


def _collect_dependencies(value: Any, trail: str = "") -> dict[str, Any]:
    found: dict[str, Any] = {}
    if isinstance(value, Mapping):
        for key, child in value.items():
            child_trail = f"{trail}.{key}" if trail else str(key)
            if key in {"dependencies", "dev-dependencies", "build-dependencies"} and isinstance(child, Mapping):
                for name, spec in child.items():
                    found[f"{child_trail}.{name}"] = spec
            else:
                found.update(_collect_dependencies(child, child_trail))
    elif isinstance(value, list):
        for idx, child in enumerate(value):
            found.update(_collect_dependencies(child, f"{trail}[{idx}]"))
    return found


def _path_dependency_escapes(manifest_path: str, dependency_path: str) -> bool:
    if dependency_path.startswith(("/", "\\")) or re.match(r"^[A-Za-z]:[\\/]", dependency_path):
        return True
    normalized = posixpath.normpath(
        posixpath.join(posixpath.dirname(manifest_path), dependency_path.replace("\\", "/"))
    )
    return normalized == ".." or normalized.startswith("../")


def compare_manifests(
    repo: Path, base_rev: str, head_rev: str | None
) -> tuple[list[dict[str, Any]], list[Finding]]:
    if tomllib is None:
        return [], []
    base_files = {p for p in list_files_at_rev(repo, base_rev) if p.endswith("Cargo.toml")}
    head_files = {p for p in list_files_at_rev(repo, head_rev) if p.endswith("Cargo.toml")}
    changes: list[dict[str, Any]] = []
    findings: list[Finding] = []
    for path in sorted(base_files | head_files):
        base = parse_toml(read_at_rev(repo, base_rev, path)) or {}
        head = parse_toml(read_at_rev(repo, head_rev, path)) or {}
        base_deps = _collect_dependencies(base)
        head_deps = _collect_dependencies(head)
        for key in sorted(set(base_deps) | set(head_deps)):
            if base_deps.get(key) == head_deps.get(key):
                continue
            kind = "changed"
            if key not in base_deps:
                kind = "added"
            elif key not in head_deps:
                kind = "removed"
            changes.append({"manifest": path, "dependency": key, "kind": kind, "old": base_deps.get(key), "new": head_deps.get(key)})
            spec = head_deps.get(key)
            unsafe_source: tuple[str, str] | None = None
            if isinstance(spec, Mapping) and "git" in spec:
                rev = str(spec.get("rev", ""))
                pinned = bool(re.fullmatch(r"[0-9a-fA-F]{40}", rev))
                unsafe_source = (
                    "P1" if pinned else "P0",
                    "git dependency is pinned to a full revision"
                    if pinned
                    else "git dependency is not pinned to a full immutable revision",
                )
            elif isinstance(spec, Mapping) and "path" in spec:
                if _path_dependency_escapes(path, str(spec.get("path", ""))):
                    unsafe_source = ("P1", "path dependency escapes the repository")
            if unsafe_source is not None:
                severity, why = unsafe_source
                findings.append(
                    Finding(
                        id="non-registry-dependency-source",
                        severity=severity,
                        area="dependencies-build",
                        title="Dependency uses a non-registry source",
                        evidence=f"{key} in {path}: {why}; spec={spec}",
                        path=path,
                        remediation="Use a reviewed crates.io release, an in-tree workspace path, or document the source in an ADR and pin an immutable revision.",
                    )
                )
            elif kind == "added":
                findings.append(
                    Finding(
                        id="new-dependency-review",
                        severity="P2",
                        area="dependencies-build",
                        title="New dependency requires supply-chain review",
                        evidence=f"{key} added in {path}: {spec}",
                        path=path,
                        remediation="Record license, MSRV, transitive size, and maintenance rationale in the PR.",
                    )
                )

        base_features = base.get("features", {}) if isinstance(base.get("features"), Mapping) else {}
        head_features = head.get("features", {}) if isinstance(head.get("features"), Mapping) else {}
        if base_features != head_features:
            removed = sorted(set(base_features) - set(head_features))
            default_changed = base_features.get("default") != head_features.get("default")
            if removed or default_changed:
                findings.append(
                    Finding(
                        id="feature-contract-change",
                        severity="P1",
                        area="dependencies-build",
                        title="Cargo feature contract changed",
                        evidence=f"{path}: removed={removed}, default_changed={default_changed}",
                        path=path,
                        remediation="Add an ADR/migration note and run the full feature powerset before merging.",
                    )
                )
    return changes, findings


def license_expression_allowed(expression: str | None, allowed: Iterable[str]) -> bool:
    if not expression:
        return False
    allowed_set = {str(x) for x in allowed}
    normalized = str(expression).replace("/", " OR ")
    tokens = re.findall(r"\(|\)|[A-Za-z0-9][A-Za-z0-9.+-]*", normalized)
    pos = 0

    def peek() -> str | None:
        return tokens[pos] if pos < len(tokens) else None

    def parse_or() -> bool:
        nonlocal pos
        value = parse_and()
        while peek() and peek().upper() == "OR":
            pos += 1
            rhs = parse_and()
            value = value or rhs
        return value

    def parse_and() -> bool:
        nonlocal pos
        value = parse_atom()
        while peek() and peek().upper() == "AND":
            pos += 1
            rhs = parse_atom()
            value = value and rhs
        return value

    def parse_atom() -> bool:
        nonlocal pos
        token = peek()
        if token is None:
            raise ValueError("unexpected end")
        if token == "(":
            pos += 1
            value = parse_or()
            if peek() != ")":
                raise ValueError("missing )")
            pos += 1
            return value
        pos += 1
        if peek() and peek().upper() == "WITH":
            pos += 1
            if peek() is not None:
                pos += 1  # exception identifier; the base license still decides
        return token in allowed_set

    try:
        result = parse_or()
        return result and pos == len(tokens)
    except Exception:
        return False


def _count_test_attributes(repo: Path, rev: str | None) -> int:
    if rev and rev != "WORKTREE":
        out = git_output(repo, ["grep", "-h", "-E", r"#\[test\]", rev, "--", "*.rs"])
        return len([line for line in out.splitlines() if "#[test]" in line])
    total = 0
    for path in list_files_at_rev(repo, rev):
        if not path.endswith(".rs"):
            continue
        try:
            total += (repo / path).read_text(encoding="utf-8", errors="ignore").count("#[test]")
        except OSError:
            continue
    return total


_TEST_ATTR_RE = re.compile(r"#\[(?:[A-Za-z_][\w:]*::)?test(?:\([^\]]*\))?\]")
_TEST_FN_RE = re.compile(r"\b(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(")


def _test_scope(path: str) -> str:
    parts = path.replace("\\", "/").split("/")
    if len(parts) > 2 and parts[0] == "crates":
        return parts[1]
    return parts[0] if parts else "workspace"


def named_test_inventory_from_sources(
    sources: Mapping[str, str],
) -> dict[tuple[str, str], int]:
    """Count named Rust tests by (scope, test name).

    Counts alone allow a bad swap: delete the one test that catches a bug and
    add an unrelated smoke test, and the total still looks healthy. This
    inventory tracks identities. The scope is the crate for ``crates/<crate>``
    paths (or the first path segment elsewhere) rather than the exact file, so
    moving a test module inside the same crate is not misreported as removal;
    duplicate test names in different modules of one crate are counted.
    """

    counts: Counter[tuple[str, str]] = Counter()
    for path, text in sources.items():
        lines = text.splitlines()
        scope = _test_scope(path)
        counted_fn_lines: set[int] = set()
        for index, line in enumerate(lines):
            if not _TEST_ATTR_RE.search(line):
                continue
            for fn_index in range(index + 1, min(len(lines), index + 13)):
                candidate = lines[fn_index].strip()
                if not candidate or candidate.startswith("#[") or candidate.startswith("//"):
                    continue
                match = _TEST_FN_RE.search(lines[fn_index])
                if match and fn_index not in counted_fn_lines:
                    counts[(scope, match.group(1))] += 1
                    counted_fn_lines.add(fn_index)
                break
    return dict(counts)


def _named_test_inventory(repo: Path, rev: str | None) -> dict[tuple[str, str], int]:
    sources: dict[str, str] = {}
    for path in list_files_at_rev(repo, rev):
        if not path.endswith(".rs"):
            continue
        text = read_at_rev(repo, rev, path)
        if text is not None:
            sources[path] = text
    return named_test_inventory_from_sources(sources)


def _named_test_findings(
    base: Mapping[tuple[str, str], int],
    head: Mapping[tuple[str, str], int],
    *,
    baseline_approved: bool = False,
) -> tuple[list[Finding], dict[str, Any]]:
    findings: list[Finding] = []
    decreased: list[tuple[str, str, int, int]] = []
    removed_instances = 0
    for (scope, name), before in sorted(base.items()):
        after = head.get((scope, name), 0)
        if after < before:
            decreased.append((scope, name, before, after))
            removed_instances += before - after
    for scope, name, before, after in decreased[:50]:
        path = f"crates/{scope}" if scope and scope not in {"tests", "golden"} else scope
        findings.append(
            Finding(
                id="test-removed",
                severity="P2" if baseline_approved else "P1",
                area="tests",
                title=f"Named test removed or renamed without same-name replacement: {name}",
                evidence=f"scope={scope}; base count={before}; head count={after}",
                path=path,
                remediation="Restore the named test, or keep the historical name when moving it inside the same crate. If the behavior is intentionally replaced, use a reviewed baseline change/exception and point to the stronger replacement test.",
            )
        )
    metrics = {
        "named_test_base_total": sum(base.values()),
        "named_test_head_total": sum(head.values()),
        "named_test_decreased_keys": len(decreased),
        "named_test_removed_instances": removed_instances,
    }
    return findings, metrics


def _fuzz_target_names(files: Sequence[str]) -> list[str]:
    names: list[str] = []
    for path in files:
        parts = path.replace("\\", "/").split("/")
        if "fuzz_targets" not in parts or not path.endswith(".rs"):
            continue
        stem = parts[-1][:-3]
        scope = parts[1] if len(parts) > 2 and parts[0] == "crates" else "/".join(parts[:-2])
        names.append(f"{scope}:{stem}" if scope else stem)
    return sorted(names)


def _regex_int(text: str | None, pattern: str) -> int | None:
    if not text:
        return None
    m = re.search(pattern, text)
    return int(m.group(1)) if m else None


def _regex_float(text: str | None, pattern: str) -> float | None:
    if not text:
        return None
    m = re.search(pattern, text)
    return float(m.group(1)) if m else None


def inventory_at(repo: Path, rev: str | None) -> dict[str, Any]:
    files = list_files_at_rev(repo, rev)
    golden_text = read_at_rev(repo, rev, "golden/vm/expected-count.txt")
    mcp_text = read_at_rev(repo, rev, "crates/rd-agent/src/mcp.rs")
    root_cargo = parse_toml(read_at_rev(repo, rev, "Cargo.toml")) or {}
    rust_version = None
    package = root_cargo.get("workspace", {}).get("package", {}) if isinstance(root_cargo.get("workspace"), Mapping) else {}
    if isinstance(package, Mapping):
        rust_version = package.get("rust-version")
    if not rust_version and isinstance(root_cargo.get("package"), Mapping):
        rust_version = root_cargo["package"].get("rust-version")
    mcp_count = _regex_int(mcp_text, r"assert_eq!\(\s*defs\.len\(\)\s*,\s*(\d+)\s*\)")
    if mcp_count is None and mcp_text:
        # Fallback for older trees: count tool definition names. The baseline
        # contract still catches shrinkage once the canonical assertion exists.
        names = re.findall(r"\bname:\s*\"([a-z0-9_]+)\"", mcp_text)
        mcp_count = len(set(names)) if names else None
    return {
        "static_test_attributes": _count_test_attributes(repo, rev),
        "golden_vector_count": _regex_int(golden_text, r"(\d+)"),
        "fuzz_target_count": len([f for f in files if "/fuzz/fuzz_targets/" in f and f.endswith(".rs")]),
        "fuzz_target_names": _fuzz_target_names(files),
        "mcp_tool_count": mcp_count,
        "rust_version": str(rust_version) if rust_version else None,
        "cargo_lock_version": lock_version(read_at_rev(repo, rev, "Cargo.lock")),
        "floor_min_sdk": _regex_int(read_at_rev(repo, rev, "crates/rd-apk/src/lib.rs"), r"FLOOR_MIN_SDK\s*:\s*u32\s*=\s*(\d+)"),
        "startup_budget_emulated_ms": _regex_float(
            read_at_rev(repo, rev, "crates/rd-cli/src/perf.rs"),
            r"STARTUP_BUDGET_EMULATED_MS\s*:\s*f64\s*=\s*([0-9]+(?:\.[0-9]+)?)",
        ),
        "tracked_files": len(files),
    }


def _is_test_path(path: str, policy: Mapping[str, Any]) -> bool:
    return match_any(path, policy.get("diff", {}).get("test_path_globs", [])) or "tests" in classify_path(path)


def _rust_test_lines(repo: Path, path: str) -> set[int]:
    """Best-effort inline-test line map for the checked-out candidate."""

    try:
        lines = (repo / path).read_text(encoding="utf-8", errors="ignore").splitlines()
    except OSError:
        return set()
    test_lines: set[int] = set()
    pending_test_block = False
    in_test_block = False
    depth = 0
    for raw_index, line in enumerate(lines, 1):
        stripped = line.strip()
        if in_test_block:
            # Already inside a `#[cfg(test)]` item: a nested `#[test]` fn or
            # helper must NOT restart the depth count, otherwise the lines
            # between nested items (helpers, shared fixtures) were reported
            # as production code once the first nested fn closed.
            test_lines.add(raw_index)
            depth += line.count("{") - line.count("}")
            if depth <= 0:
                in_test_block = False
            continue
        if "#[cfg(test)]" in stripped or stripped == "#[test]":
            pending_test_block = True
            test_lines.add(raw_index)
            continue
        if pending_test_block and "{" not in line:
            test_lines.add(raw_index)
            continue
        if pending_test_block and "{" in line:
            pending_test_block = False
            in_test_block = True
            depth = line.count("{") - line.count("}")
            test_lines.add(raw_index)
            if depth <= 0:
                in_test_block = False
            continue
        if in_test_block:
            test_lines.add(raw_index)
            depth += line.count("{") - line.count("}")
            if depth <= 0:
                in_test_block = False
    return test_lines


_PUBLIC_DECL_RE = re.compile(
    r"^\s*pub(?:\s*\([^)]*\))?\s+"
    r"(?:default\s+)?(?:async\s+|const\s+|unsafe\s+|extern\s+(?:\"[^\"]*\"\s+)?)*?"
    r"(fn|struct|enum|trait|type|const|static|mod|union)\s+"
    r"([A-Za-z_][A-Za-z0-9_]*)\b"
)


def _normalized_public_signature(line: str) -> str:
    value = re.sub(r"\s+", " ", line.strip())
    for marker in ("{", ";", "="):
        if marker in value:
            value = value.split(marker, 1)[0].rstrip()
    return value


def public_api_inventory_from_sources(
    sources: Mapping[str, str],
) -> dict[tuple[str, str, str], set[str]]:
    """Extract a conservative public-item inventory from Rust sources.

    Keys are (crate, kind, name), deliberately ignoring the exact file so a
    same-crate move is not misreported as removal. Values are normalized
    declaration lines; a same-name item whose declaration no longer overlaps
    the base declaration is reported as a signature change. This is not a
    replacement for rustdoc-based semver tooling, but it is much less noisy
    than flagging every removed ``pub`` diff line.
    """

    inventory: dict[tuple[str, str, str], set[str]] = {}
    for path, text in sources.items():
        parts = path.replace("\\", "/").split("/")
        crate = parts[1] if len(parts) > 2 and parts[0] == "crates" else "workspace"
        for line in text.splitlines():
            match = _PUBLIC_DECL_RE.match(line)
            if not match:
                continue
            kind, name = match.group(1), match.group(2)
            inventory.setdefault((crate, kind, name), set()).add(
                _normalized_public_signature(line)
            )
    return inventory


def _public_api_inventory(repo: Path, rev: str | None) -> dict[tuple[str, str, str], set[str]]:
    sources: dict[str, str] = {}
    for path in list_files_at_rev(repo, rev):
        if path.startswith("crates/") and "/src/" in path and path.endswith(".rs"):
            text = read_at_rev(repo, rev, path)
            if text is not None:
                sources[path] = text
    return public_api_inventory_from_sources(sources)


def _public_api_findings(
    base: Mapping[tuple[str, str, str], set[str]],
    head: Mapping[tuple[str, str, str], set[str]],
) -> tuple[list[Finding], dict[str, Any]]:
    findings: list[Finding] = []
    removed = sorted(set(base) - set(head))
    changed: list[tuple[str, str, str]] = []
    for key in sorted(set(base) & set(head)):
        if base[key] and head[key] and not (base[key] & head[key]):
            changed.append(key)

    for crate, kind, name in removed[:50]:
        findings.append(
            Finding(
                id="public-api-removed",
                severity="P1",
                area="contracts",
                title=f"Public {kind} removed: {name}",
                evidence=f"crate={crate}; base declarations={sorted(base[(crate, kind, name)])[:3]}",
                path=f"crates/{crate}/src",
                remediation="Restore the item, keep a compatibility wrapper/re-export, or document the breaking change and migration in an ADR.",
            )
        )
    for crate, kind, name in changed[:50]:
        findings.append(
            Finding(
                id="public-api-signature-changed",
                severity="P1",
                area="contracts",
                title=f"Public {kind} signature changed: {name}",
                evidence=(
                    f"crate={crate}; base={sorted(base[(crate, kind, name)])[:2]}; "
                    f"head={sorted(head[(crate, kind, name)])[:2]}"
                ),
                path=f"crates/{crate}/src",
                remediation="Prove downstream callers still compile/behave the same, or document the breaking API change in an ADR.",
            )
        )
    metrics = {
        "public_api_base_items": len(base),
        "public_api_head_items": len(head),
        "public_api_removed_items": len(removed),
        "public_api_changed_items": len(changed),
    }
    return findings, metrics


def _candidate_baseline(repo: Path, head_rev: str | None) -> dict[str, Any] | None:
    text = read_at_rev(repo, head_rev, ".github/rd-sentinel/baseline.json")
    if not text:
        return None
    try:
        value = json.loads(text)
    except json.JSONDecodeError:
        return None
    return value if isinstance(value, dict) else None


def _check_baseline_inventory(
    inventory: Mapping[str, Any], baseline: Mapping[str, Any]
) -> list[Finding]:
    contracts = baseline.get("contracts", {}) if isinstance(baseline.get("contracts"), Mapping) else {}
    findings: list[Finding] = []

    def exact(key: str, contract_key: str, title: str, path: str) -> None:
        expected = contracts.get(contract_key)
        actual = inventory.get(key)
        if expected is not None and actual is not None and actual != expected:
            findings.append(
                Finding(
                    id=f"contract-{contract_key.replace('_', '-')}",
                    severity="P1",
                    area="contracts",
                    title=title,
                    evidence=f"baseline={expected!r}, actual={actual!r}",
                    path=path,
                    remediation="Restore the versioned contract or land a separately approved baseline/ADR change with before-and-after evidence.",
                )
            )

    exact("rust_version", "rust_version", "Rust MSRV contract changed", "Cargo.toml")
    exact("cargo_lock_version", "cargo_lock_version", "Cargo.lock format changed", "Cargo.lock")
    exact("floor_min_sdk", "floor_min_sdk", "Android minSdk floor changed", "crates/rd-apk/src/lib.rs")
    exact("mcp_tool_count", "mcp_tool_count", "MCP tool-count contract changed", "crates/rd-agent/src/mcp.rs")
    exact("golden_vector_count", "golden_vector_count", "JVM golden-vector count changed", "golden/vm/expected-count.txt")
    exact("fuzz_target_count", "fuzz_target_count", "Fuzz target count changed", "crates/rd-apk/fuzz/fuzz_targets")

    min_tests = contracts.get("minimum_static_test_attributes")
    actual_tests = inventory.get("static_test_attributes")
    if isinstance(min_tests, int) and isinstance(actual_tests, int) and actual_tests < min_tests:
        findings.append(
            Finding(
                id="test-inventory-below-baseline",
                severity="P1",
                area="tests",
                title="Static test inventory fell below baseline",
                evidence=f"baseline>={min_tests}, actual={actual_tests}",
                remediation="Restore equivalent or stronger tests; test deletion cannot make a regression look green.",
            )
        )

    max_budget = contracts.get("startup_budget_emulated_ms_max")
    actual_budget = inventory.get("startup_budget_emulated_ms")
    if isinstance(max_budget, (int, float)) and isinstance(actual_budget, (int, float)) and actual_budget > max_budget:
        findings.append(
            Finding(
                id="startup-budget-relaxed",
                severity="P0",
                area="performance",
                title="Startup performance budget was relaxed",
                evidence=f"baseline max={max_budget} ms, actual={actual_budget} ms",
                path="crates/rd-cli/src/perf.rs",
                remediation="Fix the performance regression instead of raising the budget; any budget change needs ADR 008/009 evidence.",
            )
        )
    return findings


def _check_inventory_regression(
    base: Mapping[str, Any], head: Mapping[str, Any], *, baseline_approved: bool = False
) -> list[Finding]:
    findings: list[Finding] = []
    shrinkable = {
        "static_test_attributes": ("test inventory", "tests"),
        "golden_vector_count": ("JVM golden-vector count", "tests"),
        "fuzz_target_count": ("fuzz target count", "fuzz"),
        "mcp_tool_count": ("MCP tool count", "agent-protocol"),
    }
    for key, (label, area) in shrinkable.items():
        before = base.get(key)
        after = head.get(key)
        if isinstance(before, int) and isinstance(after, int) and after < before:
            findings.append(
                Finding(
                    id=f"{key.replace('_', '-')}-shrank",
                    severity="P2" if baseline_approved else "P1",
                    area=area,
                    title=f"{label} shrank versus base",
                    evidence=f"base={before}, head={after}",
                    remediation="Restore the evidence surface or obtain a dated baseline exception with equivalent replacement coverage.",
                )
            )
    named_shrinkable = {
        "fuzz_target_names": ("fuzz target", "fuzz", "fuzz-target-removed"),
    }
    for key, (label, area, finding_id) in named_shrinkable.items():
        before_values = base.get(key)
        after_values = head.get(key)
        if not isinstance(before_values, list) or not isinstance(after_values, list):
            continue
        after_set = {str(item) for item in after_values}
        for missing in sorted({str(item) for item in before_values} - after_set)[:20]:
            findings.append(
                Finding(
                    id=finding_id,
                    severity="P2" if baseline_approved else "P1",
                    area=area,
                    title=f"Named {label} removed or renamed: {missing}",
                    evidence=f"base has {missing}; head does not",
                    remediation="Restore the named evidence entry, or use a reviewed baseline change/exception that names the replacement coverage. Matching counts alone do not prove the same surface is still exercised.",
                )
            )
    return findings


_WORKFLOW_USES_RE = re.compile(r"^\s*-?\s*uses:\s*([^#\s]+)")
_DANGEROUS_RUN_EXPRESSIONS = (
    "github.event.issue.title",
    "github.event.issue.body",
    "github.event.pull_request.title",
    "github.event.pull_request.body",
    "github.event.pull_request.head.ref",
    "github.event.comment.body",
    "github.event.review.body",
    "github.event.discussion.title",
    "github.event.discussion.body",
    "github.event.head_commit.message",
    "github.event.head_commit.author",
    "github.event.commits",
    "github.event.workflow_run.head_branch",
    "github.head_ref",
    "inputs.",
)


def workflow_action_refs(text: str) -> list[tuple[int, str]]:
    refs: list[tuple[int, str]] = []
    for idx, line in enumerate(text.splitlines(), 1):
        match = _WORKFLOW_USES_RE.match(line)
        if match:
            refs.append((idx, match.group(1).strip("\"'")))
    return refs


def _action_ref_kind(value: str) -> str:
    if value.startswith(("./", "../")):
        return "local"
    if value.startswith("docker://"):
        return "pinned" if "@sha256:" in value else "mutable-tag"
    if "@" not in value:
        return "unpinned"
    ref = value.rsplit("@", 1)[-1]
    if re.fullmatch(r"[0-9a-fA-F]{40}", ref):
        return "pinned"
    if re.fullmatch(r"v?\d+(?:\.\d+){0,3}(?:[-+][A-Za-z0-9.-]+)?", ref):
        return "mutable-tag"
    return "moving-branch"


def workflow_template_injections(text: str) -> list[tuple[int, str]]:
    """Find user-controlled GitHub expressions interpolated into run scripts."""

    findings: list[tuple[int, str]] = []
    lines = text.splitlines()
    run_block_indent: int | None = None

    def inspect(line_no: int, value: str) -> None:
        for expr in re.findall(r"\$\{\{\s*([^}]+?)\s*\}\}", value):
            compact = re.sub(r"\s+", "", expr)
            if any(marker.replace(" ", "") in compact for marker in _DANGEROUS_RUN_EXPRESSIONS):
                findings.append((line_no, expr.strip()))

    for idx, line in enumerate(lines, 1):
        indent = len(line) - len(line.lstrip(" "))
        if run_block_indent is not None:
            if line.strip() and indent <= run_block_indent:
                run_block_indent = None
            else:
                inspect(idx, line)
                continue
        match = re.match(r"^(\s*)-?\s*run:\s*(.*)$", line)
        if not match:
            continue
        inline = match.group(2).strip()
        if inline in {"|", ">", "|-", ">-", "|+", ">+"}:
            run_block_indent = len(match.group(1))
        elif inline:
            inspect(idx, inline)
    return findings


def _workflow_trigger_names(text: str) -> set[str]:
    """Extract names from the top-level ``on:`` block only.

    Looking for trigger substrings anywhere makes comments and trusted
    publisher references look like dangerous triggers. This small indentation
    parser deliberately handles the common scalar, flow-list, block-list, and
    mapping forms used by GitHub workflows.
    """

    triggers: set[str] = set()
    in_on_block = False
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        indent = len(line) - len(line.lstrip(" "))
        if indent == 0:
            match = re.match(r"^['\"]?on['\"]?\s*:\s*(.*)$", stripped)
            in_on_block = match is not None
            if not match:
                continue
            inline = match.group(1).strip().strip("[]")
            if inline:
                for item in inline.split(","):
                    name = item.strip().strip("\"'")
                    if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", name):
                        triggers.add(name)
            continue
        if not in_on_block or indent > 4:
            continue
        key_match = re.match(r"^([A-Za-z_][A-Za-z0-9_]*)\s*:", stripped)
        if key_match:
            triggers.add(key_match.group(1))
            continue
        list_match = re.match(r"^-\s*['\"]?([A-Za-z_][A-Za-z0-9_]*)['\"]?\s*$", stripped)
        if list_match:
            triggers.add(list_match.group(1))
    return triggers


def _workflow_findings(
    repo: Path, files: Sequence[ChangedFile], head_rev: str | None
) -> tuple[list[Finding], dict[str, Any]]:
    findings: list[Finding] = []
    metrics: dict[str, Any] = {
        "workflow_files_changed": 0,
        "workflow_action_refs": 0,
        "workflow_unpinned_action_refs": 0,
        "workflow_template_injections": 0,
    }
    trusted_write_workflows = {
        ".github/workflows/rd-sentinel-command.yml",
        ".github/workflows/rd-sentinel-publish.yml",
    }
    sentinel_workflows = trusted_write_workflows | {
        ".github/workflows/rd-sentinel.yml",
        ".github/workflows/rd-sentinel-deep.yml",
    }

    for item in files:
        path = item.path
        if item.status.startswith("D") or not path.startswith(".github/workflows/"):
            continue
        if not path.endswith((".yml", ".yaml")):
            continue
        text = read_at_rev(repo, head_rev, path)
        if text is None:
            continue
        metrics["workflow_files_changed"] += 1
        lines = text.splitlines()
        has_permissions = any(re.match(r"^\s*permissions\s*:", line) for line in lines)
        has_write = bool(
            re.search(r"(?m)^\s*[a-z][a-z-]*\s*:\s*write\s*$", text)
            or re.search(r"(?m)^\s*permissions\s*:\s*write-all\s*$", text)
        )
        dangerous_trigger = next(
            (
                trigger
                for trigger in ("pull_request_target", "workflow_run", "issue_comment")
                if trigger in _workflow_trigger_names(text)
            ),
            None,
        )
        if not has_permissions:
            findings.append(
                Finding(
                    id="workflow-permissions-missing",
                    severity="P1" if dangerous_trigger or has_write else "P2",
                    area="ci-security",
                    title="Workflow does not declare explicit token permissions",
                    evidence=path,
                    path=path,
                    remediation="Declare least-privilege permissions at the top level or per job; do not inherit repository defaults.",
                )
            )

        refs = workflow_action_refs(text)
        metrics["workflow_action_refs"] += len(refs)
        unpinned: list[str] = []
        moving: list[str] = []
        for _line_no, ref in refs:
            kind = _action_ref_kind(ref)
            if kind == "moving-branch":
                moving.append(ref)
                unpinned.append(ref)
            elif kind in {"mutable-tag", "unpinned"}:
                unpinned.append(ref)
        if unpinned:
            metrics["workflow_unpinned_action_refs"] += len(unpinned)
            third_party_unpinned = [
                ref
                for ref in unpinned
                if not ref.startswith(("actions/", "github/"))
            ]
            severity = (
                "P1"
                if moving or third_party_unpinned or path in sentinel_workflows or has_write or dangerous_trigger
                else "P2"
            )
            findings.append(
                Finding(
                    id="workflow-action-not-sha-pinned",
                    severity=severity,
                    area="ci-security",
                    title="GitHub Action is not pinned to a full commit SHA",
                    evidence=", ".join(sorted(set(unpinned))[:12]),
                    path=path,
                    remediation="Pin each action to the reviewed release commit SHA with a version comment, or adopt a verified workflow lockfile; tags and branches can move.",
                )
            )

        injections = workflow_template_injections(text)
        metrics["workflow_template_injections"] += len(injections)
        for line_no, expr in injections[:20]:
            findings.append(
                Finding(
                    id="workflow-template-injection",
                    severity="P1",
                    area="ci-security",
                    title="Untrusted GitHub expression interpolated into a shell script",
                    evidence=f"${{{{ {expr} }}}}",
                    path=path,
                    line=line_no,
                    remediation="Bind the expression to an environment variable first and reference the shell variable; never template user-controlled text into run:.",
                )
            )

        checkout_refs = [ref for _n, ref in refs if ref.startswith("actions/checkout@")]
        persist_false = len(re.findall(r"persist-credentials\s*:\s*false", text))
        if checkout_refs and persist_false < len(checkout_refs):
            findings.append(
                Finding(
                    id="workflow-checkout-persists-credentials",
                    severity="P1" if dangerous_trigger or has_write else "P2",
                    area="ci-security",
                    title="Checkout may persist the GitHub token in the worktree",
                    evidence=f"checkouts={len(checkout_refs)}, persist-credentials:false={persist_false}",
                    path=path,
                    remediation="Set persist-credentials: false on checkouts that do not push; persisted tokens can be stolen by later build code.",
                )
            )

        candidate_checkout = bool(
            re.search(
                r"ref\s*:\s*[^\n]*(?:pull_request\.head|workflow_run\.head_sha|github\.head_ref)",
                text,
            )
        )
        if dangerous_trigger and candidate_checkout:
            findings.append(
                Finding(
                    id="workflow-dangerous-trigger-candidate-checkout",
                    severity="P0",
                    area="ci-security",
                    title=f"Dangerous trigger {dangerous_trigger} checks out candidate code",
                    evidence=path,
                    path=path,
                    remediation="Never execute or build untrusted head code in a base-repository privileged trigger; split read-only analysis from the trusted publisher.",
                )
            )

        if "actions/cache@" in text and (has_write or dangerous_trigger):
            findings.append(
                Finding(
                    id="workflow-cache-in-privileged-job",
                    severity="P2",
                    area="ci-security",
                    title="Cache used in a privileged workflow",
                    evidence=path,
                    path=path,
                    remediation="Review cache scope/keys for poisoning risk; avoid restoring untrusted caches in publish/write jobs.",
                )
            )
    return findings, metrics


def _diff_rule_findings(
    repo: Path,
    files: Sequence[ChangedFile],
    lines: Sequence[DiffLine],
    policy: Mapping[str, Any],
) -> tuple[list[Finding], set[str]]:
    findings: list[Finding] = []
    triggers: set[str] = set()
    diff_cfg = policy.get("diff", {}) if isinstance(policy.get("diff"), Mapping) else {}
    unsafe_allowed = diff_cfg.get("unsafe_allowed_path_globs", ["crates/rd-jni/**", "crates/rd-ndk/**"])
    changed_paths = [f.path for f in files]
    has_adr = any(match_any(p, diff_cfg.get("adr_path_globs", ["docs/adr/**"])) for p in changed_paths)
    has_tests = any(_is_test_path(p, policy) for p in changed_paths) or any(
        l.kind == "added" and "#[test]" in l.text for l in lines
    )
    has_compat = any(match_any(p, diff_cfg.get("compat_evidence_globs", [])) for p in changed_paths)

    rust_test_line_cache: dict[str, set[int]] = {}

    for line in lines:
        text = line.text
        path = line.path
        secret_kind = detect_secret_kind(text) if line.kind == "added" else None
        if secret_kind:
            findings.append(
                Finding(
                    id="possible-secret-added",
                    severity="P0",
                    area="ci-security",
                    title="Possible credential added",
                    evidence=f"A {secret_kind} shaped string was added (value intentionally not reproduced).",
                    path=path,
                    line=line.line,
                    remediation="Remove and rotate the credential; load secrets from the CI secret store.",
                )
            )
            triggers.add("secret")

        if line.kind == "removed" and re.search(r"^\s*pub\s+(?:async\s+)?(?:fn|struct|enum|trait|type|const|static)\b", text):
            # The set-based public-API inventory in build_static_report emits
            # the actual finding, so a same-crate move is not a false P1.
            triggers.add("public-api")
            continue
        if line.kind != "added":
            continue

        is_rust = path.endswith(".rs")
        in_test_path = _is_test_path(path, policy)
        if is_rust and not in_test_path and line.line is not None:
            if path not in rust_test_line_cache:
                rust_test_line_cache[path] = _rust_test_lines(repo, path)
            in_test_path = line.line in rust_test_line_cache[path]
        areas = classify_path(path)

        if is_rust and re.search(r"\bunsafe\s*(?:\{|fn\b|impl\b|trait\b|extern\b)", text):
            triggers.add("unsafe")
            if not match_any(path, unsafe_allowed):
                findings.append(
                    Finding(
                        id="unsafe-outside-boundary",
                        severity="P0",
                        area="native-security",
                        title="New unsafe outside rd-jni/rd-ndk",
                        evidence=text.strip()[:300],
                        path=path,
                        line=line.line,
                        remediation="Move the boundary into an approved crate or remove the unsafe; document soundness in an ADR.",
                    )
                )
            else:
                findings.append(
                    Finding(
                        id="new-unsafe-boundary",
                        severity="P1",
                        area="native-security",
                        title="New unsafe boundary needs soundness evidence",
                        evidence=text.strip()[:300],
                        path=path,
                        line=line.line,
                        remediation="Add a narrow safety argument, hostile-input tests, and an ADR note.",
                    )
                )

        if is_rust and path.startswith("crates/rd-vm/") and "thread_local!" in text:
            findings.append(
                Finding(
                    id="new-thread-local-state",
                    severity="P1",
                    area="vm-core",
                    title="New thread-local state may break Engine ownership/handoff",
                    evidence=text.strip()[:300],
                    path=path,
                    line=line.line,
                    remediation="Store guest-visible state in Engine; only immutable/pure caches may use a dated policy exception.",
                )
            )
            triggers.add("global-state")

        if is_rust and re.search(r"\bstatic\s+mut\b|\blazy_static!\b", text):
            findings.append(
                Finding(
                    id="new-mutable-global-state",
                    severity="P1",
                    area="vm-core",
                    title="New mutable global state",
                    evidence=text.strip()[:300],
                    path=path,
                    line=line.line,
                    remediation="Move state into the owning Engine/session or prove it is process configuration with tests.",
                )
            )
            triggers.add("global-state")

        collection_cell = re.search(r"OnceLock\s*<\s*(?:[^>]*\b(?:HashMap|BTreeMap|Vec|HashSet|Mutex|RwLock)\b)", text)
        # A process-global is a `static` item. Besides `static X: OnceLock<..>`
        # this also catches `static X: Mutex<HashMap<..>>`/`LazyLock<..>`/
        # `RwLock<..>` globals, which the OnceLock-only pattern missed.
        global_static = re.search(
            r"\bstatic\s+(?:ref\s+)?[A-Za-z_][A-Za-z0-9_]*\s*:\s*[^=;]*\b(?:OnceLock|LazyLock|Lazy|Mutex|RwLock)\s*<[^=;]*\b(?:HashMap|BTreeMap|Vec|HashSet|BTreeSet|VecDeque|Mutex|RwLock)\b",
            text,
        )
        if is_rust and not in_test_path and collection_cell and not global_static:
            # Same pattern on a struct field / local: instance-owned, not
            # global. Still reported (non-blocking) so reviewers see it.
            findings.append(
                Finding(
                    id="new-instance-collection-cell",
                    severity="P2",
                    area="vm-core",
                    title="New lazily-initialized collection cell (instance-owned)",
                    evidence=text.strip()[:300],
                    path=path,
                    line=line.line,
                    remediation="Confirm the owner is per-Engine/per-document state and that clones never share initialized cells.",
                )
            )
        if is_rust and global_static:
            findings.append(
                Finding(
                    id="new-global-collection",
                    severity="P1",
                    area="vm-core",
                    title="New global mutable/collection cache",
                    evidence=text.strip()[:300],
                    path=path,
                    line=line.line,
                    remediation="Prove the value is immutable and guest-invisible, or move it into Engine/session ownership.",
                )
            )
            triggers.add("global-state")

        if is_rust and not in_test_path and re.search(r"\b(?:todo!|unimplemented!|dbg!|panic!)\s*\(", text):
            findings.append(
                Finding(
                    id="new-panic-or-placeholder",
                    severity="P1",
                    area="runtime",
                    title="New panic/placeholder macro",
                    evidence=text.strip()[:300],
                    path=path,
                    line=line.line,
                    remediation="Return a structured RdError/NOT_IMPLEMENTED instead of aborting host execution.",
                )
            )

        if is_rust and not in_test_path and re.search(r"\.unwrap\s*\(\)|\.expect\s*\(", text):
            severity = "P1" if areas & {"parsers", "native-security"} else "P2"
            findings.append(
                Finding(
                    id="new-unwrap-expect",
                    severity=severity,
                    area="parsers" if "parsers" in areas else "runtime",
                    title="New unwrap/expect on a production path",
                    evidence=text.strip()[:300],
                    path=path,
                    line=line.line,
                    remediation="Handle hostile input/state with a structured error, or move the assertion behind a proven internal invariant.",
                )
            )

        if is_rust and "#[ignore" in text:
            findings.append(
                Finding(
                    id="test-ignored",
                    severity="P1",
                    area="tests",
                    title="A test was newly ignored",
                    evidence=text.strip()[:300],
                    path=path,
                    line=line.line,
                    remediation="Fix the test/product or add a dated tracking issue and remove the ignore before merge.",
                )
            )

        if is_rust and ("NOT_IMPLEMENTED" in text or "not_implemented" in text.lower()):
            triggers.add("not-implemented")
            if not has_compat:
                findings.append(
                    Finding(
                        id="not-implemented-without-compat-evidence",
                        severity="P2",
                        area="agent-protocol",
                        title="New NOT_IMPLEMENTED surface lacks compatibility evidence in the diff",
                        evidence=text.strip()[:300],
                        path=path,
                        line=line.line,
                        remediation="Update the compat matrix/API coverage or add a contract/golden test proving the structured response.",
                    )
                )

        if is_rust and re.search(r"Box::leak\b|mem::forget\b|process::exit\b|thread::sleep\b", text) and not in_test_path:
            findings.append(
                Finding(
                    id="resource-lifetime-red-flag",
                    severity="P1",
                    area="runtime",
                    title="New leak/exit/sleep red flag",
                    evidence=text.strip()[:300],
                    path=path,
                    line=line.line,
                    remediation="Use owned lifetimes, structured shutdown, and deterministic virtual time; leaks can mask UAF/regressions.",
                )
            )

        if path.startswith("crates/rd-agent/src/") and ("ToolDef" in text or "tool_defs" in text):
            triggers.add("mcp-contract")

        if "STARTUP_BUDGET_EMULATED_MS" in text or "FLOOR_MIN_SDK" in text:
            triggers.add("contract-constant")

        if path.startswith(".github/workflows/"):
            if "pull_request_target" in text:
                findings.append(
                    Finding(
                        id="workflow-pull-request-target",
                        severity="P0",
                        area="ci-security",
                        title="Workflow uses pull_request_target",
                        evidence=text.strip()[:300],
                        path=path,
                        line=line.line,
                        remediation="Use pull_request for analysis and a separate trusted workflow_run publisher.",
                    )
                )
            if re.search(r"permissions\s*:\s*write-all|contents\s*:\s*write|actions\s*:\s*write", text):
                trusted_workflows = {
                    ".github/workflows/rd-sentinel-command.yml",
                    ".github/workflows/rd-sentinel-publish.yml",
                }
                findings.append(
                    Finding(
                        id="workflow-write-permission",
                        severity="P2" if path in trusted_workflows else "P1",
                        area="ci-security",
                        title=(
                            "Trusted workflow uses a write permission"
                            if path in trusted_workflows
                            else "Workflow gained a broad write permission"
                        ),
                        evidence=text.strip()[:300],
                        path=path,
                        line=line.line,
                        remediation="Keep candidate-executing workflows read-only and isolate writes in trusted default-branch workflows; trusted workflow changes still require protected-path approval.",
                    )
                )
            if re.search(r"curl[^\n|]*\|\s*(?:sh|bash)|wget[^\n|]*\|\s*(?:sh|bash)", text):
                findings.append(
                    Finding(
                        id="workflow-curl-pipe-shell",
                        severity="P0",
                        area="ci-security",
                        title="Workflow pipes a download directly into a shell",
                        evidence=text.strip()[:300],
                        path=path,
                        line=line.line,
                        remediation="Download, verify a pinned checksum/signature, and execute the verified artifact.",
                    )
                )

    areas = set(areas_for_files(files))
    code_files = [f for f in files if f.path.endswith((".rs", ".py", ".sh", ".toml"))]
    if code_files and areas & HIGH_RISK_AREAS and not has_tests:
        findings.append(
            Finding(
                id="high-risk-change-without-test-evidence",
                severity="P1",
                area="tests",
                title="High-risk change has no test or golden evidence in the diff",
                evidence=f"areas={','.join(sorted(areas & HIGH_RISK_AREAS))}; changed code files={len(code_files)}",
                remediation="Add a regression/unit/integration/golden test that fails on the base behavior or locks the new contract.",
            )
        )

    adr_triggers = {"unsafe", "global-state", "public-api", "mcp-contract", "contract-constant"}
    if triggers & adr_triggers and not has_adr:
        findings.append(
            Finding(
                id="architecture-change-without-adr",
                severity="P1",
                area="contracts",
                title="Architecture/contract trigger has no ADR in the diff",
                evidence=f"triggers={','.join(sorted(triggers & adr_triggers))}",
                remediation="Add or amend an ADR before code review, as required by CONTRIBUTING.md.",
            )
        )

    large_limit = int(diff_cfg.get("large_file_bytes", 5 * 1024 * 1024))
    blocking_exts = {str(x).lower() for x in diff_cfg.get("binary_extensions_blocking", [])}
    for item in files:
        target = repo / item.path
        ext = Path(item.path).suffix.lower()
        if item.status.startswith("A") and ext in blocking_exts:
            findings.append(
                Finding(
                    id="binary-artifact-committed",
                    severity="P1",
                    area="dependencies-build",
                    title="Binary artifact added to the repository",
                    evidence=item.path,
                    path=item.path,
                    remediation="Generate/download fixtures through a pinned script and keep binaries out of git.",
                )
            )
        try:
            size = target.stat().st_size if target.is_file() else None
        except OSError:
            size = None
        if size is not None and size > large_limit and not item.status.startswith("D"):
            findings.append(
                Finding(
                    id="large-file-added",
                    severity="P1",
                    area="dependencies-build",
                    title="Changed file exceeds Sentinel size budget",
                    evidence=f"{item.path}: {size} bytes > {large_limit}",
                    path=item.path,
                    remediation="Split/generated-download the asset or approve a specific exception.",
                )
            )
    return findings, triggers


def _is_dependabot(context: AuditContext, files: Sequence[ChangedFile]) -> bool:
    actor = (context.actor or context.author).lower()
    if actor in {"dependabot[bot]", "dependabot-preview[bot]"}:
        return True
    if context.head_ref.startswith("dependabot/"):
        return True
    return False


def diff_cfg_globs(policy: Mapping[str, Any], key: str) -> list[str]:
    diff_cfg = policy.get("diff", {}) if isinstance(policy.get("diff"), Mapping) else {}
    value = diff_cfg.get(key, [])
    return [str(item) for item in value] if isinstance(value, list) else []


def build_static_report(
    *,
    repo: Path,
    base_ref: str,
    head_ref: str,
    policy: Mapping[str, Any],
    baseline: Mapping[str, Any],
    context: AuditContext,
) -> dict[str, Any]:
    repo = repo.resolve()
    base_sha = resolve_rev(repo, base_ref)
    findings: list[Finding] = validate_policy(policy)
    if base_sha is None:
        base_sha = resolve_rev(repo, "HEAD")
        findings.append(
            Finding(
                id="base-ref-unresolved",
                severity="P1",
                area="policy",
                title="Requested base ref could not be resolved",
                evidence=f"base_ref={base_ref}",
                remediation="Fetch full history and pass an explicit trusted base SHA.",
            )
        )
    if base_sha is None:
        raise RuntimeError("not a git repository or HEAD is missing")

    head_is_worktree = head_ref == "WORKTREE"
    head_sha = resolve_rev(repo, "HEAD" if head_is_worktree else head_ref)
    if head_sha is None:
        raise RuntimeError(f"could not resolve head ref {head_ref}")
    head_for_git = "WORKTREE" if head_is_worktree else head_sha

    files = collect_changed_files(repo, base_sha, head_for_git)
    diff_lines = collect_diff_lines(repo, base_sha, head_for_git, files)
    areas = areas_for_files(files)
    added_lines = sum(f.added or 0 for f in files)
    deleted_lines = sum(f.deleted or 0 for f in files)
    direct_packages, affected = affected_packages(
        repo, "WORKTREE" if head_is_worktree else head_sha, files
    )
    downstream_packages = [p for p in affected if p not in direct_packages]
    risk, risk_factors = risk_assessment(
        files,
        areas,
        added_lines,
        deleted_lines,
        downstream_packages=len(downstream_packages),
    )
    dependency_bot = _is_dependabot(context, files)
    plan = build_plan(
        profile=context.profile,
        areas=areas,
        files=files,
        labels=context.labels,
        risk=risk,
        dependency_bot=dependency_bot,
    )

    base_inventory = inventory_at(repo, base_sha)
    head_inventory = inventory_at(repo, "WORKTREE" if head_is_worktree else head_sha)
    approvals = policy.get("approvals", {}) if isinstance(policy.get("approvals"), Mapping) else {}
    baseline_label = str(approvals.get("baseline_label", "sentinel:baseline-approved"))
    effective_baseline: Mapping[str, Any] = baseline
    baseline_approval_applied = False
    if baseline_label in context.labels:
        candidate_baseline = _candidate_baseline(repo, "WORKTREE" if head_is_worktree else head_sha)
        if candidate_baseline is not None:
            effective_baseline = candidate_baseline
            baseline_approval_applied = True
    findings.extend(validate_baseline(effective_baseline))
    findings.extend(_check_baseline_inventory(head_inventory, effective_baseline))
    findings.extend(
        _check_inventory_regression(
            base_inventory, head_inventory, baseline_approved=baseline_approval_applied
        )
    )

    diff_findings, triggers = _diff_rule_findings(repo, files, diff_lines, policy)
    findings.extend(diff_findings)

    workflow_findings, workflow_metrics = _workflow_findings(
        repo, files, "WORKTREE" if head_is_worktree else head_sha
    )
    findings.extend(workflow_findings)

    public_api_metrics: dict[str, Any] = {}
    named_test_metrics: dict[str, Any] = {}
    if any(f.path.startswith("crates/") and f.path.endswith(".rs") for f in files):
        public_findings, public_api_metrics = _public_api_findings(
            _public_api_inventory(repo, base_sha),
            _public_api_inventory(repo, "WORKTREE" if head_is_worktree else head_sha),
        )
        findings.extend(public_findings)
        if public_findings:
            triggers.add("public-api")
    if any(f.path.endswith(".rs") for f in files):
        named_test_findings, named_test_metrics = _named_test_findings(
            _named_test_inventory(repo, base_sha),
            _named_test_inventory(repo, "WORKTREE" if head_is_worktree else head_sha),
            baseline_approved=baseline_approval_applied,
        )
        findings.extend(named_test_findings)
        if named_test_findings:
            triggers.add("named-test-removal")

    protected = policy.get("protected_paths", [])
    policy_label = str(approvals.get("policy_label", "sentinel:policy-approved"))
    touched_protected = [f for f in files if match_any(f.path, protected)]
    if touched_protected and policy_label not in context.labels:
        findings.append(
            Finding(
                id="protected-sentinel-path-changed",
                severity="P0",
                area="sentinel",
                title="Sentinel policy/code changed without maintainer policy approval",
                evidence=", ".join(f.path for f in touched_protected[:20]),
                remediation=f"A maintainer must review the bot change separately and apply `{policy_label}`.",
            )
        )
    for item in touched_protected:
        if item.status.startswith("D"):
            findings.append(
                Finding(
                    id="protected-sentinel-path-deleted",
                    severity="P0",
                    area="sentinel",
                    title="Protected Sentinel file deleted",
                    evidence=item.path,
                    path=item.path,
                    remediation="Restore the gate; disabling the anti-regression system is itself a regression.",
                )
            )

    evidence_harnesses = [
        f for f in files if match_any(f.path, diff_cfg_globs(policy, "evidence_harness_globs"))
    ]
    if evidence_harnesses and policy_label not in context.labels:
        findings.append(
            Finding(
                id="evidence-harness-changed",
                severity="P1",
                area="tests",
                title="Regression evidence harness changed without policy approval",
                evidence=", ".join(f.path for f in evidence_harnesses[:20]),
                remediation=(
                    "Have a maintainer review the measurement/golden/fuzz harness change "
                    f"separately and apply `{policy_label}`; a change must not quietly "
                    "teach its own regression detector to pass."
                ),
            )
        )

    if any(f.path == ".github/rd-sentinel/baseline.json" for f in files):
        if baseline_label not in context.labels:
            findings.append(
                Finding(
                    id="baseline-changed-without-approval",
                    severity="P0",
                    area="sentinel",
                    title="Sentinel baseline changed without baseline approval",
                    evidence=".github/rd-sentinel/baseline.json",
                    remediation=f"Attach before/after campaign artifacts and apply `{baseline_label}` after maintainer review.",
                )
            )

    dependency_changes: list[dict[str, Any]] = []
    dependency_metrics: dict[str, Any] = {}
    deep_dependency_changes: list[str] = []
    if any(f.path.endswith(("Cargo.toml", "Cargo.lock")) for f in files) or "dependencies-build" in areas:
        head_rev_for_read = "WORKTREE" if head_is_worktree else head_sha
        lockfiles = sorted(
            {
                path
                for rev in (base_sha, head_rev_for_read)
                for path in list_files_at_rev(repo, rev)
                if path.endswith("Cargo.lock")
            }
        ) or ["Cargo.lock"]
        aggregate_metrics: dict[str, int] = {}
        for lockfile in lockfiles:
            lock_changes, lock_metrics = compare_cargo_locks(
                read_at_rev(repo, base_sha, lockfile),
                read_at_rev(repo, head_rev_for_read, lockfile),
            )
            for change in lock_changes:
                dependency_changes.append({**change, "lockfile": lockfile})
            for key, value in lock_metrics.items():
                if isinstance(value, int):
                    aggregate_metrics[key] = aggregate_metrics.get(key, 0) + value
        dependency_metrics = dict(aggregate_metrics)
        dependency_metrics["dependency_lockfiles_checked"] = len(lockfiles)
        _, manifest_findings = compare_manifests(repo, base_sha, head_rev_for_read)
        findings.extend(manifest_findings)

        dependencies_cfg = (
            policy.get("dependencies", {}) if isinstance(policy.get("dependencies"), Mapping) else {}
        )
        registry_prefix = str(dependencies_cfg.get("allowed_registry_source_prefix", ""))
        major_label = str(approvals.get("dependency_major_label", "sentinel:dependency-major-approved"))
        deep_patterns: list[re.Pattern[str]] = []
        for pattern in dependencies_cfg.get("deep_package_name_patterns", []):
            if not isinstance(pattern, str):
                continue
            try:
                deep_patterns.append(re.compile(pattern))
            except re.error:
                # validate_policy already emitted the P0 for this trusted regex.
                continue

        def add_deep_plan(component: str, reason: str) -> None:
            if context.profile in {"plan", "static"}:
                return
            if not any(item.get("component") == component for item in plan):
                plan.append({"component": component, "reason": reason, "required": True})

        duplicate_delta = dependency_metrics.get("dependency_duplicate_names_delta")
        if isinstance(duplicate_delta, int) and duplicate_delta > 0:
            findings.append(
                Finding(
                    id="dependency-duplicate-versions-increased",
                    severity="P2",
                    area="dependencies-build",
                    title="Duplicate dependency versions increased",
                    evidence=(
                        f"base duplicate names={dependency_metrics.get('dependency_duplicate_names_base')}, "
                        f"head duplicate names={dependency_metrics.get('dependency_duplicate_names_head')}"
                    ),
                    path="Cargo.lock",
                    remediation="Prefer updating the direct consumers to one compatible version; duplicate versions increase binary size, audit surface, and type-identity surprises.",
                )
            )
            add_deep_plan("compare", "Duplicate dependency versions increased")

        mass_threshold = _safe_policy_int(
            dependencies_cfg.get("mass_update_change_threshold"), 20
        )
        if len(dependency_changes) >= mass_threshold:
            findings.append(
                Finding(
                    id="dependency-mass-update",
                    severity="P2",
                    area="dependencies-build",
                    title="Mass dependency update needs differential evidence",
                    evidence=f"{len(dependency_changes)} resolved package changes >= threshold {mass_threshold}",
                    path="Cargo.lock",
                    remediation="Run feature, determinism, and A/B campaigns; split unrelated major updates so a regression can be bisected to one crate.",
                )
            )
            add_deep_plan("features", "Mass dependency update")
            add_deep_plan("determinism", "Mass dependency update")
            add_deep_plan("compare", "Mass dependency update")

        for change in dependency_changes:
            name = str(change.get("name"))
            lockfile = str(change.get("lockfile") or "Cargo.lock")

            if any(pattern.match(name) for pattern in deep_patterns):
                deep_dependency_changes.append(name)
                findings.append(
                    Finding(
                        id="deep-dependency-changed",
                        severity="P2",
                        area="dependencies-build",
                        title=f"Deep dependency needs expanded Sentinel trials: {name}",
                        evidence=f"{change.get('old')} -> {change.get('new')} in {lockfile}",
                        path=lockfile,
                        remediation="Run feature, determinism, and A/B evidence because this crate sits on a parser/compression/render/JIT/build critical path.",
                    )
                )
                add_deep_plan("features", f"Deep dependency changed: {name}")
                add_deep_plan("determinism", f"Deep dependency changed: {name}")
                add_deep_plan("compare", f"Deep dependency changed: {name}")

            if change.get("kind") == "major" and major_label not in context.labels:
                findings.append(
                    Finding(
                        id="dependency-major-change",
                        severity="P1",
                        area="dependencies-build",
                        title=f"Major dependency movement: {name}",
                        evidence=f"{change.get('old')} -> {change.get('new')}",
                        path=lockfile,
                        remediation="Run the deep campaign, read the upstream changelog/MSRV notes, and apply the dependency-major approval label.",
                    )
                )
            if change.get("kind") == "major":
                add_deep_plan("features", f"Major dependency changed: {name}")
                add_deep_plan("compare", f"Major dependency changed: {name}")
            if change.get("kind") == "unknown":
                findings.append(
                    Finding(
                        id="dependency-version-unclassified",
                        severity="P1",
                        area="dependencies-build",
                        title=f"Dependency version movement could not be classified: {name}",
                        evidence=f"{change.get('old')} -> {change.get('new')}",
                        path=lockfile,
                        remediation="Review the non-semver version movement manually before merge.",
                    )
                )

            old_parsed = parse_semver(str(change.get("old") or ""))
            new_parsed = parse_semver(str(change.get("new") or ""))
            if old_parsed is not None and new_parsed is not None:
                downgraded = new_parsed[0] < old_parsed[0] or (
                    new_parsed[0] == old_parsed[0]
                    and not old_parsed[1]
                    and bool(new_parsed[1])
                )
                if downgraded:
                    findings.append(
                        Finding(
                            id="dependency-downgrade",
                            severity="P1",
                            area="dependencies-build",
                            title=f"Dependency downgrade: {name}",
                            evidence=f"{change.get('old')} -> {change.get('new')}",
                            path=lockfile,
                            remediation="Document why the older release is safer than the base version, including advisory and MSRV evidence.",
                        )
                    )

            source = change.get("new_source")
            if source and registry_prefix and not str(source).startswith(registry_prefix):
                findings.append(
                    Finding(
                        id="dependency-source-changed",
                        severity="P0",
                        area="dependencies-build",
                        title=f"Dependency source is not crates.io: {name}",
                        evidence=str(source),
                        path=lockfile,
                        remediation="Use the canonical crates.io source or approve an immutable, reviewed alternate source in an ADR.",
                    )
                )
        dependency_metrics["deep_dependency_changes"] = sorted(set(deep_dependency_changes))

    if dependency_bot:
        allowed_dep_paths = ("Cargo.toml", "Cargo.lock", ".github/workflows/")
        unexpected = [
            f.path
            for f in files
            if not (f.path.endswith(allowed_dep_paths[:2]) or f.path.startswith(allowed_dep_paths[2]))
        ]
        if unexpected:
            findings.append(
                Finding(
                    id="dependabot-pr-touches-source",
                    severity="P0",
                    area="dependencies-build",
                    title="Dependency-bot PR touches non-dependency files",
                    evidence=", ".join(unexpected[:20]),
                    remediation="Split source changes into a human PR; a Dependabot PR may only move manifests, lockfiles, or Actions versions.",
                )
            )

    diff_cfg = policy.get("diff", {}) if isinstance(policy.get("diff"), Mapping) else {}
    if len(files) > _safe_policy_int(diff_cfg.get("very_large_diff_files"), 120) or (added_lines + deleted_lines) > _safe_policy_int(diff_cfg.get("very_large_diff_lines"), 12000):
        findings.append(
            Finding(
                id="very-large-diff",
                severity="P2",
                area="contracts",
                title="Diff is too large for high-confidence regression review",
                evidence=f"files={len(files)}, lines={added_lines + deleted_lines}",
                remediation="Split mechanical/generated changes from semantic changes, or run the deep campaign and document the risk.",
            )
        )

    findings = apply_exceptions(findings, policy)

    dirty = False
    if head_is_worktree:
        dirty = bool(git_output(repo, ["status", "--porcelain"]).strip())

    corpus_metrics: dict[str, Any] = {}
    corpus = baseline.get("corpus", []) if isinstance(baseline.get("corpus"), list) else []
    for entry in corpus:
        if not isinstance(entry, Mapping):
            continue
        path_text = entry.get("path")
        env_name = entry.get("path_env")
        if env_name:
            env_value = os.environ.get(str(env_name), "")
            target = Path(env_value) if env_value else Path("__rd_sentinel_missing_corpus__")
            display_path = f"${env_name}"
        elif path_text:
            target = repo / str(path_text)
            display_path = str(path_text)
        else:
            continue
        key = f"corpus_{entry.get('id', 'unknown')}_present"
        corpus_metrics[key] = target.is_file()
        expected_hash = entry.get("sha256") or entry.get("hash")
        if expected_hash and target.is_file() and sha256_file(target) != str(expected_hash):
            findings.append(
                Finding(
                    id="corpus-hash-mismatch",
                    severity="P1",
                    area="tests",
                    title=f"Pinned corpus hash mismatch: {entry.get('id')}",
                    evidence=display_path,
                    path=display_path,
                    remediation="Re-fetch the pinned APK and verify its SHA-256; do not silently re-baseline a different app build.",
                )
            )

    metrics: dict[str, Any] = {
        "changed_files": len(files),
        "added_lines": added_lines,
        "deleted_lines": deleted_lines,
        "risk_score": risk,
        "targeted_packages": affected,
        "direct_packages": direct_packages,
        "downstream_packages": downstream_packages,
        "affected_package_count": len(affected),
        "dependency_bot": dependency_bot,
        "baseline_approval_applied": baseline_approval_applied,
        "triggers": sorted(triggers),
        **base_inventory,
    }
    for key, value in head_inventory.items():
        metrics[key] = value
        metrics[f"base_{key}"] = base_inventory.get(key)
    metrics.update(dependency_metrics)
    metrics.update(public_api_metrics)
    metrics.update(named_test_metrics)
    metrics.update(workflow_metrics)
    metrics.update(corpus_metrics)

    checks = [
        CheckResult(
            id="static-diff-audit",
            title="Diff, contract, protected-path, and evidence audit",
            status="fail" if any(f.severity in {"P0", "P1"} for f in findings) else "pass",
            required=True,
            component="static",
            summary=f"{len(files)} changed files; risk {risk}/100; {len(findings)} findings",
            metrics={"areas": areas, "triggers": sorted(triggers)},
        ).to_dict()
    ]

    return {
        "repository": context.repository,
        "event_name": context.event_name,
        "campaign": context.campaign,
        "profile": context.profile,
        "base_ref": base_ref,
        "base_sha": base_sha,
        "head_ref": context.head_ref or head_ref,
        "head_sha": head_sha,
        "head_is_worktree": head_is_worktree,
        "worktree_dirty": dirty,
        "pr_number": context.pr_number,
        "actor": context.actor or context.author,
        "labels": context.labels,
        "run_url": context.run_url,
        "areas": areas,
        "risk_score": risk,
        "risk_factors": risk_factors,
        "changed_files": [
            {
                "status": f.status,
                "path": f.path,
                "old_path": f.old_path,
                "added": f.added,
                "deleted": f.deleted,
                "areas": sorted(classify_path(f.path)),
            }
            for f in files
        ],
        "dependency_changes": dependency_changes,
        "plan": plan,
        "metrics": metrics,
        "checks": checks,
        "findings": [f.to_dict() for f in findings],
    }
