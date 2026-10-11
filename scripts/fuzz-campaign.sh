#!/usr/bin/env bash
#
# scripts/fuzz-campaign.sh — Campanha de fuzz 24h (DoD do M1) — ver docs/FUZZING.md
#
# Política (docs/FUZZING.md): os 5 targets (zip, axml, arsc, apk_full, dex)
# rodam em PARALELO por N segundos (padrão 86400 = 24 h), em modo -fork com
# -ignore_crashes=1 — o fuzzer CONTINUA após crash/oom/timeout e arquiva cada
# achado em crates/*/fuzz/artifacts/.
#
#   DoD atingido  = ao final, NENHUM arquivo `crash-*` nos artifacts E os 5
#                   targets rodaram os SECS íntegros (veredito honesto: um
#                   target que morreu no build NÃO conta como "sem crash").
#   DoD violado   = qualquer `crash-*` é bug P0 (Lei nº 1) → minimizar + issue.
#                   (oom-*/timeout-* são AVISOS — investigar depois; não violam
#                    o DoD escrito, que é "sem crash".)
#
# Uso:
#   ./scripts/fuzz-campaign.sh                  # 24 h (86400 s), tudo automático
#   ./scripts/fuzz-campaign.sh --seeds-only     # só popula os corpus (2 min, sem Rust)
#   ./scripts/fuzz-campaign.sh 600              # smoke de 10 min (FAÇA ANTES DAS 24h)
#   ./scripts/fuzz-campaign.sh 28800 4          # 8 h, 4 workers por target
#   ./scripts/fuzz-campaign.sh --help           # ajuda
#
# Memória (auto-calibrada): lê MemAvailable e escolhe workers × rss_limit para
# NÃO estourar a RAM (notebook 16 GB/WSL 12 GB → tipicamente workers=1, rss
# ~1,7 GB por processo). Sobrescreva com WORKERS (2º arg) e/ou RD_FUZZ_RSS=<MB>.
#
# Requisitos: rustup + toolchain nightly + cargo-fuzz@0.13.2 (versão do CI) +
# unzip + curl (fetch do APK golden). Corpus persiste em crates/*/fuzz/corpus/
# (gitignored) — rodar de novo CONTINUA a campanha a partir do corpus existente.
#
# WSL2/kernel 6.6+: se vm.mmap_rnd_bits > 28, o AddressSanitizer morre com
# SEGV em segundos (a campanha viraria lixo). O script detecta e ABORTA antes
# com o comando de correção. Escape: RD_FUZZ_SKIP_SYSCTL_CHECK=1
#
# DISCO: o corpus cresce bastante nas primeiras horas (unidades novas têm até
# o tamanho de max_len). Cheque de vez em quando:
#     du -sh crates/*/fuzz/corpus
# Se apertar o disco, PARE, minimize e retome (a cobertura NÃO se perde) —
# dentro de crates/rd-apk (repita para zip/axml/arsc; análogo em rd-dex):
#     cargo fuzz merge corpus.min corpus/apk_full -f apk_full
#     rm -rf corpus/apk_full && mv corpus.min/apk_full corpus/apk_full

set -euo pipefail

