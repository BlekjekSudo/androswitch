# ADR 014 — LazyArsc + LazyStringPool: decodificação sob demanda de resources.arsc

- **Status:** proposto (aceito quando o dono mergear — precedente ADR 008); implementa o pilar "Resources" do ADR 008 (LazyStringPool + LazyArsc com resolução O(1) filtrada por config)
- **Data:** 2026-10-25 · **Escopo:** `crates/rd-apk/src/pool.rs`, `crates/rd-apk/src/arsc.rs` (+ testes/fuzz desses arquivos); **sem** tocar `zip.rs`/`paged.rs`/`lib.rs` (frente PagedZip do ArenaAgent — complemento, não sobreposição)
- **Relacionados:** ADR 008 (orçamentos moto-e5, causa-raiz #4 "Eager ARSC"), ADR 003 (detecção não valida payload), Lei 1 (erros estruturados), Lei 4 (determinismo)
- **Baseline medida (2026-10-25, release build, benchmark isolado `Arsc::parse`):** `torture-huge-arsc.apk` do APK-Tester v1.0.2 — ARSC de 40.930.960 B → **parse eager 137,95 ms**, **410.014 strings** decodificadas, 10.011 entradas materializadas, pico VmHWM do processo de bench 383.232 KB. No fator moto-e5 (0.25), esses 138 ms de host equivalem a ~550 ms emulados — dentro do orçamento de cold start inteiro do ADR 008. APKs típicos: ARSC < 60 KB, parse < 0,2 ms (o problema é a classe flagship/torture).

## Contexto

`StringPool::parse` decodifica TODAS as strings no momento do parse (global +
type + key pools; o mesmo vale para os pools do AXML), e `parse_package`
materializa TODAS as entradas (`BTreeMap<u32, Vec<ResEntry>>`) com bags completos
e strings pré-resolvidas (`string: Option<String>` + `ResMapEntry::string` por
item de bag). O cold path do ADR 008 toca <2–5% disso: o framework resolve
poucos resids (label do launcher, tema, dimens/strings do layout inicial).
Consumidores externos de `Arsc` usam APENAS os métodos `resolve_string`,
`resolve_name`, `resolve_value` (rd-vm: android_res.rs, framework.rs, engine.rs,
inflate.rs) e `Package::resolve_name` — ninguém acessa `entries`/pools
diretamente, e `StringPool::iter()` não tem consumidores fora do próprio pool.
`Arsc::parse` recebe `&[u8]` e devolve estrutura owned (sem autoreferência).

## Decisão

**PR A — `LazyStringPool` (pool.rs), API-compatível:**

1. Estrutura: `{ chunk: Vec<u8> (cópia dos bytes crus do chunk), offsets: Vec<u32> (tabela u32 — barata, parseada eager), strings_base: usize, utf8: bool, cache: Vec<OnceLock<String>> }`. `parse()` valida header/limites como hoje (mesmos erros estruturados), lê a tabela de offsets, NÃO decodifica strings.
2. `get(&self, idx) -> Option<&str>` via `OnceLock::get_or_init` — decodifica UMA string sob demanda, memoiza; `&self` seguro, zero `unsafe`. `len()`/`is_empty()` O(1) (string_count). `iter()` decodifica sob demanda (sem consumidores externos hoje; mantido por compatibilidade). `Clone` manual: cache renasce vazio (mesmo conteúdo, decodificação de novo por instância — determinístico).
3. Custo de memória pior caso = eager de hoje (todas as strings tocadas); estritamente menor quando subconjunto é tocado. Os bytes crus (chunk) ficam retidos — Lei 2 (offsets crus preservados) reforçada.
4. Benefício colateral: os pools do AXML (manifest) usam o mesmo tipo e ficam lazy de graça.

**PR B — `LazyArsc` (arsc.rs), índice em vez de materialização:**

5. `Package` troca `entries: BTreeMap<u32, Vec<ResEntry>>` por índice leve: `BTreeMap<u32, Vec<TypeChunkRef>>` onde `TypeChunkRef = { chunk_off, chunk_size, header_size, config: ResConfigInfo, entry_count, flags, entries_start }` — configs são minúsculas e parseadas eager (seleção de melhor config continua correta); entradas pesadas NÃO.
6. `resolve_string/resolve_name/resolve_value` (e `Package::resolve_name`) materializam SOB DEMANDA as `ResEntry` dos chunks daquele resid (mesma lógica de parse de hoje, extraída para função por-entry), selecionam a melhor config como hoje, e memoizam num cache LRU **limitado (cap 8.192 resids)** por `Arsc`. Cache só muda QUANDO decodifica, nunca O QUÊ retorna — Lei 4 preservada (mesma query → mesmo resultado, byte-a-byte).
7. Seleção de melhor config inalterada (specificity/is_default); cadeias de referência (limite 8 saltos) inalteradas; bags complexos só decodificam quando o resid complexo é resolvido.
8. Erros idênticos: toda validação de bounds/formato que hoje rejeita o parse continua rejeitando no MESMO ponto semântico (erros de string individual podem surgir no acesso em vez do parse — mesma classe `RdError` estruturado, Lei 1; documentado na evidência).

## Gates (ambos os PRs)

- **Paridade golden:** `resolve_string/resolve_name/resolve_value` byte-idênticos ao eager: fixtures de unidade existentes + os 56 APKs da suíte APK-Tester v1.0.2 (incluindo os 11 de torture: truncated axml, arsc collision, zip64, unaligned…) + suítes golden do repo. Ferramenta: binário de paridade rodando eager-vs-lazy e diffando saída (evidência anexada ao PR).
- **Perf (números antes/depois no PR, medidos, não estimados):** `Arsc::parse` no `torture-huge-arsc`: 137,95 ms → meta **< 15 ms** (só índice+offsets+configs); primeira `resolve_string` < custo eager total; VmHWM do decode limitado ao subconjunto tocado (número medido relatado); cold do app trivial SEM regressão (gate `perf_cli` existente continua verde).
- **Fuzz:** targets existentes de arsc/axml continuam verdes + novos casos "malformado sob demanda" (offset apontando para string truncada no índice N: erro estruturado idêntico ao eager quando N é tocado).
- **moto-e5:** o parse do ARSC conta no cold budget do ADR 008; ~550 ms emulados reclamados na classe huge-arsc. MSRV 1.76, `unsafe_code="forbid"`, zero dependências novas.

## Consequências

- Inspect/launch em APKs da classe flagship/torture param de pagar 410k decodificações + materialização total no cold path; APKs típicos ficam ~iguais (ARSC minúsculo) — "nunca pior que hoje, estritamente melhor quando subconjunto tocado".
- `ResEntry`/bags deixam de ser campos pub observáveis (ninguém externo os usa — auditado); `Package.entries` some da API pública (breaking interna documentada no changelog do crate 0.x).
- Bytes crus retidos por pool ≈ tamanho do chunk (já inflado em RAM hoje via ZIP) — sem novo pico; o pico de DECODE some.
- Trabalho futuro natural (fora deste ADR): paged I/O do ARSC direto do ZIP (ArenaAgent, zip/paged/lib) — LazyArsc é o lado decode desse pilar.

## Alternativas rejeitadas

- **Manter eager e só cachear:** não resolve cold path nem RSS (decode acontece sempre no parse).
- **mmap/posicional no ARSC inteiro:** é a frente PagedZip do ArenaAgent (zip/paged/lib) — sobreposição de arquivos; este ADR fica no decode.
- **Cache ilimitado de resids:** pico de RSS volta a crescer com APK hostil; cap LRU 8.192 limita o pico declarado.
- **Trocar a API pública (`resolve_*`) por iterator materializado:** quebraria os 4 consumidores do rd-VM sem ganho; mantida a API atual.
