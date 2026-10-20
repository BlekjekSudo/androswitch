#!/usr/bin/env python3
"""Summarize assertion-bearing paired Linux microbenchmarks without app-wide claims.

The interval estimates the MEDIAN OF PAIRED RATIOS, not the ratio of medians.
Intervals are descriptive percentile bootstraps, not multiplicity-adjusted, and
cannot account for all host noise or establish an absolute performance bound.
"""
import argparse
import json
from pathlib import Path
import random
import statistics

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--evidence", type=Path, required=True)
parser.add_argument("--input", default="final-original-nine-paired.json")
parser.add_argument("--resamples", type=int, default=10000)
args = parser.parse_args()
if args.resamples < 1000:
    parser.error("--resamples must be at least 1000")
rows = json.loads((args.evidence / args.input).read_text())
expected = {"roots", "host", "sweep", "minor", "resolve", "interp", "strings", "subtype", "jit"}
assert {row["workload"] for row in rows} == expected
summary = []
for name in dict.fromkeys(row["workload"] for row in rows):
    selected = [row for row in rows if row["workload"] == name]
    assert {row["label"] for row in selected} == {"baseline", "candidate"}
    baseline = [row for row in selected if row["label"] == "baseline"]
    candidate = [row for row in selected if row["label"] == "candidate"]
    before = {row["repetition"]: row for row in baseline}
    after = {row["repetition"]: row for row in candidate}
    assert len(before) == len(baseline) == len(after) == len(candidate) >= 2
    assert before.keys() == after.keys()
    paired = []
    for repetition in before:
        a, b = before[repetition], after[repetition]
        assert a["checksum"] == b["checksum"]
        assert a["iterations"] == b["iterations"]
        assert a["rss_source"] == b["rss_source"] == "gnu-time-child"
        assert a["elapsed_ms"] > 0 and b["elapsed_ms"] > 0
        paired.append(a["elapsed_ms"] / b["elapsed_ms"])
    rng = random.Random(20261020)
    boot = sorted(statistics.median(rng.choices(paired, k=len(paired)))
                  for _ in range(args.resamples))
    low = boot[int(args.resamples * 0.025)]
    high = boot[int(args.resamples * 0.975) - 1]
    bm = statistics.median(row["elapsed_ms"] for row in baseline)
    cm = statistics.median(row["elapsed_ms"] for row in candidate)
    br = statistics.median(row["max_rss_kib"] for row in baseline)
    cr = statistics.median(row["max_rss_kib"] for row in candidate)
    result = dict(workload=name, pairs=len(paired), baseline_ms=bm, candidate_ms=cm,
                  ratio_of_medians=bm / cm,
                  median_paired_speedup=statistics.median(paired),
                  paired_bootstrap_ci95=[low, high],
                  baseline_rss_kib=br, candidate_rss_kib=cr,
                  rss_reduction_percent=100 * (br - cr) / br,
                  checksum=baseline[0]["checksum"],
                  cpu_interpretation=("CI above parity" if low > 1 else
                                      "CI below parity" if high < 1 else "CI includes parity"))
    summary.append(result)
    print(f"{name}: {bm:.3f} -> {cm:.3f} ms; ratio of medians {bm/cm:.3f}x; "
          f"median paired ratio {result['median_paired_speedup']:.3f}x "
          f"(descriptive 95% CI {low:.3f}-{high:.3f}); RSS {br}->{cr} KiB")
(args.evidence / "final-benchmark-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
