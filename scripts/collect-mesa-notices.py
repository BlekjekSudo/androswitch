"""Preserve pinned Mesa license files and source copyright comments for packaging."""
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys

source, output = map(Path, sys.argv[1:])
output.mkdir(parents=True, exist_ok=True)
shutil.copytree(source / "licenses", output / "licenses", dirs_exist_ok=True)
shutil.copy2(source / "docs/license.rst", output / "license.rst")
tracked = subprocess.check_output(["git", "-c", "core.fsmonitor=false", "-C", str(source), "ls-files", "-z"]).decode().split("\0")
comments = re.compile(r"/\*.*?\*/|(?m:^(?://|#|;|\*)[^\n]*(?:\n(?://|#|;|\*)[^\n]*)*)", re.S)
notices = []
for name in tracked:
    path = source / name
    if not name or not path.is_file() or path.suffix not in {".c", ".h", ".cpp", ".rs", ".py", ".S", ".sh"}:
        continue
    if path.stat().st_size > 2 * 1024 * 1024:
        continue
    text = path.read_text(encoding="utf-8", errors="replace")
    blocks = [m.group() for m in comments.finditer(text) if any(x in m.group().lower() for x in ("copyright", "spdx-license-identifier", "permission is hereby granted", "redistribution and use"))]
    if blocks:
        notices.append("\nSOURCE: " + name + "\n" + "\n\n".join(dict.fromkeys(blocks)))
(output / "SOURCE-NOTICES.txt").write_text(
    "Conservative copyright/license comment collection from the pinned Mesa source.\n"
    "Includes source/build components that may not be linked. See each original source and license.rst.\n"
    + "\n".join(notices), encoding="utf-8")
receipt = {"source_files_with_notices": len(notices), "files": [
    {"path": str(p.relative_to(output)), "sha256": hashlib.sha256(p.read_bytes()).hexdigest()}
    for p in output.rglob("*") if p.is_file() and p.name != "notices.json"]}
(output / "notices.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
