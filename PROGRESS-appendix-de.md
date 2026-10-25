# PROGRESS — Apêndices D + E (branch `appendix-d-e-20261025`)

> Arquivo de retomada. O ambiente reseta: **leia isto primeiro**, depois
> `git log --oneline main..HEAD` e continue do primeiro item `[ ]`.
> Salvar = commit nesta branch a cada etapa + atualizar este arquivo no
> mesmo commit. Push: sem credencial no sandbox (https sem token) — tentar
> `git push -u origin appendix-d-e-20261025` quando houver token; até lá os
> commits locais + snapshot do workspace são a persistência.

## Setup (repetir após reset)

```sh
git clone https://github.com/nadabgfccvd/RustyDroid.git  # se sumiu
cd RustyDroid && git checkout appendix-d-e-20261025       # se a branch existir
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
export PATH="$HOME/.cargo/bin:$PATH"   # rustc 1.99.1 em 2026-10-25
# RAM 1.9 GiB / 2 cores: SEMPRE builds com CARGO_BUILD_JOBS=1 (debug, sem LTO)
```

## Regras invioláveis desta tarefa

- **PRs #191/#192 (outro agente, Apêndice C): NÃO TOCAR.** Arquivos quentes
  deles (evitar; se inevitável, edição mínima aditiva + nota no PR):
  `crates/rd-vm/src/classpath.rs`, `intrinsics.rs`, `android_util.rs`,
  `android_drawable.rs`, `android_json.rs`, `jcollections.rs`, `jlstring.rs`,
  `parcel.rs`, `bundle.rs`, `graphics.rs`, `jfile.rs`, `jtime.rs`,
  `android_res.rs`, `choreographer.rs`, `numerics.rs`, `view_props.rs`,
  `m8.rs` (+43), `crates/rd-framework/src/fd_api_coverage.rs` (+9),
  `data/api-coverage.toml`, `docs/api-coverage.md`,
  `docs/appendix-c-literal-completion.md`, `crates/rd-agent/src/mcp.rs`
  (só 1 teste ~L3847), `crates/rd-vm/src/lib.rs` (+2 mods após
  `android_spannable` — inserir os nossos longe dali), tabela do Apêndice C
  em `RustyDroid.md`, `docs/compat-matrix.md` (gerada; regenerar só no fim).
- Arquivos seguros (PRs não tocam): `interp.rs`, `framework.rs`, `engine.rs`,
  `gc.rs`, `heap.rs`, `inflate.rs`, `xtask/src/main.rs`,
  `crates/rd-cli/src/main.rs`, `crates/rd-framework/src/lib.rs`, arquivos NOVOS.
- Lei 1 do projeto: status honesto. `supported` = subconjunto documentado
  com modelo host + teste; nunca a biblioteca/serviço inteiro. Sem modelo =
  `stub`/`not_implemented` + `gap_id`, nunca sucesso fingido.
