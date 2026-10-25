"""Validate actual UI callbacks, export and reset/stop persistence after E2E."""
import argparse
import hashlib
import json
from pathlib import Path

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("--receipt", required=True, type=Path)
a = p.parse_args()
r = json.loads(a.receipt.read_text(encoding="utf-8-sig"))
assert r["sd_original_restore"] == "restored" and r["completed_sessions"] == 1
case = r["cases"][0]
assert case["runtime_status"] == "passed_execution" and case["returned_to_launcher"]
views = [v for s in case["snapshots"] for v in s.get("views", [])]
texts = [v.get("text", "") for v in views]
assert any(t.startswith("Conformance | DEX |") for t in texts), "Next category callback"
assert "dex.division | NOT_RUN" in texts, "Bottom Next result callback"
assert "Exported Download/AndroSwitch-Conformance/report.json" in texts
assert "Stopped; saved report.json" in texts
buttons = [v for v in views if v.get("text") == "Next result"]
assert buttons and all(v["bounds"][1] >= 0 and v["bounds"][3] > 0
                       and v["bounds"][1] + v["bounds"][3] <= 360 for v in buttons)
sd = Path(r["sd_result"])
files = sd / "userdata/org.androswitch.conformance/files"
assert (files / "autorun.txt").read_text().strip() == "0"
export = sd / "Download/AndroSwitch-Conformance/report.json"
journal = files / "conformance/report.json"
history = list((files / "conformance").rglob("history-*.json"))
assert len(history) == 1, "Reset preserves the prior report"
for file in [export, journal, *history]:
    rows = json.loads(file.read_text())["cases"]
    assert len(rows) == 219 and len({row["id"] for row in rows}) == 219
    assert all(row["status"] == "NOT_RUN" for row in rows), "UI run must not fabricate compatibility passes"
out = dict(schema_version=1, status="passed_ui_contracts", compatibility_pass=False,
           scope="Touch-driven category/result navigation, visible bottom row, export, reset history and persistent stop; not 219-case acceptance",
           nro_sha256=r["nro_sha256"], apk_sha256=r["inputs"][0]["sha256"],
           visual_review="Separate observed framebuffer; automatic validation covers callbacks and persisted output",
           files=[dict(path=str(f), sha256=hashlib.sha256(f.read_bytes()).hexdigest())
                  for f in [a.receipt, export, journal, *history, files / "autorun.txt"]])
destination = a.receipt.parent / "conformance-ui-summary.json"
destination.write_text(json.dumps(out, indent=2), encoding="utf-8")
print(destination)
