# worklog.md: guia de entrada para agentes

> Este arquivo é para **o próximo agente que chega sem contexto**. Ele diz como se
> portar no repositório. **Não é diário de sessão**: não acrescente "Task ID",
> logs de trabalho ou resumos de sessão aqui. O histórico fica no `git log` e nos
> PRs. Se você descobrir uma regra **duradoura** (algo que vai pegar o próximo
> agente também), acrescente uma linha curta na seção certa.
>
> O histórico longo anterior (3.4k linhas, até 2026-10-25) está no git:
> `git show c227eba:worklog.md`.

## 1. Comece por aqui

1. Leia `RustyDroid.md`, a especificação mestra e a fonte da verdade sobre escopo, milestones e DoD.
2. Leia `CONTRIBUTING.md` (as regras de ouro) e `.github/PULL_REQUEST_TEMPLATE.md`.
3. O estado atual está em `docs/ROADMAP.md`, `docs/compat-matrix.md` (gerado) e nos `docs/m*.md` / `docs/appendix-*.md`.
4. As decisões de arquitetura estão em `docs/adr/` (numeração sequencial; veja o maior número antes de criar um ADR).
5. Antes de começar, veja as issues e os PRs abertos. Não duplique trabalho em andamento.

## 2. Mapa rápido

| Crate | Papel |
|---|---|
| `rd-apk` | ZIP, AXML, ARSC, manifest e assinaturas (base do workspace) |
| `rd-dex` | parser DEX (inclui o LazyDex do ADR 008) |
| `rd-vm` | interpretador, heap/GC, host Android, JIT (feature `jit`, ADR 007) |
| `rd-framework` / `rd-render` | framework headless e renderização |
| `rd-agent` | servidor MCP (`rd mcp`) e live viewer |
| `rd-ndk` / `rd-jni` | código nativo; **os únicos crates onde `unsafe` é permitido** |
| `rd-engine-registry` | Apêndice G (engines) |
| `rd-cli` | binário `rd` |
| `xtask` | geradores de relatório (`cargo xtask …`) |
| `tools/rd-sentinel` | bot anti-regressão (ADR 010) |

As dependências só apontam para baixo (rd-apk → rd-dex → rd-vm → …). Não crie ciclos.

## 3. Portões obrigatórios (CI e Sentinel)

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace            # ou -p <crate> durante o desenvolvimento
```

- MSRV 1.76. Não use APIs mais novas.
- Conventional Commits (`feat(vm): …`, `fix(agent): …`, `perf(rd-dex): …`).
- Declare o uso de IA no PR e preencha o template.
- O CI procura strings parecidas com tokens. **Nunca** faça commit de credenciais.
- **Docs gerados**: se mudar a fonte, regenere no mesmo PR.
  - `docs/api-coverage.md`: `cargo xtask api-coverage-report` (mudou `data/api-coverage.toml`?)
  - `docs/compat-matrix.md`: `cargo xtask compat-report docs/compat-matrix.md`
  - `docs/mcp-tools.md`: `rd mcp --gen-docs docs/mcp-tools.md` (mudou tool ou descrição do MCP?)
- O job Docs-integrity confere textos em `RustyDroid.md` (versão da especificação, piso minSdk, `moto-e5`). Não os quebre.

### RD Sentinel (o relatório fica no comentário `rd-sentinel-report` do PR)
- **Não enfraqueça** `.github/rd-sentinel/policy.json` nem `baseline.json`.
- Os labels `sentinel:*-approved` são decisão **humana** do mantenedor. Agente não aplica label.
- Bloqueios comuns e a **causa real** de cada um. Corrija a causa, não o sintoma:
  - "no test evidence": falta um `#[test]` novo.
  - "contract trigger has no ADR": mudança de contrato pede ADR (ou adendo) em `docs/adr/`.
  - `unwrap`/`expect` novos em código de produção: troque por erro tipado.
  - "named test removed" / "public fn removed" num PR antigo: geralmente é a base desatualizada. Faça merge da `main` no branch.
  - Tamanho do binário: o gate por PR é +5% / +256 KiB. Mudança grande pede números de antes/depois.
- A suíte do próprio Sentinel não roda no CI. Se mexer nele, rode `cd tools/rd-sentinel && python3 -m unittest discover -s tests`.

## 4. Regras de convivência

- Nada de push na `main` e nada de force-push. Deltas novos vão por cima do branch.
- Merge só com aprovação do dono.
- PR de performance só com números de antes/depois (mesma máquina, mesmo protocolo).
- Mudança de API Android → atualize `data/api-coverage.toml` e regenere o relatório do Apêndice C.
- Honestidade: nada de "funciona" sem teste. Erros tipados em vez de stubs silenciosos.
- Logs de sessão ficam fora do repositório (`PROGRESS-*.md` está no `.gitignore`).

## 5. Armadilhas conhecidas

- `cargo build --release` com todas as features consome muita RAM. Em sandbox, use `CARGO_BUILD_JOBS=1` e o perfil `profiling`.
- Não rode dois builds cargo ao mesmo tempo no mesmo target dir.
- Patch em diff puro: use `git apply --index`. `git am` só aceita o formato mailbox.
- Com PR empilhado, quando o PR de baixo é mergeado o GitHub não troca a base sozinho se o branch não foi apagado. Troque a base para `main` manualmente.
