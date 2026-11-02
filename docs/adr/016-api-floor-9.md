# ADR 016 — Piso de API de execução: minSdk 26 → 9 (tier T0 best-effort)

- **Status:** proposto (decisão do dono na sala 8873 em 2026-10-25; aceito no merge — precedente ADR 008)
- **Data:** 2026-10-25 · **Escopo:** `crates/rd-apk/src/lib.rs` (`FLOOR_MIN_SDK`), bordas que o aplicam (`crates/rd-agent/src/mcp.rs` install, `crates/rd-cli/src/main.rs` `boot_app`/`cmd_behavior`, snapshot e inspect já liam a constante), `RustyDroid.md` (cabeçalho + Apêndice A), `.github/rd-sentinel/baseline.json` (`floor_min_sdk`, commit isolado). **Sem** tocar: piso de hardware, piso de execução nativa, `api_floor` dos scoreboards.
- **Relacionados:** Apêndice A (tiers), ADR 008 (orçamentos moto-e5), Lei 1 (erros estruturados), issue #82/#215 (honestidade de launch nas bordas).

## Contexto

A spec 2.1 fixava o piso de compatibilidade em `minSdkVersion 26` (Android
8.0, o Android de fábrica do Moto E5): APKs abaixo disso eram parseados e
inspecionados, mas a execução era recusada com `BELOW_FLOOR`. Duas evidências
mudaram o quadro:

1. O APK-Tester (suíte oficial de fixtures) sempre construiu uma escada
   minSdk 9→36 — o dono confirmou que ela existe porque 9 é o piso planejado
   do produto, não um artefato de teste.
2. A rodada end-to-end do APK-Tester expôs que as bordas divergiam: o MCP
   `install` recusava <26, mas `rd app run` executava em silêncio. A #215
   unificou a aplicação do piso; restou ao dono decidir QUAL piso vale.

O dono decidiu: **o piso de execução é 9 (Android 2.3 Gingerbread)**.

## Decisão

1. `FLOOR_MIN_SDK: 26 → 9` no rd-apk. Todas as bordas que recusam execução
   (MCP install, CLI `rd app run/dump/shot`, fast-path de snapshot) leem a
   mesma constante — comportamento idêntico nas superfícies, textos de erro
   interpolam o piso (sem literal mágico para o próximo drift).
2. Apêndice A ganha o **tier T0 (APIs 9–25): best-effort**. Executa com os
   comutadores de comportamento por `targetSdk`
   (`data/behavior-switches.toml`). O que não existia na época segue a
   semântica histórica ou responde erro estruturado — nunca invenção
   silenciosa (Lei 1). T1 (26–28) passa a "compatibilidade plena (piso
   histórico)"; **o gate de release continua ancorado em T1+**.
3. `floor_min_sdk` no baseline do RD Sentinel acompanha (26→9) em commit
   isolado, sob o label `sentinel:baseline-approved` do dono — é mudança de
   contrato declarada, não afrouxamento silencioso de gate.

## O que NÃO muda (três "pisos" distintos)

- **Piso de hardware (Moto E5, Apêndice A2):** o profile `moto-e5` continua
  modelando um aparelho com Android 8.0/API 26 de fábrica. Um app de
  minSdk 9 executando no RustyDroid roda num "sistema" cujo nível de
  comportamento base é o do profile — como um app legado instalado num
  Android moderno real. Os orçamentos do ADR 008 (CPU/RSS/startup)
  continuam calibrados no E5.
- **Piso de execução nativa (ELF):** `rd-engine-registry` segue exigindo
  minSdk ≥ 26 para executar código nativo, com erro estruturado próprio
  ("native execution requires minSdkVersion 26 or greater"). É um limite do
  loader/linker modelado, não do contrato de API; revisitar é trabalho
  futuro do trilho de engines (M11).
- **Piso de cobertura (Apêndice C):** `api_floor = 26` em
  `data/api-coverage.toml`/`gms.toml`/`jetpack.toml` mede a superfície de
  API modelada a partir do nível 26 — métrica de auditoria, não permissão
  de execução. Não muda neste ADR.