usage() {
    cat >&2 <<'EOF'
RustyDroid — campanha de fuzz (DoD do M1, ver docs/FUZZING.md)

Uso: ./scripts/fuzz-campaign.sh [--seeds-only] [SECS] [WORKERS]

  SECS     segundos por target (padrão 86400 = 24 h; os 5 targets rodam em paralelo)
  WORKERS  processos libFuzzer por target (padrão 0 = auto por CPU+RAM)
  RD_FUZZ_RSS=<MB>  força o rss_limit por processo (padrão: auto)

Passo a passo recomendado (WSL2):
  ./scripts/fuzz-campaign.sh --seeds-only   # 2 min — valida seeds/corpus
  ./scripts/fuzz-campaign.sh 600            # smoke 10 min — valida build/execução
  ./scripts/fuzz-campaign.sh                # campanha 24 h (dentro do tmux!)

Exit: 0 = ✅ sem crash/leak (DoD ok) · 1 = ❌ crash/leak achado (P0, issue #58) · 2 = ❌ target não executou
EOF
}

# ----------------------------------------------------------------- args ----
if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then usage; exit 0; fi

SEEDS_ONLY=0
if [ "${1:-}" = "--seeds-only" ]; then
    SEEDS_ONLY=1
    shift
fi

SECS="${1:-86400}"                 # tempo total por target (s)
WORKERS="${2:-0}"                  # 0 = auto: CPU/RAM decidem
REPO="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
APK="${REPO}/golden-apks/org.fdroid.fdroid.apk"
LOGDIR="${REPO}/fuzz-logs/$(date +%Y%m%d-%H%M%S)"

if [ "${SEEDS_ONLY}" -eq 0 ]; then
    case "${SECS}" in ''|*[!0-9]*)
        echo "ERRO: SECS deve ser um número inteiro de segundos (ex.: 600, 86400)" >&2; usage; exit 1;; esac
    # issue #65: SECS=0 no libFuzzer (-max_total_time=0) = campanha INFINITA —
    # rejeitar explicitamente (o check de dígitos puro deixava 0 passar)
    if [ "${SECS}" -eq 0 ]; then
        echo "ERRO: SECS=0 significa SEM LIMITE (-max_total_time=0 = infinito no libFuzzer)" >&2
        echo "      Passe os segundos de verdade: ex. 600 (smoke), 86400 (24h)" >&2
        usage
        exit 1
    fi
    case "${WORKERS}" in ''|*[!0-9]*)
        echo "ERRO: WORKERS deve ser um número inteiro (0 = auto)" >&2; usage; exit 1;; esac
fi

# --------------------------------------------------------------- guardas ---
# O script PRECISA rodar de dentro do clone (usa caminhos de crates/ e do APK).
for d in "${REPO}/crates/rd-apk/fuzz" "${REPO}/crates/rd-dex/fuzz" "${REPO}/golden-apks"; do
    if [ ! -d "${d}" ]; then
        echo "ERRO: diretório ausente: ${d}" >&2
        echo "      Rode o script DE DENTRO do clone (não copie para fora):" >&2
        echo "      cd RustyDroid && ./scripts/fuzz-campaign.sh" >&2
        exit 1
    fi
done
if ! command -v unzip >/dev/null; then
    echo "ERRO: instale unzip (sudo apt install unzip)" >&2; exit 1
fi

# Check ASan × kernel (WSL2 recente / kernel ≥ 6.6 com mmap_rnd_bits=32 quebra
# o AddressSanitizer: todos os targets morrem com SEGV em segundos e as 24 h
# virariam lixo). Detecta ANTES de qualquer build.
check_asan_kernel() {
    if [ "${RD_FUZZ_SKIP_SYSCTL_CHECK:-0}" = "1" ]; then return 0; fi
    local rnd
    rnd="$(sysctl -n vm.mmap_rnd_bits 2>/dev/null || true)"
    case "${rnd}" in ''|*[!0-9]*) return 0 ;; esac   # parâmetro inexistente: ok
    if [ "${rnd}" -gt 28 ]; then
        echo "ERRO PRÉ-VOO: vm.mmap_rnd_bits=${rnd} > 28 — o AddressSanitizer vai" >&2
        echo "  morrer com SEGV em segundos (kernel ≥ 6.6 / WSL2 recente). Corrija ANTES:" >&2
        echo "" >&2
        echo "    sudo sysctl -w vm.mmap_rnd_bits=28" >&2
        echo "" >&2
        echo "  Para ficar permanente no WSL2, adicione ao /etc/wsl.conf e rode" >&2
        echo "  'wsl --shutdown' no Windows (reabra o WSL depois):" >&2
        echo "    [boot]" >&2
        echo "    command = sysctl -w vm.mmap_rnd_bits=28" >&2
        echo "" >&2
        echo "  (Para ignorar este check: RD_FUZZ_SKIP_SYSCTL_CHECK=1 $0)" >&2
        exit 1
    fi
}

