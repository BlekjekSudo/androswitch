# Apêndices D + E — registro executável e ledger de conclusão honesta

Este documento é o ledger dos Apêndices D (AndroidX/Jetpack, shims por
módulo) e E (Google Play Services / Firebase, modo "sem GMS" honesto) da
spec (`RustyDroid.md`). Como no Apêndice C, a fonte executável da verdade
**não é este texto**: são `data/jetpack.toml` e `data/gms.toml`, validados
por `rd-framework::fd_jetpack`/`fd_gms`, consultáveis via
`rd jetpack`/`rd gms` e regenerados em `docs/jetpack.md`/`docs/gms.md`
por `cargo xtask jetpack-report|gms-report`.

## Semântica de status (idêntica à do Apêndice C)

- `supported` — modelo host com estado para o subconjunto documentado em
  `notes`/`key_apis`, com testes. **Nunca** significa a biblioteca ou o
  serviço inteiro.
- `partial` — um subconjunto real executa: DEX da biblioteca sobre a
  plataforma host e/ou shim host com estado. O restante cai em DEX,
  `NOT_FOUND` tipado ou desvio documentado.
- `stub` — só reconhecimento: hierarquia de tipos, constantes, inflate ou
  alocação sem comportamento. Presença em DEX não é cobertura.
- `not_implemented` — nenhum modelo registrado; a chamada falha de forma
  tipada (`NOT_IMPLEMENTED`/`NOT_FOUND`), nunca com sucesso fingido.

Nenhum status sobe sem código + teste no mesmo PR (regra do Apêndice C).

## Estratégias do Apêndice D

Bibliotecas AndroidX e de terceiros **viajam no APK** (DEX). Por isso cada
módulo declara uma estratégia:

- `dex-passthrough` — o DEX da biblioteca executa na VM; o host só fornece
  a plataforma `android.*`/`java.*` embaixo. É a estratégia correta para
  bibliotecas puras (Gson, Moshi, coroutines…); o status mede quanto da
  plataforma embaixo + dos pontos de integração funciona de verdade.
- `host-shim` — o host intercepta métodos listados com um modelo com
  estado (ex.: RecyclerView e WorkManager no M8; LiveData/ViewModelStore
  como fallback quando o APK não traz o corpo DEX). Fora da lista, o
  dispatch devolve `None` e o interpretador segue para o DEX real.
- `host-native` — implementação nativa no host, sem DEX esperado.

Jetpack Compose (UI-05) continua experimental/longo prazo na PARTE 0 da
spec: o registro o mantém `not_implemented` com gap explícito — registrar
não é implementar.

## Modo "sem GMS" honesto (Apêndice E)

Não há processo GMS, conta Google nem backend real — e o registro diz
isso em toda linha (`network = "none"`, validado):

- As APIs **existem** como shims host com respostas estruturadas,
  determinísticas e **programáveis pelo agente** (`Engine::gms_*`):
  localização fixa, usuário fake, catálogo de billing, token FCM etc.
- Eventos são **capturados** e consultáveis (Analytics, Crashlytics —
  incluindo crashes reais da VM —, ads, compras), nunca enviados à rede.
- Firestore/RTDB são stores locais em memória (record/replay NET-03):
  writes disparam listeners sincronamente, sem sincronização externa.
- `GoogleApiAvailability` é programável; o default `SUCCESS` significa
  apenas "o shim responde", jamais "o Google respondeu".

## Como atualizar

1. Mexeu num shim AndroidX/GMS? Atualize a linha em
   `data/jetpack.toml`/`data/gms.toml` **no mesmo PR**.
2. Rode `rd jetpack check --json` / `rd gms check --json` (ou os testes
   de `rd-framework`).
3. Regenere relatórios e spec: `cargo xtask jetpack-report jetpack-spec
   gms-report gms-spec`.

## Ledger de execução (branch `appendix-d-e-20261025`)

Progresso passo a passo em `PROGRESS-appendix-de.md` (raiz do repo).
Os PRs #191/#192 (Apêndice C, outro agente) não são tocados por este
trabalho; arquivos compartilhados recebem, no máximo, edições aditivas
mínimas documentadas no PR.

- **S6 (2026-10-25, branch `appendix-e-gms-runtime-20261025`)**: o
  runtime do Apêndice E vive em `crates/rd-vm/src/gms.rs` (host-first,
  estado em `HostState.gms`, dispatch aditivo em `interp.rs` ao lado do
  Apêndice D) com 21 testes em `crates/rd-vm/tests/gms.rs`. 20 dos 21
  serviços sobem de `not_implemented` para `partial` (subconjuntos
  testados descritos em `data/gms.toml`); ML Kit permanece
  `not_implemented` e sem dispatch — o teste
  `unknown_gms_surface_returns_none` fixa essa fronteira.
