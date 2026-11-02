# ADR 018 — Restaurar o RD Sentinel após exclusão acidental (`4489813`)

- **Status:** Accepted (restauração executada, 2026-10-30)
- **Scope:** `tools/rd-sentinel/**` (restauração byte-a-byte), `docs/adr/018-*`
- **Relaciona-se a:** [ADR 010](010-rd-sentinel-anti-regression-bot.md)
  (o Sentinel é a camada anti-regressão além do CI), ADR 003 / ADR 017
- **Identity (inalterada):** runtime Android em Rust; sem VM, KVM ou kernel
  convidado; MSRV 1.76; minSdk 26; determinismo de MCP/runtime; nenhum `unsafe`
  novo fora de `rd-jni`/`rd-ndk`.

## 1. Contexto

O commit `4489813db513b42c531979b079397c50edd54e25` ("Delete tools/rd-sentinel
directory") foi aplicado **diretamente na `main`** e removeu os 10 arquivos do
bot: 9.913 linhas, incluindo `sentinel/audit.py`, `sentinel/campaigns.py`,
`sentinel/model.py`, `sentinel/cli.py`, `sentinel/github_api.py` e a suíte
`tests/test_rd_sentinel.py`.

A exclusão **não** foi uma retirada do Sentinel do projeto. Tudo o que define e
consome o bot continuou versionado e ativo:

| Artefato | Estado após `4489813` |
|---|---|
| `.github/rd-sentinel/policy.json` | presente (política v1.2.0 completa) |
| `.github/rd-sentinel/baseline.json` | presente |
| `.github/workflows/rd-sentinel.yml` | presente — executa `tools/rd-sentinel/rd_sentinel.py` |
| `.github/workflows/rd-sentinel-deep.yml` | presente — idem |
| `.github/workflows/rd-sentinel-command.yml` | presente — idem (comando `/rd-sentinel`) |
| `.github/workflows/rd-sentinel-publish.yml` | presente |
| `docs/rd-sentinel.md` | presente (guia do operador, descreve v1.1/v1.2) |
| `worklog.md` §2 | presente — lista `tools/rd-sentinel` no mapa de crates |
| `.github/CODEOWNERS` | presente — `tools/rd-sentinel/ @nadabgfccvd` |

O resultado foi um estado inconsistente: **política e baseline presentes,
executor ausente**. O workflow cai no ramo de bootstrap
(`Base has no RD Sentinel yet; bootstrap mode uses candidate bot`), que é um
caminho pensado para o *primeiro* PR que introduz o bot — não para a `main` de
um repositório que já o tinha. Na prática, todo PR passou a depender de um bot
que não existia mais em lugar nenhum, e o gate anti-regressão ficou inoperante.

Como o commit foi o **topo da `main`**, não houve janela em que política e
executor estivessem simultaneamente corretos: qualquer PR aberto contra essa
base herdou a inconsistência.

## 2. Decisão

1. **Reverter `4489813` de forma exata** (`git revert 4489813`), sem edições
   manuais. A árvore de `tools/rd-sentinel/` volta byte-a-byte ao estado de
   `3761815`, que é o último commit em que o Sentinel existia e estava verde.
2. **Não reformar o bot neste PR.** Uma restauração e uma refatoração no mesmo
   diff tornam impossível provar que a restauração é fiel. Melhorias futuras no
   Sentinel seguem o fluxo normal do ADR 010 (ADR próprio + aprovação de
   política).
3. **Reabrir a discussão de causa-raiz separadamente** (ver §4), não silenciar
   o problema com a restauração.

## 3. Por que reverter em vez de reescrever

- **Fidelidade auditável.** O `git revert` prova igualdade com `3761815`. Um
  "retype" do bot não teria como demonstrar isso, e o Sentinel é justamente a
  camada cuja integridade precisa ser demonstrável.
- **Custo zero de risco.** Nenhuma linha de Rust muda; nenhum budget do
  `moto-e5` é afetado; nenhuma superfície `android.*` muda.
- **Evidência preservada.** A suíte do próprio Sentinel volta a rodar
  (`python3 -m unittest discover -s tests` → 63 testes, OK). O §3 do worklog
  exige exatamente essa verificação para mudanças no bot.
- **Lei 1 (sem falha silenciosa).** Deixar o gate desligado enquanto "o CI ainda
  está verde" é o pior modo de falha possível: ausência de evidência sendo lida
  como evidência de ausência de regressão.

## 4. Consequências

### Positivas

- O gate anti-regressão volta a funcionar em todos os PRs.
- Os quatro workflows `rd-sentinel*` deixam de depender do ramo de bootstrap.
- A suíte de 63 testes do bot volta a ser executável e auditável.
- `worklog.md`, `docs/rd-sentinel.md`, `CODEOWNERS` e o mapa de crates voltam a
  descrever a árvore real (a documentação estava correta; o código é que sumiu).

### Negativas / custo

- O PR de restauração toca `tools/rd-sentinel/**`, que é **protected path** na
  política do próprio Sentinel. Ele é, portanto, bloqueado pelo seu próprio
  finding P0 (`protected-sentinel-path-changed`) até que um mantenedor aplique
  a label `sentinel:policy-approved`. Isso é o gate funcionando como projetado,
  não um defeito: a política exige que mudanças no bot tenham revisão humana
  separada — inclusive quando a mudança é "voltar a existir".
- O diff também dispara o trigger `contract-constant` (as strings
  `STARTUP_BUDGET_EMULATED_MS` / `FLOOR_MIN_SDK` reaparecem nos arquivos
  restaurados), o que exige um ADR no mesmo diff. Este documento é esse ADR.

### Neutras

- `BOT_VERSION` permanece `1.2.0` (o valor de `3761815`). Nada de comportamento
  novo é introduzido por uma restauração.

## 5. Causa-raiz e follow-up

A exclusão foi feita pelo botão "Delete directory" da interface web, direto na
`main`. Duas fragilidades permitiram que isso passasse sem barreira:

1. **`main` aceita escrita direta.** Enquanto existir push direto na `main`, um
   clique de UI consegue remover um subsistema inteiro sem PR, sem CI e sem
   Sentinel. A recomendação é exigir PR + checks verdes para a `main`,
   mantendo um bypass explícito do owner para emergências.
2. **A ausência do bot era invisível.** O workflow de bootstrap faz o Sentinel
   *parecer* configurado quando ele não existe. Um *guard* explícito — falhar
   com mensagem clara se `.github/rd-sentinel/policy.json` existir e
   `tools/rd-sentinel/rd_sentinel.py` não — transforma "gate desligado por
   acidente" em erro visível.

Nenhum dos dois é resolvido por este ADR; ambos ficam registrados como
follow-up para um PR próprio (item de política protegida, exige o mesmo fluxo de
aprovação deste).

## 6. Verificação

```sh
cd tools/rd-sentinel && python3 -m unittest discover -s tests
# Ran 63 tests in 0.712s
# OK
```

`git diff 3761815 HEAD -- tools/rd-sentinel/` deve estar vazio.
