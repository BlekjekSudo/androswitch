#!/usr/bin/env python3
"""ADR 009 process-cold harness.

Spawns a brand-new `rd mcp --live` process, drives initialize + install +
launch over MCP stdio, and times exec -> first live frame
(`GET /frame.png` == 200). Reports t_install_ms, t_launch_ms,
t_frame_ms (the <26ms budget metric) and t_interactive_ms (first tap
answered; in snapshot mode this includes lazy engine materialization).

Usage:
  cold26.py --apk path.apk [--bin target/release/rd] [--runs 5]
            [--snapshot on|off] [--capture] [--phases] [--budget 26]
Exit 0 iff median t_frame_ms < budget.
"""
import argparse, hashlib, json, os, socket, statistics, subprocess, sys, time, urllib.request

def free_port():
    s = socket.socket(); s.bind(("127.0.0.1", 0)); p = s.getsockname()[1]; s.close(); return p

def rpc(proc, rid, method, params=None):
    msg = {"jsonrpc": "2.0", "id": rid, "method": method}
    if params is not None: msg["params"] = params
    proc.stdin.write(json.dumps(msg) + "\n"); proc.stdin.flush()
    while True:
        line = proc.stdout.readline()
        if not line: raise RuntimeError(f"server closed stdout during {method}")
        d = json.loads(line)
        if d.get("id") == rid: return d

def tool(proc, rid, name, args):
    d = rpc(proc, rid, "tools/call", {"name": name, "arguments": args})
    r = d.get("result", {})
    txt = ""
    for c in r.get("content", []):
        if c.get("type") == "text": txt += c.get("text", "")
    return (not r.get("is_error", False)), txt

def one_run(bin_path, apk, snap, phases, capture, extra_env):
    port = free_port()
    env = dict(os.environ); env.update(extra_env)
    if snap: env["RD_SNAPSHOT"] = "on"
    if phases: env["RD_PHASES"] = "1"
    t0 = time.perf_counter()
    proc = subprocess.Popen([bin_path, "mcp", "--live", f"127.0.0.1:{port}"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)
    try:
        rpc(proc, 1, "initialize", {})
        t_pre = time.perf_counter()
        ok, txt = tool(proc, 2, "install", {"apk": os.path.abspath(apk)})
        t_inst = time.perf_counter()
        if not ok: return {"error": "install failed: " + txt[:300]}
        ok, txt = tool(proc, 3, "launch", {})
        t_launch = time.perf_counter()
        if not ok: return {"error": "NOT_LAUNCHABLE: " + txt[:300],
                           "t_install_ms": (t_inst - t_pre) * 1e3}
        url = f"http://127.0.0.1:{port}/frame.png"
        frame = None; deadline = time.perf_counter() + 30
        while time.perf_counter() < deadline:
            try:
                req = urllib.request.urlopen(url, timeout=5)
                body = req.read()
                if req.status == 200 and body[:8] == b"\x89PNG\r\n\x1a\n":
                    frame = body; break
            except Exception:
                pass
            time.sleep(0.0005)
        t_frame = time.perf_counter()
        if frame is None: return {"error": "no frame within 30s"}
        ok_tap, _ = tool(proc, 4, "tap", {"x": 360, "y": 720})
        t_tap = time.perf_counter()
        out = {
            "t_install_ms": round((t_inst - t_pre) * 1e3, 2),
            "t_launch_rpc_ms": round((t_launch - t_inst) * 1e3, 2),
            "t_frame_ms": round((t_frame - t0) * 1e3, 2),
            "t_interactive_ms": round((t_tap - t0) * 1e3, 2),
            "frame_bytes": len(frame),
            "frame_md5": hashlib.md5(frame).hexdigest()[:12],
            "launch": txt[:160], "tap_ok": ok_tap,
        }
        if capture: out["capture"] = True
        return out
    finally:
        try: proc.stdin.close()
        except Exception: pass
        try: proc.wait(timeout=10)
        except Exception: proc.kill()
        if phases:
            err = proc.stderr.read() if proc.stderr else ""
            for line in err.splitlines():
                if line.startswith("phase "): print("  " + line, file=sys.stderr)

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--apk", required=True)
    ap.add_argument("--bin", default="target/release/rd")
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--snapshot", choices=["on", "off"], default="off")
    ap.add_argument("--capture", action="store_true", help="one untimed warm-up run (snapshot capture)")
    ap.add_argument("--phases", action="store_true")
    ap.add_argument("--budget", type=float, default=26.0)
    a = ap.parse_args()
    env = {}
    if a.capture:
        r = one_run(a.bin, a.apk, a.snapshot == "on", a.phases, True, env)
        print("capture:", json.dumps(r))
    rows = []
    for _ in range(a.runs):
        r = one_run(a.bin, a.apk, a.snapshot == "on", a.phases, False, env)
        print(json.dumps(r)); rows.append(r)
        if "error" in r: break
    ok = [r for r in rows if "t_frame_ms" in r]
    if not ok:
        print("VERDICT: FAIL (no successful frame)"); return 1
    med = statistics.median(r["t_frame_ms"] for r in ok)
    print(f"median t_frame_ms={med:.2f} budget={a.budget} -> {'PASS' if med < a.budget else 'FAIL'}")
    return 0 if med < a.budget else 1

if __name__ == "__main__":
    sys.exit(main())
