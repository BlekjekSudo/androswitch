#!/usr/bin/env bash
# issue #72: o gate do golden M2 (run-golden.sh) saía com 0 quando o TSV vinha
# VAZIO — `total = passes + fails` dava 0 e `sys.exit(0 if fails == 0 else 1)`
# ficava verde sem comparar nenhum vetor, sustentando um "150/150" falso.
#
# Este teste exercita o gate de CONTAGEM isoladamente: TSV sintético, `rd` e
# `java` falsos — sem JDK 21, sem o binário rd real e sem os 150 casos reais.
# Roda em qualquer máquina com bash + python3.
set -uo pipefail

dir="$(cd "$(dirname "$0")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# cópia descartável do harness (não polui golden/vm/build/)
harness="$tmp/harness"
mkdir -p "$harness/build/classes" "$harness/bin"
cp "$dir/run-golden.sh" "$dir/gate.py" "$dir/expected-count.txt" "$harness/"
: > "$harness/build/classes.dex"

# `rd` falso: todo caso sintético é <void> → resposta ok/null (PASS)
cat > "$harness/bin/rd" <<'EOF'
#!/usr/bin/env bash
echo '{"status":"ok","result":{"type":"null","value":null}}'
EOF

# `java` falso: emite N linhas de TSV sintético (não compila nada)
cat > "$harness/bin/java" <<'EOF'
#!/usr/bin/env bash
n="${GOLDEN_FAKE_CASES:-150}"
i=0
while [ "$i" -lt "$n" ]; do
    echo "Arith|add${i}|(II)V|1,2|<void>"
    i=$((i + 1))
done
EOF
chmod +x "$harness/bin/rd" "$harness/bin/java"

fails=0

expect_rc() {
    local label="$1" want="$2" n="$3" rc
    GOLDEN_FAKE_CASES="$n" \
        PATH="$harness/bin:$PATH" \
        bash "$harness/run-golden.sh" "$harness/bin/rd" >"$tmp/out.txt" 2>&1
    rc=$?
    if [ "$rc" -eq "$want" ]; then
        echo "PASS $label (exit $rc — esperado $want)"
    else
        echo "FAIL $label: exit $rc, esperado $want"
        sed -n '1,6p' "$tmp/out.txt" | sed 's/^/       | /'
        fails=$((fails + 1))
    fi
}

# critérios de aceite da issue #72
expect_rc "150 vetores → gate verde (estado atual preservado)" 0 150
expect_rc "149 vetores (um a menos) → gate vermelho" 1 149
expect_rc "0 vetores (TSV vazio) → gate vermelho" 1 0

# defesa em profundidade: o gate.py também recusa sozinho (sem o bash)
pygate_rc() {
    local n="$1"
    local expected="$2"
    local d="$tmp/pygate-${1}-${2}"
    local rc
    mkdir -p "$d"
    : > "$d/expected.tsv"
    i=0
    while [ "$i" -lt "$n" ]; do
        echo "Arith|add${i}|(II)V|1,2|<void>" >>"$d/expected.tsv"
        i=$((i + 1))
    done
    python3 "$dir/gate.py" "$harness/bin/rd" "$harness/build/classes.dex" "$d" \
        "$expected" >"$d/stdout.txt" 2>&1
    rc=$?
    echo "$rc"
}

if [ "$(pygate_rc 0 150)" = "1" ]; then
    echo "PASS gate.py: TSV vazio com esperado=150 → exit 1"
else
    echo "FAIL gate.py: TSV vazio deveria sair com 1"
    fails=$((fails + 1))
fi
if [ "$(pygate_rc 149 150)" = "1" ]; then
    echo "PASS gate.py: 149 comparados com esperado=150 → exit 1"
else
    echo "FAIL gate.py: 149 comparados deveriam sair com 1"
    fails=$((fails + 1))
fi
if [ "$(pygate_rc 150 150)" = "0" ]; then
    echo "PASS gate.py: 150/150 → exit 0"
else
    echo "FAIL gate.py: 150/150 deveriam sair com 0"
    fails=$((fails + 1))
fi

# a contagem esperada precisa estar versionada (não "mágica" no script)
if [ -f "$dir/expected-count.txt" ] && grep -Eq '^[0-9]+$' "$dir/expected-count.txt"; then
    echo "PASS contagem esperada versionada: $(cat "$dir/expected-count.txt")"
else
    echo "FAIL golden/vm/expected-count.txt ausente ou inválido"
    fails=$((fails + 1))
fi

if [ "$fails" -eq 0 ]; then
    echo "TESTE DO GATE OK — verde falso (0 vetores) é impossível"
    exit 0
fi
echo "$fails verificação(ões) falharam" >&2
exit 1
