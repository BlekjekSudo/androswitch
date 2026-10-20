"""Reproducible real-APK MCP regression/endurance runner (no external MCP SDK)."""

import subprocess, json, time, os, sys, pathlib, selectors, signal, argparse

parser = argparse.ArgumentParser(
    description="Comprix 1.6.0 real-APK regression and endurance, using the in-house stdio MCP server"
)
parser.add_argument(
    "--rd",
    type=pathlib.Path,
    default=pathlib.Path(__file__).resolve().parents[1] / "target/release/rd",
)
parser.add_argument("--apk", type=pathlib.Path, required=True)
parser.add_argument(
    "--out",
    type=pathlib.Path,
    default=pathlib.Path(__file__).resolve().parents[1] / "target/comprix-campaigns",
)
parser.add_argument("--name", default="soak")
parser.add_argument("--cycles", type=int, default=10)
parser.add_argument("--gc", choices=["on", "off"], default="on")
parser.add_argument("--jit", choices=["on", "off"], default="off")
parser.add_argument("--typed", action="store_true")
parser.add_argument(
    "--deadline",
    default="",
    help="optional ISO-8601 UTC/offset timestamp checked after each cycle",
)
a = parser.parse_args()
if not 1 <= a.cycles <= 10000:
    parser.error("--cycles must be 1..10000")
if not a.apk.is_file():
    parser.error("--apk must be a real local APK file")
if not a.rd.is_file():
    parser.error("--rd not found; build rd-cli first")
root = pathlib.Path(__file__).resolve().parents[1]
a.apk = a.apk.resolve()
a.rd = a.rd.resolve()
folder = (a.out / a.name).resolve()
folder.mkdir(parents=True, exist_ok=True)
env = dict(
    os.environ,
    RD_GC=a.gc,
    RD_JIT=a.jit,
    RD_MCP_SANDBOX=os.pathsep.join([str(a.apk.parent), str(folder)]),
)
p = subprocess.Popen(
    [str(a.rd), "mcp"],
    cwd=str(root),
    env=env,
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=(folder / "stderr.log").open("wb"),
    bufsize=0,
    start_new_session=True,
)
# Let start_process/shell cancellation run the cleanup below, even though the
# app server has a separate process group for reliable timeout termination.
signal.signal(signal.SIGTERM, lambda _s, _f: sys.exit(143))
sel = selectors.DefaultSelector()
sel.register(p.stdout, selectors.EVENT_READ)
pending = b""
seq = 0
failures = []
start = time.monotonic()
health = []
log = (folder / "transcript.jsonl").open("w")
summary = {
    "name": a.name,
    "gc": a.gc,
    "jit": a.jit,
    "cycles_requested": a.cycles,
    "cycles_completed": 0,
    "steps": 0,
    "failures": failures,
}


def rpc(name, args=None, expect=None, title=False):
    global seq, pending
    seq += 1
    req = {
        "jsonrpc": "2.0",
        "id": seq,
        "method": "tools/call",
        "params": {"name": name, "arguments": args or {}},
    }
    t = time.monotonic()
    p.stdin.write((json.dumps(req) + "\n").encode())
    limit = time.monotonic() + 120
    while True:
        while b"\n" not in pending:
            remaining = limit - time.monotonic()
            if remaining <= 0 or not sel.select(max(0, remaining)):
                raise RuntimeError(f"MCP timeout at {name}")
            chunk = os.read(p.stdout.fileno(), 65536)
            if not chunk:
                raise RuntimeError(f"MCP exited {p.poll()} during {name}")
            pending += chunk
        line, pending = pending.split(b"\n", 1)
        response = json.loads(line)
        if response.get("id") == seq:
            break
    elapsed = round((time.monotonic() - t) * 1000, 2)
    result = response.get("result", {})
    texts = [
        c.get("text", "") for c in result.get("content", []) if c.get("type") == "text"
    ]
    joined = "\n".join(texts)
    entry = {
        "seq": seq,
        "name": name,
        "arguments": args or {},
        "ms": elapsed,
        "response": response,
    }
    log.write(json.dumps(entry, ensure_ascii=False) + "\n")
    log.flush()
    summary["steps"] = seq
    if result.get("isError") or response.get("error"):
        failures.append({"seq": seq, "name": name, "error": joined})
        raise RuntimeError(f"{name}: {joined[:1000]}")
    if "!! draw pass failed" in joined:
        failures.append({"seq": seq, "name": name, "warning": joined.split("!!")[-1]})
        raise RuntimeError("frame/provider warning: " + joined.split("!!")[-1])
    observed = next((t for t in texts if t.startswith("frame v")), joined).split(
        "-- changed since"
    )[0]
    if title:
        observed = "\n".join(observed.splitlines()[:5])
    if expect and expect not in observed:
        failures.append(
            {"seq": seq, "name": name, "missing": expect, "observed": joined}
        )
        raise RuntimeError(f"{name}: missing {expect}")
    print(f"{a.name} step={seq} {name} {elapsed}ms", flush=True)
    if name == "runtime_health":
        health.append(json.loads(texts[0]))
        summary["health"] = health
    return result, texts


