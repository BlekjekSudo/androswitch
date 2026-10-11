#!/usr/bin/env python3
"""Gate do golden M2 — compara `rd vm exec` com a JVM real, campo a campo.

Extraído de `run-golden.sh` (issue #72) para ser testável isoladamente
(`golden/vm/test-gate.sh`) sem precisar de JDK, de `rd` nem dos vetores reais.

Uso: gate.py <rd> <classes.dex> <build-dir> <expected-count>
Exit 0 SOMENTE se o número de vetores comparados for exatamente
`expected-count` e nenhum caso falhar.
"""

import json
import subprocess
import sys
from pathlib import Path

rd, dex, build = sys.argv[1], sys.argv[2], Path(sys.argv[3])
expected_count = (
    int(sys.argv[4]) if len(sys.argv) > 4 else read_expected_count(build)
)
tsv = [
    line
    for line in (build / "expected.tsv").read_text(encoding="utf-8").splitlines()
    if line.strip()
]
actual_path = build / "actual.jsonl"
report_path = build / "report.txt"

def norm_vm_value(ret, val):
    """Normaliza o value da VM (JSON) para a forma do runner JVM."""
    if ret == "Z":
        return {"1": "true", "0": "false"}.get(val, val)
    if ret == "C":
        try:
            return chr(int(val))
        except (ValueError, OverflowError):
            return "<char?%s>" % val
    return val  # I/J/F/D/B/S: mesma representação textual

def ret_of(sig):
    return sig[sig.rindex(")") + 1:]

def read_expected_count(build: Path) -> int:
    """Contagem esperada versionada (golden/vm/expected-count.txt)."""
    path = Path(__file__).with_name("expected-count.txt")
    for candidate in (path, build.parent / "expected-count.txt"):
        if candidate.is_file():
            text = candidate.read_text(encoding="utf-8").strip()
            if text.isdigit():
                return int(text)
    print(
        "FALHA DE GATE: golden/vm/expected-count.txt ausente ou inválido",
        file=sys.stderr,
    )
    sys.exit(2)


passes, fails, lines = 0, 0, []
with actual_path.open("w", encoding="utf-8") as actual:
    for line in tsv:
        parts = line.split("|", 4)
        if len(parts) != 5:
            fails += 1
            lines.append(f"FAIL <tsv malformado>: {line!r}")
            continue
        cls, met, sig, args, expected = parts
        cmd = [rd, "vm", "exec", dex,
               "--class", f"Lbr/rustydroid/vm/{cls};",
               "--method", met,
               "--sig", sig,
               "--json"]
        if args:
            # sintaxe --args=VALOR: args começando com '-' (ex.: '-7') seriam
            # interpretados como flag pelo clap na forma separada '--args -7'
            cmd += ["--args=" + args]
        name = f"{cls}.{met}{sig}"
        try:
            proc = subprocess.run(cmd, capture_output=True, text=True,
                                  timeout=120)
        except subprocess.TimeoutExpired:
            fails += 1
            lines.append(f"FAIL {name}: TIMEOUT (120s)")
            continue
        raw = proc.stdout.strip()
        actual.write(raw + "\n")
        got = None
        if raw:
            try:
                got = json.loads(raw)
            except json.JSONDecodeError:
                pass
        if got is None:
            fails += 1
            lines.append(f"FAIL {name}: saída não-JSON (rc={proc.returncode}) {raw!r} {proc.stderr.strip()!r}")
            continue

        status = got.get("status")
        # ── exceção esperada: compara APENAS a classe (msg é texto livre) ──
        if expected.startswith("EXCEPTION:"):
            want_cls = expected.split(":", 2)[1]
            if status == "exception":
                got_cls = got.get("class", "")
                if got_cls == want_cls:
                    passes += 1
                    lines.append(f"PASS {name} (exceção {want_cls})")
                else:
                    fails += 1
                    lines.append(f"FAIL {name}: classe de exceção esperada {want_cls}, VM devolveu {got_cls} (msg {got.get('message','')!r})")
            else:
                fails += 1
                lines.append(f"FAIL {name}: esperava EXCEPTION {want_cls}, VM devolveu status={status} {got}")
            continue

        # ── resultado normal ────────────────────────────────────────────────
        if status != "ok":
            fails += 1
            if status == "error":
                lines.append(f"FAIL {name}: erro VM {got.get('code')}: {got.get('cause')}")
            else:
                lines.append(f"FAIL {name}: esperava resultado, VM devolveu {got}")
            continue
        result = got.get("result")
        if expected == "<void>":
            # contrato 5-b: void → result nulo; a implementação devolve
            # {"type":"null","value":null} — ambos os formatos aceitos
            if result is None or result.get("type") == "null":
                passes += 1
                lines.append(f"PASS {name} (<void>)")
            else:
                fails += 1
                lines.append(f"FAIL {name}: esperava <void>, VM devolveu {result}")
            continue
        if result is None or not isinstance(result.get("value"), str):
            fails += 1
            lines.append(f"FAIL {name}: esperava {expected!r}, VM devolveu {result}")
            continue
        got_val = norm_vm_value(ret_of(sig), result["value"])
        if got_val == expected:
            passes += 1
            lines.append(f"PASS {name} = {expected}")
        else:
            fails += 1
            lines.append(f"FAIL {name}: esperado {expected!r} (tipo {ret_of(sig)}), VM {got_val!r} (type {result.get('type')})")

total = passes + fails
summary = f"RESUMO GOLDEN M2: {passes}/{total} PASS, {fails} FAIL"
report_path.write_text("\n".join(lines) + "\n" + summary + "\n", encoding="utf-8")
print("\n".join(lines))
print(summary)

# issue #72: um TSV vazio (ou truncado) dava `total == 0`, `fails == 0` e o
# gate saía com 0 — verde sem comparar nada. A contagem esperada é versionada
# em golden/vm/expected-count.txt e comparada AQUI e no bash.
if total != expected_count:
    print(
        f"FALHA DE GATE: esperava {expected_count} vetores, comparei {total}. "
        "O golden harness não produziu a quantidade de casos do DoD do M2.",
        file=sys.stderr,
    )
    sys.exit(1)
sys.exit(0 if fails == 0 else 1)
