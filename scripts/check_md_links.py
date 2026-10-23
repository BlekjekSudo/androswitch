#!/usr/bin/env python3
"""CI gate: every internal markdown link must resolve (issue #6).

Scanned files:
  * Current README/contribution/security files, CREDITS.md, NOTICE.md,
    RustyDroid.md and preserved *.rustydroid.md documents (repo root)
  * every *.md under docs/ and .github/ (recursive)

Checked links:
  * inline markdown links ``](target)`` whose target is a repository-relative
    path (bare ``X.md``, ``./X.md``, ``../X.md``; a leading ``/`` resolves
    against the repo root, matching GitHub rendering).
  * Skipped: external targets (``http://``, ``https://``, ``mailto:`` or any
    other ``scheme:``), protocol-relative ``//...`` and fragment-only links
    (``#anchor`` -> same page).
  * URL escapes are decoded (``%20`` -> space).

Fragment handling:
  * ``](path.md#anchor)`` requires (a) the target file to exist and
    (b) ``anchor`` to match a heading in the target file using GitHub-style
    anchor slugs: lowercase; backticks stripped; anything outside
    [a-z0-9 _-] removed; spaces -> '-'; duplicate headings get ``-1``/``-2``
    suffixes. Matching is exact/case-sensitive (GitHub anchors are lowercase).

Output:
  * ``::error file=...,line=...::`` annotation per broken link
  * one summary line
  * exit 1 on any broken link, 0 otherwise.

Python 3 standard library only — no runtime dependencies.
"""

from __future__ import annotations

import re
import sys
import urllib.parse
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

ROOT_FILES = (
    "README.md", "CONTRIBUTING.md", "SECURITY.md", "RustyDroid.md",
    "CREDITS.md", "NOTICE.md", "README.rustydroid.md",
    "CONTRIBUTING.rustydroid.md", "SECURITY.rustydroid.md",
)
SCAN_DIRS = ("docs", ".github")

LINK_RE = re.compile(r"\]\(([^)]*)\)")
FENCE_RE = re.compile(r"^\s{0,3}(?:```+|~~~+)")
HEADING_RE = re.compile(r"^(#{1,6})\s+(.+?)\s*$")
SCHEME_RE = re.compile(r"^[A-Za-z][A-Za-z0-9+.\-]*:")
DEST_RE = re.compile(r"^(\S+)(?:\s+[\"'][^\"']*[\"'])?$")


def strip_code_spans(line: str) -> str:
    """Remove `inline code` (content included) so example links are ignored."""
    return re.sub(r"`[^`]*`", " ", line)


def strip_fenced_blocks(lines: list[str]) -> list[str]:
    """Drop fenced code blocks (```/~~~) — links shown as examples stay unchecked."""
    out: list[str] = []
    inside = False
    for line in lines:
        if FENCE_RE.match(line):
            inside = not inside
            continue
        if not inside:
            out.append(line)
    return out


def github_slug(text: str) -> str:
    """GitHub-style heading anchor slug (see module docstring)."""
    text = text.replace("`", "")
    text = text.lower()
    text = re.sub(r"[^a-z0-9 _-]", "", text)
    text = text.replace(" ", "-")
    return text


def heading_slugs(path: Path) -> frozenset[str]:
    """All GitHub anchor slugs of ATX headings (#{1,6}) in *path*."""
    slugs: list[str] = []
    counts: dict[str, int] = {}
    try:
        raw = path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return frozenset()
    for line in strip_fenced_blocks(raw.splitlines()):
        m = HEADING_RE.match(line.rstrip())
        if not m:
            continue
        text = m.group(2).strip()
        text = re.sub(r"\s+#+\s*$", "", text)  # closing # sequence
        slug = github_slug(text)
        n = counts.get(slug, 0)
        counts[slug] = n + 1
        slugs.append(slug if n == 0 else f"{slug}-{n}")
    return frozenset(slugs)


def extract_links(path: Path) -> list[tuple[int, str]]:
    """(line_no, target) for every relative inline link target in *path*."""
    try:
        raw = path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return []
    found: list[tuple[int, str]] = []
    for line_no, line in enumerate(strip_fenced_blocks(raw.splitlines()), 1):
        line = strip_code_spans(line)
        for m in LINK_RE.finditer(line):
            inner = m.group(1).strip()
            if not inner:
                continue
            if inner.startswith("<") and inner.endswith(">"):  # <destination>
                inner = inner[1:-1].strip()
                if not inner:
                    continue
            # The destination is either one token or a token plus a quoted title;
            # anything else is not a rendered markdown link.
            dm = DEST_RE.match(inner)
            if not dm:
                continue
            target = dm.group(1)
            if target.startswith("#"):
                continue  # fragment-only (same page) — out of scope
            if SCHEME_RE.match(target):
                continue  # external: http/https/mailto/...
            if target.startswith("//"):
                continue  # protocol-relative URL — external
            found.append((line_no, target))
    return found


def rel(p: Path) -> str:
    try:
        return p.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return p.as_posix()


def gather_files() -> tuple[list[Path], list[Path]]:
    files: list[Path] = []
    missing: list[Path] = []
    for name in ROOT_FILES:
        p = ROOT / name
        (files if p.is_file() else missing).append(p)
    for d in SCAN_DIRS:
        dd = ROOT / d
        if dd.is_dir():
            files.extend(sorted(p for p in dd.rglob("*.md") if p.is_file()))
    seen: set[Path] = set()
    unique: list[Path] = []
    for p in files:
        key = p.resolve()
        if key not in seen:
            seen.add(key)
            unique.append(p)
    return unique, missing


def main() -> int:
    scan_files, missing = gather_files()
    errors = 0
    checked = 0
    slug_cache: dict[Path, frozenset[str]] = {}

    for p in missing:
        print(f"::error file={p.name}::link-check scan target missing: {p.name}")
        errors += 1

    for f in scan_files:
        file_rel = rel(f)
        base = f.parent
        for line_no, target in extract_links(f):
            checked += 1
            path_part, sep, frag = target.partition("#")
            frag = frag if (sep and frag) else None
            decoded = urllib.parse.unquote(path_part)
            if decoded.startswith("/"):
                dest = ROOT / decoded.lstrip("/")
            else:
                dest = base / decoded
            if not dest.exists():
                print(
                    f"::error file={file_rel},line={line_no}::"
                    f"broken internal link '{target}' -> target not found: {rel(dest)}"
                )
                errors += 1
                continue
            if frag is None:
                continue
            if not dest.is_file():
                print(
                    f"::error file={file_rel},line={line_no}::"
                    f"link '{target}' attaches a fragment to a non-file target"
                )
                errors += 1
                continue
            key = dest.resolve()
            if key not in slug_cache:
                slug_cache[key] = heading_slugs(dest)
            if frag not in slug_cache[key]:
                print(
                    f"::error file={file_rel},line={line_no}::"
                    f"broken anchor in '{target}' -> no heading in {rel(dest)} "
                    f"matches fragment '#{frag}'"
                )
                errors += 1

    if errors:
        print(
            f"❌ {errors} problem(s) found — checked {checked} internal link(s) "
            f"across {len(scan_files)} markdown file(s)."
        )
        return 1
    print(
        f"✅ All internal markdown links resolve — checked {checked} link(s) "
        f"across {len(scan_files)} file(s)."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