try:
    rpc("install", {"apk": str(a.apk)})
    rpc("launch", {"observe": True})
    rpc("tap_text", {"text": "Pular", "observe": True}, "Minhas listas")
    rpc("observe", {"format": "text"}, "Minhas lojas")
    rpc("tap_text", {"text": "Minhas lojas", "observe": True}, "Nome da loja")
    rpc("key", {"key": "BACK", "observe": True}, "Minhas listas")
    if a.typed:
        rpc("tap_text", {"text": "Nova lista", "observe": True})
        rpc("wait", {"ms": 500, "observe": True}, "Criar e adicionar itens")
        rpc("tap_text", {"text": "Compra do Mês", "observe": True})
        rpc(
            "tap_text",
            {"text": "Criar e adicionar itens", "observe": True},
            "Adição rápida",
        )
        rpc("wait", {"ms": 400})
        rpc("tap", {"x": 260, "y": 309})
        rpc(
            "type_text",
            {"text": "2 kg de arroz", "replace": True, "observe": True},
            "2 kg de arroz",
        )
        rpc("key", {"key": "ENTER", "observe": True}, "Arroz")
        rpc("wait", {"ms": 500, "observe": True}, "1 item")
        rpc("screenshot", {"path": str(folder / "typed-item.png")})
        rpc("key", {"key": "BACK", "observe": True}, "Minhas listas")
    # Deliberately exclude the camera tab: physical camera/JNI is a separate gap.
    for cycle in range(a.cycles):
        for label, expected in [
            ("Ajustes", "Configurações"),
            ("Histórico", "Histórico e economia"),
            ("Comparar", "Comparar"),
            ("Listas", "Minhas listas"),
        ]:
            _, observed = rpc(
                "tap_text", {"text": label, "observe": True}, expected, title=True
            )
            if a.typed and label in {"Listas", "Comparar"}:
                frame = next(t for t in observed if t.startswith("frame v")).split(
                    "-- changed since"
                )[0]
                count = "1 item" if label == "Listas" else "1 itens"
                if "Compra do Mês" not in frame or count not in frame:
                    raise RuntimeError("typed list/item disappeared during tab cycling")
        rpc("runtime_health", {})
        summary["cycles_completed"] = cycle + 1
        summary["elapsed_seconds"] = round(time.monotonic() - start, 2)
        (folder / "summary.json.tmp").write_text(
            json.dumps(summary, ensure_ascii=False, indent=2)
        )
        (folder / "summary.json.tmp").replace(folder / "summary.json")
        print(f'CYCLE {cycle+1} DONE elapsed={summary["elapsed_seconds"]}', flush=True)
        if a.deadline:
            import datetime

            if datetime.datetime.now(
                datetime.timezone.utc
            ) >= datetime.datetime.fromisoformat(a.deadline):
                break
    rpc("runtime_health", {"gc": True} if a.gc == "on" else {})
    if a.typed:
        rpc("tap_text", {"text": "Abrir", "observe": True}, "Arroz")
        _, observed = rpc("wait", {"ms": 400, "observe": True}, "Arroz")
        frame = next(t for t in observed if t.startswith("frame v")).split(
            "-- changed since"
        )[0]
        if "2 x kg" not in frame or "1 item" not in frame:
            raise RuntimeError("typed quantity/item changed after final collection")
        rpc("screenshot", {"path": str(folder / "endurance-final.png")})
        rpc("key", {"key": "BACK", "observe": True}, "Minhas listas")
        summary["final_item_verified"] = True
    summary["success"] = True
except Exception as e:
    summary["success"] = False
    summary["fatal"] = str(e)
    print("CAMPAIGN FAILED", e, flush=True)
finally:
    summary["elapsed_seconds"] = round(time.monotonic() - start, 2)
    (folder / "summary.json").write_text(
        json.dumps(summary, ensure_ascii=False, indent=2)
    )
    log.close()
    try:
        os.killpg(p.pid, signal.SIGTERM)
        p.wait(timeout=5)
    except Exception:
        try:
            os.killpg(p.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    print(
        json.dumps(
            {k: v for k, v in summary.items() if k != "health"}, ensure_ascii=False
        ),
        flush=True,
    )
    sys.exit(0 if summary.get("success") else 1)
