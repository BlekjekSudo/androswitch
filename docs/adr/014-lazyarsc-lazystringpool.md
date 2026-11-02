# ADR 014 — LazyArsc + LazyStringPool: decodificação sob demanda de resources.arsc

- **Status:** proposto (aceito quando o dono mergear — precedente ADR 008); implementa o pilar "Resources" do ADR 008 (LazyStringPool + LazyArsc com resolução por índice)
- **Data:** 2026-10-25 · **Escopo:** `crates/rd-apk/src/pool.rs`, `crates/rd-apk/src/arsc.rs` (+ adaptações mecânicas de 1-3 linhas em `lib.rs`, `rd-vm/src/android_res.rs`, `rd-vm/tests/app_xml.rs` e 2 examples do rd-apk, todas forçadas pela troca de API e divulgadas no PR); **sem** tocar `zip.rs`/`paged.rs` (frente PagedZip do ArenaAgent — complemento, não sobreposição)
- **Relacionados:** ADR 008 (orçamentos moto-e5, causa-raiz #4 "Eager ARSC"), Lei 1 (erros estruturados), Lei 2 (offsets crus), Lei 4 (determinismo)
- **Medido (2026-10-25, release build, bench isolado `Arsc::parse`):** `torture-huge-arsc.apk` do APK-Tester v1.0.2 — ARSC de 40.930.960 B, 410.014 strings, ~400k pares (resid, config):
  - **Parse: 137,95 ms → 32,34 ms (4,3×)**; no fator moto-e5 (0.25): ~550 ms → ~130 ms emulados devolvidos ao orçamento de cold do ADR 008.
  - **VmHWM do processo de bench: 383 MB → 137 MB (−64%)** (inclui leitura whole-file do ZIP — frente PagedZip; o delta é do decode).
  - APKs típicos (engines/ladder): parse sub-ms antes e depois (ARSCs pequenos — "nunca pior que o eager").
  - **Paridade eager-vs-lazy: 56/56 APKs da suíte APK-Tester v1.0.2 (32 engines + escada + torture + flagship-classic de 1.851 resids) — 36.771 checagens de `resolve_string`/`resolve_name`/`resolve_value` sobre a união dos resids, 0 divergências.** Única diferença de índice (documentada): 1 resid de entrada truncada no torture-huge-arsc que o eager nem inseria e o lazy indexa — ambos resolvem `None` (resultado idêntico).

## Contexto

`StringPool::parse` decodificava TODAS as strings no parse (global + type + key), e
`parse_package` materializava TODAS as entradas (`BTreeMap<u32, Vec<ResEntry>>`)
com bags completos e strings pré-resolvidas. O cold path do ADR 008 toca <2–5%
disso. Consumidores externos de `Arsc` usam APENAS `resolve_*` (+ iteração de
resids p/ `getIdentifier` e ferramentas de diagnóstico — cobertos por APIs novas).

## Decisão (como implementado)

1. **`LazyStringPool` (pool.rs):** o documento inteiro vive atrás de um
   `Arc<Vec<u8>>`; cada pool é uma REGIÃO lógica dele (`parse_shared` — zero
   cópias por pool no caminho ARSC; `parse` standalone do AXML faz 1 cópia do
   documento — manifestos são pequenos). Header/contagens validados como hoje
   (erros `RdError` idênticos); **validação de offsets é O(1)** (a tabela cabe
   no documento) — offsets individuais ruins são rejeitados **no acesso**
   (`get → None`, falha memoizada): o eager falhava o parse inteiro na
   primeira string ruim; o lazy isola a string ruim (Lei 1 no nível
   estrutural). **Port do `main` (#331/#246):** toda leitura de string fica
   confinada à fatia do CHUNK do pool (`[chunk_off..chunk_off+chunkSize]`, nunca
   cruza para o chunk vizinho); tamanho UTF-16 é checado em BYTES antes de
   qualquer alocação; pools de pacote têm o limite no FIM DO PACOTE (não no
   container); decodificação total é orçada em `MAX_POOL_DECODED_BYTES`
   (128 MiB por pool, cobrada NO ACESSO — estourou → `None`).
   `get(&self) -> Option<&str>` via `OnceLock` por slot
   (decodifica 1×, memoiza; `None` interno = malformada) com **alocação
   preguiçosa do vetor de slots** (pool nunca tocada não paga os 410k `OnceLock`).
   `len`/`is_empty` O(1); `iter` sob demanda; `Clone` compartilha o `Arc`
   (cache fresco). AXML ganha lazy de graça (mesmo tipo).

2. **`LazyArsc` (arsc.rs):** `Arsc::parse_owned(Vec<u8>)` (caminho do
   `Apk::open` — zero cópias extras; `parse(&[u8])` mantido) retem o buffer
   **inteiro** num único `Arc` (Lei 2; clones de `Arsc`/`Package` O(1) —
   endereçamento por offsets absolutos idênticos aos do eager, garantia de
   paridade por construção). Por pacote, o walk de chunks é o MESMO do eager
   (mesmos guards/breaks); em vez de materializar, constrói:
   - `chunks: Vec<TypeChunkRef>` — referência leve por chunk de tipo
     (offsets absolutos + config parseada eager — configs são minúsculas);
   - `pairs: Vec<(resid, chunk_idx, slot)>` — **append O(1) durante o walk + um
     `sort_unstable` no fim** (400k pares do torture: ~15 ms; BTreeMap com
     clones por inserção custava ~60 ms). `slot` é o índice da tabela de onde o
     offset é lido na materialização (sparse: um par por slot, na ordem do
     walk; dense/offset16: um par por índice). A ordem (resid, chunk_idx, slot)
     reproduz a ordem de walk do eager — decisiva para o tie-breaking do
     `min_by_key` (Lei 4).
   - Walk e janelas idênticos ao eager (`main`): tabela no intervalo
     `[headerSize, entriesStart)` do próprio chunk; `entryCount` além da janela
     não lê vizinhos; `chunk_end` é o limite de todo guard de entrada.
3. **Materialização sob demanda:** `entries_for(resid)` (pública —
   diagnóstico/testes) faz busca binária nos `pairs`, materializa as entradas
   dos slots daquele resid com a MESMA lógica do `collect` eager do `main`
   (`materialize_slot`: mesmos endereços, mesmos guards de truncamento, mesma
   pré-resolução de strings pela pool global). **Orçamento de materialização
   movido do parse para o acesso:** `ArscBudget` (atômico, `Arc` compartilhado
   por todos os pacotes do `Arsc`) cobra 1 entrada/item de bag + os bytes de
   cada string clonada contra `MAX_TOTAL_ARSC_ENTRIES` (4.000.000) e
   `MAX_TOTAL_ARSC_STRING_BYTES` (128 MiB). Ao estourar, as entradas restantes
   não são materializadas. Resid sem entradas materializadas = `None` (igual ao
   eager, que não empilhava). Resultados são memoizados num cache `Mutex<BTreeMap>` **cap
   8.192 resids com reset ao estourar** (bounded; muda QUANDO decodificar,
   nunca O QUÊ retornar). `resolve_string/name/value/style_attribute`
   inalterados (trocam só a origem das entradas).
4. **APIs novas (migração honesta):** `Package::resource_ids()` (resids
   ordenados — usado por `getIdentifier` no rd-vm), `Package::entries_for`
   (snapshot por resid), `Package::entries_snapshot()` (materialização total —
   ferramentas de diagnóstico; custo ≈ eager). `Package::synthetic` para
   testes que construíam o campo `entries` direto. O campo `entries` deixou
   de existir (auditado: sem consumidores externos além dos adaptados).

## Gates (executados)

- **Paridade:** harness eager-vs-lazy (eager vendorizado da main 87a4145 como
  referência) — números no cabeçalho; evidência reproduzível no PR.
- **Testes:** rd-apk 48/48 lib (novos: lazy memoiza/clone/empty-string/malformed→
  None/índice-resolve; port do `main`: `issue_331_*` — aliasing/orçamento no acesso,
  UTF-16 cruzando chunk → None, offset inválido near usize::MAX → erro tipado;
  `issue_246_*` — tabela/entradas não cruzam chunk, pools do pacote não leem além
  do pacote); rd-vm lib 132/132 + app_xml 25/25 + app 10/10 + m8
  13/13 + m9 8/8 + vm_exec 103/103; rd-framework 34/34; rd-agent 84/84;
  rd-cli perf_cli 3/3 (gate de perf) + app/inspect/engine/test/mcp_cli verdes.
- **Qualidade:** `cargo fmt` limpo; `clippy --workspace --all-targets
  -D warnings` limpo; MSRV 1.76 (zero dependências novas — std only);
  `unsafe_code = "forbid"` intacto.
- **moto-e5:** os ~420 ms emulados devolvidos no huge-arsc-class contam no
  cold budget do ADR 008; a medição do binário DEBUG do ArenaAgent
  (screenshot frio ~110 ms dominando no app trivial) não é afetada (ARSC
  minúsculo no trivial).

## Mudança de semântica de erro (Lei 1) — explícita

Com o eager, UMA string malformada em qualquer pool reprovava o **parse
inteiro** do resources.arsc (`PARSE_ERROR` estruturado). Com o lazy, o mesmo
input: parse **sucede** (header/tabela/estrutura válidos) e a string ruim
aparece como **`None` no acesso** (`get`/resolve sobre aquele índice — falha
memoizada por slot). Em resumo:

| Cenário | Eager (antes) | Lazy (agora) |
|---|---|---|
| Header/contagens/tabela de offsets fora do documento | erro no parse | erro no parse (inalterado) |
| String individual truncada/corrompida | erro no parse | `None` no acesso |
| Entrada individual truncada | entrada pulada (parse ok) | entrada pulada (idêntico) |
| UTF-16 cujo comprimento cruza o fim do chunk do pool | erro no parse (`PARSE_ERROR`) | `None` no acesso (string) |
| Decodificação total de strings acima de 128 MiB por pool | erro no parse (`budget`) | strings seguintes `None` no acesso |
| Entradas + bytes de strings acima de `MAX_TOTAL_ARSC_*` | erro no parse (`budget`) | entradas seguintes não materializadas (resolve → `None` / parcial) |

Conhecido e aceito: o orçamento de materialização é cobrado de novo quando o
cache de resids é reconstruído (cap 8.192). Um APK legítimo que varre mais
resids que o cap pode gastar o orçamento em re-decodificação; o caso
normal (resolve de um subconjunto) fica bem abaixo dos 4 M de entradas.
Pools/APKs **válidos** têm comportamento idêntico (paridade medida: 0
divergências em 36.771 checagens). A diferença só existe para input
malformado — e o novo fuzz target (`arsc.rs` chama `resolve_*` sobre todos os
resids) garante que esse caminho nunca entra em pânico. Ferramentas que
precisam distinguir "string ausente" de "string corrompida" devem inspecionar
o slot via `get` (o `None` é memoizado e determinístico).

## Consequências

- Inspect/launch na classe flagship/torture param de pagar 410k decodificações
  + materialização total no cold path; APKs típicos ficam ~iguais ("nunca
  pior que o eager, estritamente melhor quando subconjunto tocado").
- Retenção de memória: buffer cru inteiro (40 MB no torture) + índice
  (~3,2 MB de pares) + touched-cache — troca o pico de DECODE (Strings +
  bags materializados) por bytes crus: melhor no caso medido (−64% VmHWM) e
  nos APKs reais (overhead por `String` 24 B × strings curtas).
- Erros de string individual mudam de "parse falha" para "get → None"
  (documentado; pools válidos idênticos). Malformados SOB DEMANDA exigem os
  fuzz targets existentes continuarem verdes (parsers nunca confiam).
- `Package.entries` (campo pub) deixa de existir; migração pelas APIs novas
  (adaptados: android_res.rs, app_xml.rs, 2 examples, lib.rs 1 linha).
- Trabalho futuro natural (fora daqui): paged I/O do ARSC direto do ZIP
  (frente PagedZip do ArenaAgent) — LazyArsc é o lado decode desse pilar;
  a cópia do buffer inteiro (o ~10 ms restante de memcpy) some com ela.

## Alternativas rejeitadas

- **Manter eager e só cachear:** não resolve cold path nem RSS.
- **BTreeMap<resid, Vec<ref>> no índice:** medido ~60 ms no torture (clones +
  inserções por árvore); pares ordenados + busca binária fazem o mesmo em ~15 ms.
- **Validação per-offset no parse:** medido ~12 ms no torture; rejeitada pela
  validação O(1) + rejeição no acesso (paridade de resolve mantida — evidência).
- **mmap/posicional no ARSC:** frente PagedZip (zip/paged) — sobreposição de
  arquivos; este ADR fica no decode.
- **Cache ilimitado de resids:** pico de RSS voltaria a crescer com APK hostil.