## Riscos conhecidos do tier T0 (aceitos conscientemente)

- **Permissões em runtime só existem da API 23.** Apps 9–22 assumem
  permissões concedidas na instalação. O engine de permissões já modela
  concessão por instalação; o risco é um app legado chamar APIs de
  runtime-permission que não existiam — resposta estruturada, não crash.
- **Sem canais de notificação (<26), sem `invoke-custom`/invokedynamic
  (<26 no ART), sem autofill/PiP/adaptive icons.** DEX genuinamente antigo
  não usa nada disso; bibliotecas modernas compiladas com minSdk baixo
  passam por desugar ou falham em APIs pontuais — cada falha deve ser erro
  tipado no ponto de uso (o comportamento dos stubs host), não recusa na
  porta. A escada do APK-Tester é a rede de regressão contínua disso.
- **Multidex legado, RenderScript, `org.apache.http` e cia.:** superfície
  histórica fora do Apêndice C; entra pela mesma regra — erro estruturado
  até alguém modelar, nunca sucesso falso.
- **Fidelidade ≠ promessa:** best-effort significa executar e responder com
  honestidade; não significa compatibilidade medida tier-1. Os scoreboards
  continuam separando "detectado/parseado" de "executado" (ADR 003).

## Alternativas consideradas

- **Manter 26 nas duas bordas** (a #215 original): recusaria na CLI todo app
  real com minSdk 21–25 — fatia enorme da Play Store histórica — e
  desperdiçaria a escada 9–25 que a suíte já executa.
- **Executar com aviso (warn-and-run) mantendo o "piso" 26 nominal:** cria
  dois pisos (um de marketing, um real) e um aviso que agentes ignoram;
  pior que declarar o piso verdadeiro.
- **Flag `--allow-below-floor`:** útil como escape, mas esconde a decisão de
  produto num opt-in por invocação; o dono preferiu o piso declarativo.

## Evidência (sweep APK-Tester v1.0.2, `rd app run`, 57 APKs)

Medido localmente com builds profiling (tabelas rodada-a-rodada anexadas ao
PR da #215). Três marcos sobre a mesma suíte:

| Rodada | Tela real | Silencioso (exit 0 vazio) | Erro estruturado |
|---|---|---|---|
| main 73b5685 (oficial, PR-Agent) | 1/57 | 41 | 15 |
| main 3e6fa29 + paged-inspect | 2/57 | 41 | 14 |
| **2aedc71 + #215 + #216 + piso 9** | **14/57** | **1** | **42** |

Na rodada com piso 9: a escada rende **12/12 com tela** (degraus 9–36; os
legados dependiam só dos fixes de `getAssets`/`getApplicationInfo`, não do
piso), os dois flagships renderizam, `BELOW_FLOOR` some da suíte (nada tem
minSdk < 9), e o Flutter (minSdk 21) executa o boot Java até a fronteira
nativa (`UnsatisfiedLinkError` estruturado — trilho M11). Os 40
`VM_TYPE_ERROR` são os fixtures cuja `MainActivity` não estende `Activity`
(contrato da #215); o único exit-0-sem-tela é o Chaquopy (Activity real sem
UI Java — comportamento fiel, já classificado assim na rodada oficial). A
VM, portanto, nunca dependeu do piso 26 para executar esses APKs: o piso
era política de borda, não capacidade — este ADR declara a política nova.

## Consequências

- `BELOW_FLOOR` passa a ser raro (minSdk ≤ 8 é arqueologia); a mensagem e o
  código permanecem o contrato para quem ficar abaixo de 9.
- A escada 9→36 do APK-Tester vira contrato executável completo nas
  rodadas noturnas, não só inspeção.
- Trabalho futuro natural (fora deste ADR): semântica de install-time
  permissions para targetSdk <23 na tabela de behavior switches, e revisão
  do piso nativo quando o trilho M11 atacar ELF legado.
