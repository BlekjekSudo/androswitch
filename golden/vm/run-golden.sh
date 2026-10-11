#!/usr/bin/env bash
# golden/vm/run-golden.sh — executa os vetores golden do M2: JVM real vs rd VM.
#
# Passo 1: GoldenRunner na JVM real (java -cp build/classes) → build/expected.tsv
# Passo 2: para cada linha do TSV, `rd vm exec build/classes.dex --class
#          Lbr/rustydroid/vm/<Classe>; --method <m> --sig <s> --args <a> --json`
#          (contrato 5-b: {"status":"ok","result":{"type":..,"value":..}} com
#          value STRING; exceção → {"status":"exception","class","message"}).
# Passo 3: comparação campo a campo (python3 embutido):
#          - int/long/float/double/byte/short: string igual (float/double no
#            formato Java dos DOIS lados);
#          - boolean: VM devolve "1"/"0" (int cru no JSON) → normalizado p/
#            true/false; char: VM devolve o código → normalizado p/ o caractere;
#          - void: VM result null ↔ runner <void>;
#          - exceção: compara APENAS a classe (a mensagem é texto livre e
#            difere entre ART/JVM e a VM mínima — decisão documentada no
#            README; o conteúdo de mensagens é coberto pelos casos
#            customMsgLen/customHash, que passam por length/hashCode).
#
# Uso: ./run-golden.sh [caminho-do-rd]
#      (default: ../../target/release/rd relativo a este script)
# Saída: build/actual.jsonl (respostas brutas) + relatório no stdout.
# Exit 0 SOMENTE se 100% dos casos derem PASS.
set -euo pipefail

dir="$(cd "$(dirname "$0")" && pwd)"
build="$dir/build"
RD_BIN="${1:-$dir/../../target/release/rd}"
DEX="$build/classes.dex"
CLASSES="$build/classes"

if [ ! -x "$RD_BIN" ]; then
    echo "ERRO: binário rd não encontrado/executável: $RD_BIN" >&2
    echo "      passe o caminho como \$1 (ex.: ./run-golden.sh ../../target/release/rd)" >&2
    exit 2
fi
if [ ! -e "$DEX" ] || [ ! -d "$CLASSES" ]; then
    echo "[run-golden] build ausente — rodando ./build.sh primeiro"
    "$dir/build.sh"
fi

# issue #72: a contagem esperada (DoD do M2) é VERSIONADA, não "mágica" no
# script — o gate falha quando o harness produz menos vetores do que o DoD.
expected_file="$dir/expected-count.txt"
if [ ! -f "$expected_file" ]; then
    echo "ERRO: $expected_file ausente (contagem de vetores do DoD do M2)" >&2
    exit 2
fi
expected=$(tr -d '[:space:]' < "$expected_file")
if ! printf '%s' "$expected" | grep -Eq '^[0-9]+$'; then
    echo "ERRO: $expected_file inválido (conteúdo: '$expected')" >&2
    exit 2
fi

echo "[run-golden] binário: $RD_BIN"
echo "[run-golden] passo 1: GoldenRunner na JVM real → build/expected.tsv"
java -cp "$CLASSES" br.rustydroid.vm.GoldenRunner > "$build/expected.tsv"
# `grep -c ''` conta a última linha mesmo sem newline final (wc -l não conta).
n_cases=$(grep -c '' "$build/expected.tsv" || true)
echo "[run-golden] ${n_cases} vetores gerados (esperado: ${expected})"
if [ "$n_cases" -ne "$expected" ]; then
    echo "[run-golden] FALHA: esperava ${expected} vetores, o harness gerou ${n_cases}" >&2
    echo "[run-golden] o gate não pode ficar verde com o conjunto errado de casos" >&2
    exit 1
fi

echo "[run-golden] passo 2+3: rd vm exec por caso + comparação"
python3 "$dir/gate.py" "$RD_BIN" "$DEX" "$build" "$expected"
rc=$?
if [ "$rc" -eq 0 ]; then
    echo "[run-golden] TODOS os vetores PASS — DoD do M2 satisfeito neste pipeline"
else
    echo "[run-golden] FALHAS acima — detalhe completo em build/report.txt" >&2
fi
exit "$rc"