- Base: `main` @ `89bbe8f` (merge PR #190).

## Desenho (espelha o Apêndice C, end-to-end)

1. Dados: `data/jetpack.toml` (Ap. D, por módulo) + `data/gms.toml` (Ap. E,
   modo sem-GMS honesto) — fonte executável da verdade.
2. `rd-framework`: `fd_jetpack.rs` + `fd_gms.rs` (parse/validate/classify
   longest-prefix/resumos/relatórios), como `fd_api_coverage.rs`.
3. CLI: `rd jetpack list|show|check|report`, `rd gms list|show|check|report`.
4. `xtask`: `jetpack-report|jetpack-spec|gms-report|gms-spec` →
   `docs/jetpack.md`, `docs/gms.md`, tabelas D/E em `RustyDroid.md`.
5. Runtime `rd-vm`: `androidx.rs` (D) + `gms.rs` (E) — estado real,
   programável pelo agente via `Engine::androidx_*`/`Engine::gms_*`,
   dispatch em `interp.rs` ao lado de m8/m9, DEX-first para AndroidX
   (a lib viaja no APK), host-first para GMS (não há processo GMS).
6. MCP: classificação D/E reusando o registro (edição mínima em `mcp.rs`,
   longe do teste do PR #192) — só se o diff ficar limpo.
7. Docs: `docs/appendix-d-e.md` (ledger de conclusão honesta) +
   `docs/adr/011-appendix-d-e-registries.md` + `worklog.md`.

## Checklist

- [x] S0 Branch + baseline (`rd-framework` 24 testes ok) + este ledger
- [x] S1 `data/jetpack.toml` (46 libs/6 categorias) + `data/gms.toml` (21 serviços/3 categorias) + `docs/appendix-d-e.md` — status = realidade em `main`
- [x] S2 `fd_jetpack.rs` + `fd_gms.rs` + testes unitários (34 passed em rd-framework, clippy/fmt limpos)
- [x] S3 CLI `rd jetpack`/`rd gms` + testes CLI (8 passed, clippy limpo)
- [x] S4 xtask + docs gerados + spec D/E + ADR 012 + ledger D/E (xtask 14 testes ✅, geradores idempotentes)
- [x] S5 Runtime AndroidX (`androidx.rs`) + estado/GC/dispatch + testes
- [ ] S6 Runtime GMS/Firebase (`gms.rs`) + estado/GC/dispatch + testes
- [x] S7 Hierarquias classpath (hook mínimo) + inflate AndroidX — **FECHADO 2026-10-25** (branch `appendix-de-s7-classpath-inflate-20261025` de `main@3e6fa29`; sessao tempchat 8873)
- [ ] S8 MCP (se limpo) + compat-matrix + verificação completa + push/PR

## Log

- 2026-10-25 S7 FECHADO (branch sobre main@3e6fa29, pos-merge do PR #205): hook minimo = extensao das tabelas existentes `builtin_hierarchy`/`builtin_interfaces` em classpath.rs com as cadeias fixas AndroidX/Material (AppCompatActivity→FragmentActivity→ComponentActivity→Activity, widgets AppCompat*, containers Constraint/Coordinator/Drawer/Card/SwipeRefresh/ViewPager(2)/NestedScrollView/FragmentContainerView + LayoutParams, Material* principais; interfaces LifecycleOwner/ViewModelStoreOwner/SavedStateRegistryOwner). Como attach_host_state e o LayoutInflater ja sao subtype-driven, isto destrava inflate de tags AndroidX FQCN sem o DEX das libs. inflate.rs: layout_params_class ganha Constraint/Drawer/ViewPager/RecyclerView e assign_layout_params semeia width/height quando a classe de LP nao tem corpo DEX (bug apanhado pelo teste novo). tests/androidx_hierarchy.rs 5/5 (subtipagem +/-, attach, inflate AXML com AppCompatTextView+MaterialButton+onClick, LP do ConstraintLayout). jetpack.toml: appcompat not_implemented→partial; constraint/drawer/card/swipe/material →stub; coordinator/viewpager2 stub com notes/code actualizados. Docs/spec D/compat regenerados; rd-vm inteiro + rd-framework 34/34 + xtask 14/14 verdes; clippy -D warnings limpo.

- 2026-10-25 S0: clone, branch criada de `main@89bbe8f`, rustup stable
  1.99.1, `cargo test -p rd-framework --lib` = 24 passed. PRs 191/192
  mapeados via `refs/pull/*/head` (refs locais `pr-191`/`pr-192`, só leitura).
- 2026-10-25 S1: inventários TOML validados (parse + unicidade); statuses iniciais conservadores (D: 5 partial m8/m9, 3 stub, resto not_implemented; E: tudo not_implemented).

- 2026-10-25 S3: rd jetpack/gms list|show|check|report no ar (main.rs só aditivo; [LOCK] coordenado entre os agentes). Meu ADR vira 012 (011 ficou com M10 do Runtime-E2E).
- 2026-10-25 S4: xtask jetpack/gms report+spec, docs/jetpack.md+gms.md, tabelas D/E geradas em RustyDroid.md (marcadores BEGIN/END), compat-matrix com seções D/E, ADR 012. Testes xtask travam docs==gerado.
- 2026-10-25 PR-1 (S0–S4): pacote validado localmente (workspace 753 passed no base antigo); rebaseado sobre main 072c317 (pós-merge do Apêndice C) e reemitido como patch limpo para abertura de PR. O runtime S5 segue como patch incremental separado.
- 2026-10-25 S5 FECHADO: crates/rd-vm/src/androidx.rs (DEX-first) + tests/androidx.rs 13/13 (incl. callbacks DEX lifecycle/livedata e invariante dex_body_wins). HostState.androidx (GC-root via Debug; gc.rs NÃO precisou de edição). 12 libs not_implemented/stub→partial (lifecycle x4, navigation, datastore-valores, collection, startup, multidex, biometric, browser, guava) + core/work estendidos → 18 partial/2 stub/26 not_implemented. Docs/spec/compat regenerados. Regressão: suíte rd-vm inteira ok; clippy lib+test -D warnings ok. Desvios single-thread documentados no cabeçalho do módulo.