# ------------------------------------------------------------ requisitos ---
if [ "${SEEDS_ONLY}" -eq 0 ]; then
    if ! command -v cargo >/dev/null; then
        echo "ERRO: cargo não está no PATH — rode: source \"\$HOME/.cargo/env\"" >&2; exit 1
    fi
    if ! rustup run nightly rustc --version >/dev/null 2>&1; then
        echo "ERRO: toolchain nightly ausente — rode: rustup toolchain install nightly --profile minimal" >&2; exit 1
    fi
    if ! cargo fuzz --version >/dev/null 2>&1; then
        echo "ERRO: cargo-fuzz ausente — rode: cargo install cargo-fuzz@0.13.2 --locked" >&2; exit 1
    fi
    # cargo-fuzz exige nightly; força em TODOS os cargos filhos, mesmo que o
    # default do rustup seja stable (não depende de `rustup default nightly`).
    export RUSTUP_TOOLCHAIN=nightly
    check_asan_kernel
fi

# ---------------------------------------------------------------- seeds ----
# Corpus real > corpus vazio: o APK golden alimenta TODOS os targets.
echo "==> preparando seeds"
if [ ! -s "${APK}" ]; then
    echo "    APK golden ausente; baixando via golden-apks/fetch.sh"
    bash "${REPO}/golden-apks/fetch.sh"
fi
# issue #65: assert APK ≤ max_len ANTES de qualquer preparação — APK maior
# seria truncado silenciosamente pelo libFuzzer (o EOCD morre e a campanha
# do apk_full/zip inteira viraria lixo sem ninguém saber).
APK_MAX_LEN=16777216
if [ -s "${APK}" ]; then
    APK_BYTES="$(wc -c < "${APK}")"
    if [ "${APK_BYTES}" -gt "${APK_MAX_LEN}" ]; then
        echo "ERRO PRÉ-VOO: APK golden tem ${APK_BYTES} bytes > max_len ${APK_MAX_LEN}" >&2
        echo "  O libFuzzer truncaria a seed (EOCD do ZIP morto) — apk_full/zip viriam lixo." >&2
        echo "  → use um APK menor no golden-apks/ ou aumente o max_len do apk_full/zip no script." >&2
        exit 1
    fi
fi

TMP="$(mktemp -d)"
# issue #65: trap de SAÍDA (limpeza) + trap de SINAIS — kill do script NÃO
# pode órfar os 5 cargo-fuzz (reproduzido no audit: filho sobreviveu ao pai
# e continuaria 24h). kill 0 alcança o processo-grupo inteiro (script + filhos).
trap 'rm -rf "${TMP}"' EXIT
trap 'trap - INT TERM; kill 0 2>/dev/null || true; exit 130' INT TERM
# -j = caminhos achatados; classes*.dex cobre classes.dex/classes2/3.dex
if ! unzip -o -j -q "${APK}" 'classes*.dex' 'resources.arsc' 'AndroidManifest.xml' -d "${TMP}"; then
    echo "ERRO: falha ao extrair seeds de ${APK} — APK corrompido/incompleto?" >&2
    echo "      Rode: rm -f '${APK}' && bash '${REPO}/golden-apks/fetch.sh'" >&2
    exit 1
fi
# res/*.xml: o '*' do unzip cruza diretórios (pega res/layout/*.xml etc.)
( cd "${TMP}" && unzip -o -q "${APK}" 'res/*.xml' 2>/dev/null ) || true

APK_CORPUS="${REPO}/crates/rd-apk/fuzz/corpus"
DEX_CORPUS="${REPO}/crates/rd-dex/fuzz/corpus"
mkdir -p "${APK_CORPUS}/apk_full" "${APK_CORPUS}/zip" "${APK_CORPUS}/axml" \
         "${APK_CORPUS}/arsc" "${DEX_CORPUS}/dex"
