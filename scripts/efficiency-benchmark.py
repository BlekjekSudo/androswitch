#!/usr/bin/env python3
"""Paired release benchmarks with Linux wait4 CPU/RSS and semantic checksums.
Build the exact same efficiency_bench example against baseline and candidate
sources. Run while no compilation or other benchmark is active. Microbenchmarks
are targeted workloads, not evidence of an application-wide percentage gain.
"""
import argparse
import json
import os
from pathlib import Path
import random
import tempfile
import time

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("--baseline", type=Path, required=True)
p.add_argument("--candidate", type=Path, required=True)
p.add_argument("--out", type=Path, required=True)
p.add_argument("--repetitions", type=int, default=5)
p.add_argument("--workloads", nargs="+", default=["roots", "host", "sweep", "minor", "resolve", "interp"])
a = p.parse_args()
if not 1 <= a.repetitions <= 100:
    p.error("repetitions must be 1..100")
for executable in (a.baseline, a.candidate):
    if not executable.is_file():
        p.error(f"missing executable: {executable}")
a.out.parent.mkdir(parents=True, exist_ok=True)
results = []
rng = random.Random(20261020)
for workload in a.workloads:
    for repetition in range(a.repetitions):
        order = [("baseline", a.baseline), ("candidate", a.candidate)]
        rng.shuffle(order)
        pair = {}
        for label, binary in order:
            with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
                start = time.monotonic()
                pid = os.fork()
                if pid == 0:
                    os.dup2(out.fileno(), 1)
                    os.dup2(err.fileno(), 2)
                    env = dict(os.environ, RD_JIT="off", RD_GC="on")
                    os.execve(str(binary.resolve()), [str(binary.resolve()), workload], env)
                _, status, usage = os.wait4(pid, 0)
                out.seek(0)
                err.seek(0)
                stdout, stderr = out.read().decode(), err.read().decode()
                if status != 0:
                    raise RuntimeError(f"{label}/{workload} status={status}\n{stdout}\n{stderr}")
                record = json.loads(stdout)
                record.update(label=label, repetition=repetition, wall_seconds=time.monotonic()-start,
                              user_seconds=usage.ru_utime, system_seconds=usage.ru_stime,
                              max_rss_kib=usage.ru_maxrss, stderr=stderr)
                results.append(record)
                pair[label] = record
                a.out.write_text(json.dumps(results, indent=2) + "\n")
                print(json.dumps(record), flush=True)
        assert pair["baseline"]["checksum"] == pair["candidate"]["checksum"], pair
        assert pair["baseline"]["iterations"] == pair["candidate"]["iterations"], pair
print(f"PASS: {len(results)} assertion-bearing workload runs; paired checksums match")
