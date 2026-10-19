# ADR 003 — M8 compat host: interceptação seletiva + estado real consultável

## Contexto
M8 (spec PARTE 4): "Compat real — app OSS médio navega funcional; v0.9".
Os subsistemas pedidos (RecyclerView, Fragments, Dialogs, OkHttp, SQLite/Room,
Services/broadcasts/deep links, WorkManager) são, na maioria, **bibliotecas que
viajam no DEX do APK** (androidx/okhttp), não classes `android.*`. A VM só
despachava `Landroid/` para o host; o resto executava DEX.

## Opções
1. Implementar cada biblioteca inteira em Rust — duplicar AndroidX/OkHttp.
2. Deixar todo o DEX rodar e só preencher `android.*`/`java.*` — RecyclerView
   real exige Canvas/MeasureSpec/etc. demais para o headless atual.
3. **Interceptação seletiva**: o dispatch (`interp`) consulta `crate::m8`
   antes da resolução DEX, mas só para métodos explicitamente listados;
   `None` cai para o DEX real da biblioteca. O host implementa esses métodos
   com **estado real consultável** (`Engine::m8_*`), chamando de volta o DEX
   do app onde o Android chamaria (Adapter.getItemCount/onCreate/onBind,
   SQLiteOpenHelper.onCreate, BroadcastReceiver.onReceive,
   DialogInterface.OnClickListener, OkHttp Callback).

## Escolha
Opção 3. É a única que entrega comportamento observável de ponta a ponta sem
reescrever bibliotecas: a RecyclerView pergunta a contagem ao adapter do app
e anexa os itemViews reais; o SQLite guarda linhas e responde cursores; o
OkHttp devolve a resposta registrada sem fingir rede.

## Desvios documentados (nunca silenciosos)
- Single-thread determinístico: `Call.enqueue`, broadcasts e commits de
  fragment são síncronos; WorkManager registra o pedido mas não roda
  `Worker.doWork()` em background (harness conclui via `m8_complete_work`).
- SQLite em memória, subconjunto SQL (CREATE/DROP TABLE, SELECT … FROM,
  insert/update/delete com `col=literal|?`); sem arquivo, sem JOIN/ORDER.
- OkHttp é shim offline: nenhuma rede; resposta vem de
  `Engine::m8_set_http_response` (default 200 vazio) e toda URL é registrada.
- RecyclerView popula no máximo 20 itens (viewport headless).
- SharedPreferences vivem na VM (sem arquivo); fragments sem animações.

## Consequências
- Apps médios navegam: lista (RecyclerView) → detalhe (fragment/dialog) →
  dados (SQLite/Room) → rede (OkHttp shim) → background (Service/WorkManager)
  é exercitável por testes (`crates/rd-vm/tests/m8.rs`) e pelo agente via
  `compat_gaps` (ids `m8-*`).
- Métodos não listados continuam `NOT_FOUND` tipados (Lei 1) — a superfície
  cresce por sondagem real, como nos 45 gaps do primftpd.