cp "${APK}" "${APK_CORPUS}/apk_full/"                 # APK inteiro (ZIP+AXML+ARSC+DEX)
cp "${APK}" "${APK_CORPUS}/zip/"                      # container ZIP real
cp "${TMP}/AndroidManifest.xml" "${APK_CORPUS}/axml/manifest.xml"
cp "${TMP}/resources.arsc" "${APK_CORPUS}/arsc/"
cp "${TMP}"/classes*.dex "${DEX_CORPUS}/dex/"

# NÃO usar `find | head | while` direto: com pipefail, o SIGPIPE do find
# (exit 141) mataria o script no APK real (703 res). Escrever em arquivo e
# consumir depois — `|| true` cobre res/ inexistente e o truncamento do head.
# ATENÇÃO: -size com unidade M/K arredonda PRA CIMA — `-size -1M` só casa
# arquivo VAZIO. Para "< 1 MiB" usar bytes exatos: -size -1048576c.
if [ -d "${TMP}/res" ]; then
    find "${TMP}/res" -name '*.xml' -size -1048576c 2>/dev/null \
        | head -50 > "${TMP}/axml-seeds.txt" || true
    while IFS= read -r f; do
        # shellcheck disable=SC2015  # cp falho em seed individual não aborta
        [ -n "${f}" ] && cp "${f}" "${APK_CORPUS}/axml/" || true
    done < "${TMP}/axml-seeds.txt"
fi

echo "    seeds ok: apk_full=$(ls "${APK_CORPUS}/apk_full" | wc -l) zip=$(ls "${APK_CORPUS}/zip" | wc -l) axml=$(ls "${APK_CORPUS}/axml" | wc -l) arsc=$(ls "${APK_CORPUS}/arsc" | wc -l) dex=$(ls "${DEX_CORPUS}/dex" | wc -l)"

