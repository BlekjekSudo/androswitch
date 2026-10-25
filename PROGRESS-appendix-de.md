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
- [ ] S3 CLI `rd jetpack`/`rd gms` + testes CLI
- [ ] S4 xtask + docs gerados + spec D/E + ADR 011 + ledger D/E
- [ ] S5 Runtime AndroidX (`androidx.rs`) + estado/GC/dispatch + testes
- [ ] S6 Runtime GMS/Firebase (`gms.rs`) + estado/GC/dispatch + testes
- [ ] S7 Hierarquias classpath (hook mínimo) + inflate AndroidX
- [ ] S8 MCP (se limpo) + compat-matrix + verificação completa + push/PR

## Log

- 2026-10-25 S0: clone, branch criada de `main@89bbe8f`, rustup stable
  1.99.1, `cargo test -p rd-framework --lib` = 24 passed. PRs 191/192
  mapeados via `refs/pull/*/head` (refs locais `pr-191`/`pr-192`, só leitura).
- 2026-10-25 S1: inventários TOML validados (parse + unicidade); statuses iniciais conservadores (D: 5 partial m8/m9, 3 stub, resto not_implemented; E: tudo not_implemented).

