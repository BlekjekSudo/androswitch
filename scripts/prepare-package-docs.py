"""Resolve packaged local docs; link unbundled source to its exact Git commit."""
from pathlib import Path
import os
import re
import sys
from urllib.parse import quote, unquote, urlsplit
package=Path(sys.argv[1]).resolve()
root=Path(__file__).resolve().parent.parent
commit=sys.argv[2]
if not re.fullmatch(r"[0-9a-f]{40}",commit):
    raise SystemExit("Expected full source commit")
origins={"READ-ME.md":"docs/USING_ANDROSWITCH.md","MULAI-DI-SINI.md":"docs/QUICK_START_ID.md","M8-M9-STATUS.md":"docs/M8_M9_E2E.md","COMPATIBILITY.md":"docs/ANDROSWITCH_COMPATIBILITY.md","HARDWARE_FEEDBACK.md":"docs/HARDWARE_FEEDBACK.md","STARTUP_FAILURES.md":"docs/STARTUP_FAILURES.md","licenses/SOURCE-NOTICES.md":"third_party/README.md"}
for path in package.rglob("*.md"):
    relative=path.relative_to(package).as_posix()
    origin=root / origins.get(relative,relative)
    if not origin.is_file():
        continue
    def replace(match):
        target=match.group(1)
        if urlsplit(target).scheme or target.startswith(("#","//")):
            return match.group(0)
        file,separator,fragment=target.partition("#")
        source=(origin.parent / unquote(file)).resolve()
        try:
            relative_source=source.relative_to(root).as_posix()
        except ValueError:
            return match.group(0)
        bundled=package / relative_source
        if bundled.exists():
            destination=quote(os.path.relpath(bundled,path.parent).replace("\\","/"),safe="/.-_")
        else:
            destination="https://github.com/BlekjekSudo/androswitch/"+("tree" if source.is_dir() else "blob")+"/"+commit+"/"+quote(relative_source,safe="/.-_")
        return "]("+destination+("#"+fragment if separator else "")+")"
    path.write_text(re.sub(r"\]\(([^)]+)\)",replace,path.read_text(encoding="utf-8-sig")),encoding="utf-8")