if [ "${SEEDS_ONLY}" -eq 1 ]; then
    echo
    echo "==> seeds-only: corpus pronto (nenhum fuzzer executado)"
    du -sh "${APK_CORPUS}"/* "${DEX_CORPUS}"/* 2>/dev/null | sed 's/^/    /' || true
    echo
    echo "    Próximo passo — smoke de 10 min ANTES das 24 h:"
    echo "      ./scripts/fuzz-campaign.sh 600"
    exit 0
fi

# ---------------------------------------------------- plano de memória -----
# 5 targets em paralelo, cada um com WORKERS filhos libFuzzer. Pior caso de
# RSS ≈ (WORKERS × 5 × rss_limit) + SO. Calibrar pela RAM REAL disponível
# evita o OOM-killer matar a campanha no meio (notebooks com 16 GB).
AVAIL_MB="$(awk '/MemAvailable/ {print $2; exit}' /proc/meminfo 2>/dev/null || echo 0)"
AVAIL_MB=$(( ${AVAIL_MB:-0} / 1024 ))
if [ "${AVAIL_MB}" -le 0 ]; then AVAIL_MB=8192; fi      # leitura falhou: presumir 8 GB

if [ "${WORKERS}" -eq 0 ]; then
    CORES="$(nproc)"
    WORKERS=$(( CORES / 5 ))
    if [ "${WORKERS}" -lt 1 ]; then WORKERS=1; fi
fi

RSS="${RD_FUZZ_RSS:-0}"                                  # 0 = auto
if [ "${RSS}" = "0" ]; then
    BUDGET=$(( AVAIL_MB - 2500 ))                        # SO do WSL + 5 pais + margem
    PER_PROC=$(( BUDGET / (WORKERS * 5) ))
    # Pouco RSS por processo gera oom-* falso. Se ficou apertado, trocar
    # throughput por estabilidade: 1 worker por target com RSS maior.
    if [ "${PER_PROC}" -lt 1536 ] && [ "${WORKERS}" -gt 1 ] && [ "${BUDGET}" -ge 7680 ]; then
        echo "    (auto) workers ${WORKERS} → 1 por target: prioriza RSS maior por processo"
        WORKERS=1
        PER_PROC=$(( BUDGET / 5 ))
    fi
    if [ "${PER_PROC}" -gt 2048 ]; then PER_PROC=2048; fi
    if [ "${PER_PROC}" -lt 512 ]; then
        echo "ERRO: RAM disponível (${AVAIL_MB} MB) insuficiente para 5 targets × ${WORKERS} workers." >&2
        echo "      → feche aplicativos; ou aumente [wsl2] memory= no .wslconfig;" >&2
        echo "      → ou rode com menos workers: ./scripts/fuzz-campaign.sh ${SECS} 1" >&2
        exit 1
    fi
    RSS=$(( PER_PROC / 256 * 256 ))
fi

# -------------------------------------------------------------- setup ------
mkdir -p "${LOGDIR}"
# Proveniência da campanha (vai junto no relatório do DoD)
{
    date -u +"%Y-%m-%dT%H:%M:%SZ"
    echo "repo: ${REPO}"
    echo "duracao_por_target_s: ${SECS}  workers_por_target: ${WORKERS}  rss_mb: ${RSS}  cores: $(nproc)  ram_disponivel_mb: ${AVAIL_MB}"
    rustc --version 2>/dev/null || true
    cargo fuzz --version 2>/dev/null || true
    uname -r
} > "${LOGDIR}/env.txt" 2>/dev/null || true

echo "==> campanha: 5 targets × ${SECS}s × ${WORKERS} workers/target — logs em ${LOGDIR}"
echo "    plano de memória: rss/malloc ${RSS} MB por processo · RAM disponível ${AVAIL_MB} MB"
echo "    (corpus cresce em crates/*/fuzz/corpus; artifacts em crates/*/fuzz/artifacts)"
echo "    progresso em tempo real: tail -f ${LOGDIR}/done.txt"

# Guarda de disco: campanha longa + corpus crescendo exige folga. Aviso (não
# aborta) abaixo de 5 GB livres no filesystem do repo.
FREE_KB="$(df -Pk "${REPO}" | awk 'NR==2 {print $4}' || echo 0)"
if [ "${FREE_KB}" -lt 5242880 ]; then
    echo "AVISO: só $(( FREE_KB / 1048576 )) GB livres em ${REPO} — o corpus pode encher o disco em 24h."
    echo "       Considere liberar espaço; resgate com 'cargo fuzz merge' (veja cabeçalho deste script)."
fi

# Arquivar artifacts de rodadas ANTERIORES: sem isso, um crash-*/oom-* antigo
# (ex.: do smoke de 10 min) seria contado de novo e contaminaria o veredito.
# Nada se perde: vai para <LOGDIR>/artifacts-anteriores/.
for c in rd-apk rd-dex; do
    A="${REPO}/crates/${c}/fuzz/artifacts"
    if [ -d "${A}" ] && [ -n "$(ls -A "${A}" 2>/dev/null)" ]; then
        DEST="${LOGDIR}/artifacts-anteriores/${c}"
        mkdir -p "${DEST}"
        mv "${A}"/* "${DEST}/" 2>/dev/null || true
        echo "    artifacts de rodadas anteriores arquivados em ${DEST}"
    fi
done

# ------------------------------------------------------------ execução ----
# max_len ≥ maior seed (senão o libFuzzer TRUNCA a seed — mata o EOCD do ZIP):
#   apk_full/zip: 16 MB · dex: 10 MB · arsc: 4 MB · axml: 2 MB
# (assert APK ≤ max_len no pré-voo, issue #65)
pids=()
names=()
run_target() { # crate target max_len
    local crate="$1" target="$2" max_len="$3"
    mkdir -p "${REPO}/crates/${crate}/fuzz/artifacts"
    (
        cd "${REPO}/crates/${crate}" && cargo fuzz run --release "${target}" -- \
            -max_total_time="${SECS}" \
            -fork="${WORKERS}" \
            -ignore_crashes=1 -ignore_timeouts=1 -ignore_ooms=1 \
            -rss_limit_mb="${RSS}" -malloc_limit_mb="${RSS}" \
            -timeout=25 \
            -max_len="${max_len}" \
            -print_final_stats=1
    ) > "${LOGDIR}/${crate}-${target}.log" 2>&1
    echo "[$(date +%H:%M:%S)] concluído: ${crate}/${target}" >> "${LOGDIR}/done.txt"
}
for spec in "rd-apk apk_full 16777216" "rd-apk zip 16777216" "rd-apk axml 2097152" \
            "rd-apk arsc 4194304" "rd-dex dex 10485760"; do
    # shellcheck disable=SC2086
    set -- ${spec}
    run_target "$1" "$2" "$3" &
    pids+=("$!")
    names+=("${1}/${2}")
    sleep 2   # espaçar builds iniciais (o cargo serializa o lock, sem erro)
done
FAILED=()
RCS=()
for i in "${!pids[@]}"; do
    set +e
    wait "${pids[$i]}"
    rc=$?
    set -e
    RCS+=("${rc}")
    if [ "${rc}" -ne 0 ]; then
        FAILED+=("${names[$i]}")
        echo "[$(date +%H:%M:%S)] saída ${rc}: ${names[$i]} — ver ${LOGDIR}/${names[$i]//\//-}.log"
    fi
done

# ------------------------------------------------------------- relatório ---
echo; echo "================ RELATÓRIO DA CAMPANHA ================"
echo "Logs:            ${LOGDIR}"
echo "Duração/target:  ${SECS}s × ${WORKERS} workers · rss ${RSS} MB"
if [ -f "${LOGDIR}/done.txt" ]; then
    echo "Conclusões em ordem real:"
    sed 's/^/  /' "${LOGDIR}/done.txt"
fi
for i in "${!names[@]}"; do
    f="${LOGDIR}/${names[$i]//\//-}.log"
    [ -e "${f}" ] || continue
    # fork mode não imprime o bloco stat::; o total executado está no último
    # pulso "#N:"; fallback: exec/s do pulso; fallback: n/a
    execs="$(grep -oE '^#[0-9]+:' "${f}" | tail -1 | grep -oE '[0-9]+' || true)"
    if [ -z "${execs}" ]; then
        execs="$(grep -oE 'stat::number_of_executed_units: [0-9]+' "${f}" | tail -1 | grep -oE '[0-9]+$' || true)"
    fi
    if [ -z "${execs}" ]; then
        execs="$(grep -oE 'exec/s: [0-9]+' "${f}" | tail -1 | grep -oE '[0-9]+$' || true)"
    fi
    [ -n "${execs}" ] || execs='n/a (ver log)'
    printf '%-28s %s unidades (exit %s)\n' "$(basename "${f}" .log):" "${execs}" "${RCS[$i]:-?}"
done
echo

# issue #58: o veredito é por ARTIFACTS, não pelo exit code — no modo fork do
# libFuzzer o exit é o do ÚLTIMO JOB (FuzzerFork.cpp): 70=timeout/71=oom/
# 77=achado descartado podem sair de uma campanha que consumiu os SECS
# íntegros. E leak-* (LSan, filho sai 77) nunca era contado.
count_artifacts() { # <pattern> — com GUARD: find falho ≠ contagem 0 silenciosa
    local n
    if ! n="$(find "${REPO}/crates" -type f -name "$1" -path '*/fuzz/artifacts/*' 2>/dev/null | wc -l)"; then
        echo "ERRO: falha ao contar artifacts ($1) — veredito indefinido" >&2
        exit 2
    fi
    printf '%s' "${n}"
}
CRASH="$(count_artifacts 'crash-*')"
LEAK="$(count_artifacts 'leak-*')"
OOM="$(count_artifacts 'oom-*')"
TMO="$(count_artifacts 'timeout-*')"
echo "crashes: ${CRASH} · leaks: ${LEAK} · oom: ${OOM} · timeouts: ${TMO} · targets com exit≠0: ${#FAILED[@]}"
echo
echo "Uso de disco do corpus (cresceu durante a campanha):"
du -sh "${REPO}/crates/rd-apk/fuzz/corpus"/* "${REPO}/crates/rd-dex/fuzz/corpus"/* 2>/dev/null | sed 's/^/  /' || true

# ── veredito (issue #58: crash/leak PRIMEIRO — P0 nunca subnotificado) ─────
if [ "${CRASH}" -gt 0 ] || [ "${LEAK}" -gt 0 ]; then
    echo; echo "RESULTADO: ❌ ${CRASH} crash-* + ${LEAK} leak-* — bug P0 (Lei nº 1):"
    find "${REPO}/crates" -type f \( -name 'crash-*' -o -name 'leak-*' \) -path '*/fuzz/artifacts/*' 2>/dev/null | sed 's/^/  /'
    echo "Reproduzir:   (cd crates/<crate> && cargo fuzz run <target> -- <arquivo-do-artifact>)"
    echo "Minimizar:    (cd crates/<crate> && cargo fuzz run <target> -- -minimize_crash=1 -runs=10000 <arquivo>)"
    exit 1
fi

# falha GENUINA = exit≠0 SEM evidência de progresso (build morto/execução
# nunca começou). Exit 70/71/77 do fork parent COM pulsos de progresso =
# campanha consumiu os SECS (último job terminou nesses estados) — não é
# fracasso (issue #58: antes vira exit 2 falso).
GENUINE_FAILED=()
for i in "${!names[@]}"; do
    [ "${RCS[$i]:-0}" -eq 0 ] || {
        f="${LOGDIR}/${names[$i]//\//-}.log"
        if grep -qE '^#[0-9]+:' "${f}" 2>/dev/null; then
            continue  # rodou (pulsos de execução) — estado do último job é aviso
        fi
        GENUINE_FAILED+=("${names[$i]}")
    }
done
if [ "${#GENUINE_FAILED[@]}" -gt 0 ]; then
    echo; echo "RESULTADO: ❌ ${#GENUINE_FAILED[@]} target(s) NÃO executaram (build/execução): ${GENUINE_FAILED[*]}"
    echo "    SEM os 5 targets íntegros NÃO há como atestar 'fuzz 24h sem crash'."
    for n in "${GENUINE_FAILED[@]}"; do
        f="${LOGDIR}/${n//\//-}.log"
        echo "--- ${f} (últimas 15 linhas)"
        tail -15 "${f}" 2>/dev/null || true
    done
    echo "    Corrija (veja o log) e rode de novo — o corpus acumulado é preservado."
    exit 2
fi

if [ "${CRASH}" -eq 0 ] && [ "${LEAK}" -eq 0 ]; then
    echo "RESULTADO: ✅ SEM CRASH/LEAK e 5/5 targets rodaram — DoD do M1 ('fuzz 24h sem crash') satisfeito"
    if [ "${OOM}" -gt 0 ] || [ "${TMO}" -gt 0 ]; then
        echo "           (⚠ ${OOM} oom + ${TMO} timeout arquivados — investigar depois; não são crash)"
    fi
    for i in "${!names[@]}"; do
        case "${RCS[$i]:-0}" in
            70|71|77)
                echo "           (⚠ exit ${RCS[$i]} em ${names[$i]} = estado do ÚLTIMO job do fork parent; a campanha consumiu os ${SECS}s — issue #58)" ;;
        esac
    done
    # issue #65: honestidade — o tempo é wall-clock
    echo "           (nota: SECS é wall-clock — suspend do notebook conta como tempo de campanha)"
    exit 0
fi
