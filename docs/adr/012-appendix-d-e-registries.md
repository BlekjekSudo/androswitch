# ADR 012 — Registros executáveis dos Apêndices D e E

- **Status:** aceito; runtime de shims em implementação incremental sobre estes registros
- **Data:** 2026-10-25
- **Escopo:** Apêndice D (AndroidX/Jetpack por módulo) e Apêndice E (GMS/Firebase sem GMS)

## Contexto

Os Apêndices D e E eram apenas tabelas/balas estáticas em `RustyDroid.md`:
não havia registro executável, CLI, classificação de tipos nem status
verificável por módulo — ao contrário do Apêndice C (`data/api-coverage.toml`,
`rd-framework::fd_api_coverage`, ADR implícito no desenho) e do Apêndice G
(ADR 003). O runtime tinha fragmentos AndroidX reais (RecyclerView, Fragment
fallback, WorkManager e Room no M8; NotificationCompat no M9; OkHttp offline)
sem um placar que dissesse o que existe, com qual estratégia, e o que continua
gap. Para GMS/Firebase não havia nenhum shim nem vocabulário de status: um
app chamando `com.google.android.gms.*` encontrava o vazio.
(Numeração: ADR 011 é do M10, branch `feat/m10-virtual-devices-20261025`;
este é o 012 para não colidir na integração.)

## Decisão

1. `data/jetpack.toml` e `data/gms.toml` são as fontes executáveis dos
   Apêndices D e E, no mesmo molde do Apêndice C: categorias exatas da spec
   (6 + 3), toda biblioteca/serviço nomeado pela spec obrigatoriamente
   presente (44 + 18; extras além da spec permitidos e marcados), prefixos
   globalmente únicos, caminhos de código/evidência validados e existentes.
2. A semântica de status é a do Apêndice C (`supported` = subconjunto
   documentado com modelo+teste, nunca a biblioteca/serviço inteiro;
   `partial` executa subconjunto real; `stub` só reconhece;
   `not_implemented` falha tipado). Status só sobe com código+teste no mesmo
   PR. Registro nunca é alegação de execução (`execution_claimed: false`).
3. Apêndice D declara estratégia por módulo: `dex-passthrough` (bibliotecas
   viajam no APK; o DEX executa na VM), `host-shim` (host intercepta métodos
   listados com estado e devolve `None` para o resto cair no DEX) e
   `host-native`. Isso codifica o princípio do M8 ("host só sequestra o que
   implementa de verdade") em vez de reescrever AndroidX em Rust.
4. Apêndice E valida o modo honesto: `mode = "no-gms-honest"` na tabela e
   `network = "none"` + contrato `programmable` em todo serviço. Não há
   processo GMS, conta nem backend; os shims são locais, determinísticos e
   programáveis pelo agente, com eventos capturados (nunca enviados) e
   stores locais record/replay (NET-03). `SUCCESS` de disponibilidade
   programável significa apenas "o shim responde".
5. Prefixos têm um único dono entre os registros: Firebase é Apêndice E
   (a linha 3rd-party do D aponta para cá); Gson/Guava/Material são
   Apêndice D; Billing (`com.android.billingclient`) é Apêndice E mesmo
   fora do pacote `gms`. Classificadores devolvem `non_jetpack`/`non_gms`
   para o ecossistema vizinho em vez de um match falso.
6. Superfícies públicas: `rd jetpack|gms list|show|check|report` (com
   `--data` explícito sem fallback silencioso), `cargo xtask
   jetpack-report|jetpack-spec|gms-report|gms-spec`, tabelas geradas na
   spec entre marcadores, `docs/jetpack.md`/`docs/gms.md`, seções no
   compat-matrix e classificação reaproveitável pelo MCP. Os shims de
   runtime vivem em `rd-vm` (`androidx.rs`/`gms.rs`) e expõem estado ao
   agente via `Engine::androidx_*`/`Engine::gms_*`, no padrão M8.

## Consequências

- D/E ganham o mesmo rigor anti-overclaim do C: CI quebra se um status
  subir sem código/evidência ou se uma biblioteca/serviço da spec sumir.
- O trabalho de runtime fica fatiatável e coordenável entre agentes
  (arquivos novos por apêndice; compartilhados sob `[LOCK]`), sem tocar
  nos PRs do Apêndice C (#191/#192).
- Compose (UI-05), media3, cameraX e afins continuam gaps nomeados neste
  registro — registrar não é implementar; promoções exigem ADR/teste
  próprios quando saírem de `not_implemented`.
