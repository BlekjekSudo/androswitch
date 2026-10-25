<!-- GERADO: `rd mcp --gen-docs docs/mcp-tools.md` — NÃO editar à mão. -->

# Ferramentas do Agente MCP (M5)

Servidor `rustydroid-agent` v0.9.0 · protocolo `2024-11-05` · transporte stdio (JSON-RPC 2.0, uma mensagem por linha).

Total: **47 tools**. Este catálogo é gerado de `rd_agent::mcp::tool_defs()` — o MESMO registro respondido em `tools/list` e impresso por `rd mcp --describe-tools`.

## Controles de segurança

- **Sandbox de caminhos**: `install`/`screenshot` só acessam arquivos dentro das raízes `RD_MCP_SANDBOX` (default: cwd + temp do sistema); `..` e URLs são recusados (`PATH_OUTSIDE_SANDBOX`/`URL_NOT_ALLOWED`).
- **APK excessivo**: teto de tamanho `RD_MCP_MAX_APK_MB` (default 256 MB) — checado antes de ler (`APK_TOO_LARGE`).
- **Tempo ilimitado**: `wait`/`swipe`/`wait_for` têm teto `RD_MCP_MAX_WAIT_MS` (default 60 000 ms) (`LIMIT_EXCEEDED`).
- **Batch**: teto de passos `RD_MCP_MAX_BATCH` (default 64); sem aninhamento; retomada idempotente via `resume_from`.
- **Estado consistente**: install/remove/clear são transacionais (validam tudo antes de mutar); app em execução é encerrado antes de remove/clear/reinstall do mesmo package.

## Erros

Falhas de tool voltam com `isError: true` e o contrato estável `{code, cause, suggestion?, module_id}`. Erros de PROTOCOLO são JSON-RPC (-32601 método, -32700 parse).

## Catálogo

### `install`

Instala um APK na sessão (abre, valida o piso API 26 e prepara o estado de permissões). Caminho PRECISA estar dentro do sandbox (RD_MCP_SANDBOX); URL é recusada; APK acima de RD_MCP_MAX_APK_MB (default 256) é recusado. Reinstalar o package EM EXECUÇÃO o encerra (sem estado inconsistente). Não executa o app.

- Parâmetros: `apk` · obrigatórios: `apk`

### `launch`

Executa o app instalado: cria a activity (LAUNCHER ou `activity`) e roda o lifecycle até o onCreate completar. `activity` precisa estar declarada como <activity>/<activity-alias> no AndroidManifest.xml E existir no DEX (issues #82/#83) — o Android recusa o que não está declarado; um <activity-alias> lança a sua targetActivity (issue #109). Se o MESMO package já está em execução, o relançamento reutiliza o processo (warm start: sem re-parse do DEX; o resultado traz warm=true) (M7). Com `package` lança outro app do registro (multi-app); sem, o app ativo (último install/launch).

- Parâmetros: `activity, engine, observe, package`

### `tap`

Toque simulado nas coordenadas (px do viewport 720×1440). Retorna se algum listener foi acionado.

- Parâmetros: `observe, x, y` · obrigatórios: `x, y`

### `swipe`

Gesto de arraste determinístico (x1,y1)→(x2,y2) em `ms` (cap RD_MCP_MAX_WAIT_MS). Reporta as views atravessadas em ordem; down+up na MESMA view vira clique (listener disparado) — semântica Android tap vs scroll.

- Parâmetros: `ms, observe, x1, x2, y1, y2` · obrigatórios: `x1, y1, x2, y2`

### `type_text`

Digita texto: na EditText focada/primeira visível; sem EditText (Jetpack Compose, editores custom) usa o caminho do IME da plataforma — a view focada cria o InputConnection (onCreateInputConnection) e recebe commitText; toque no campo antes. `replace=true` substitui; default anexa. Devolve o texto final lido do editor.

- Parâmetros: `observe, replace, text` · obrigatórios: `text`

### `key`

Evento de tecla: BACK (finaliza a activity corrente), DEL (apaga último caractere da EditText), ENTER (no editor do IME — Compose — executa a IME action do EditorInfo via performEditorAction; senão entregue sem efeito). Outras teclas: erro tipado NOT_IMPLEMENTED com a lista suportada.

- Parâmetros: `key, observe` · obrigatórios: `key`

### `wait`

Avança o clock virtual (ms) e executa os runnables do Looper com deadline vencida. Retorna quantos rodaram. Cap RD_MCP_MAX_WAIT_MS (default 60_000) — contra tempo ilimitado.

- Parâmetros: `ms, observe` · obrigatórios: `ms`

### `wait_for`

Espera CONDICIONAL: avança o clock virtual em passos de `step_ms` (default 100) até a view (resource_id/text/class) existir ou `timeout_ms` vencer (cap RD_MCP_MAX_WAIT_MS). `class` casa o nome completo ou o nome simples exato (issue #78). found=false é resposta honesta (não é erro) — o agente decide.

- Parâmetros: `class, observe, resource_id, step_ms, text, timeout_ms` · obrigatórios: `timeout_ms`

### `ui_tree`

Dump da UI corrente: `xml` (uiautomator, formato dos seletores) ou `text` (árvore textual). Views GONE não aparecem.

- Parâmetros: `format`

### `screenshot`

Screenshot PNG determinístico 720×1440. Com `path` escreve o arquivo (DENTRO do sandbox — RD_MCP_SANDBOX; symlink no destino é recusado: SYMLINK_NOT_ALLOWED, issue #67); sem, devolve o PNG em base64 (conteúdo image).

- Parâmetros: `path`

### `logs`

Saída do app (System.out/System.err) acumulada na sessão. `clear=true` limpa após ler.

- Parâmetros: `clear`

### `invoke`

Invoca um método static no app em execução (sonda de estado geral — o 'eval' do agente). Exceções Java voltam tipadas.

- Parâmetros: `args, class, method, sig` · obrigatórios: `class, method, sig`

### `assert_view`

Asserção de existência: procura view por resource-id (ex.: com.app:id/btn), trecho de texto ou classe. `class` casa o nome COMPLETO (android.widget.TextView) ou o nome SIMPLES exato (TextView) — `View` NÃO casa TextView. Retorna matches.

- Parâmetros: `class, resource_id, text`

### `assert_text`

Asserção de conteúdo: o texto da view com `resource_id` é igual a `expected` (ou contém, com contains=true).

- Parâmetros: `contains, expected, resource_id` · obrigatórios: `resource_id, expected`

### `assert_activity`

Asserção: a activity corrente é `activity` (com.ex.Main ou Lcom/ex/Main;).

- Parâmetros: `activity` · obrigatórios: `activity`

### `assert_finished`

Asserção: a activity corrente terminou (finish() chamado).

- Parâmetros: nenhum

### `app_info`

Identidade do app ATIVO: package, versão, SDKs, floor e componentes do manifest. `engines` registra os 28 itens do Apêndice G e detecta assinaturas/candidatos por nomes ZIP/manifest; não valida payload nem promete execução. Features selecionam contratos, não runtimes.

- Parâmetros: nenhum

### `package_list`

Lista os packages instalados NO REGISTRO da sessão (multi-app), marcando o ativo.

- Parâmetros: nenhum

### `package_remove`

Desinstala um package do registro. Se estiver EM EXECUÇÃO, o app é encerrado antes (nunca estado inconsistente). Se era o ativo, o ativo vira o último remanescente.

- Parâmetros: `package` · obrigatórios: `package`

### `package_clear`

Limpa os DADOS de um package (Android clear data): permissões runtime voltam ao estado de instalação e o app em execução é encerrado. Permanece instalado.

- Parâmetros: `package` · obrigatórios: `package`

### `permissions_list`

Permissões declaradas pelo app com o estado atual (granted/denied/runtime-pending/special).

- Parâmetros: nenhum

### `permissions_grant`

Concede uma permissão (runtime/special) na sessão.

- Parâmetros: `permission` · obrigatórios: `permission`

### `permissions_revoke`

Revoga uma permissão na sessão.

- Parâmetros: `permission` · obrigatórios: `permission`

### `permissions_audit`

Auditoria estática do manifest (exported sem guarda, debuggable, cleartext, permissões desconhecidas).

- Parâmetros: nenhum

### `device_profile`

Perfil de device corrente (piso contratual moto-e5, budgets e snapshot de periféricos virtuais M10) e controle determinístico AG-10 via `action` opcional (`get`, `location`, `sensor`, `battery`, `network`, `telephony`, `sms_receive`, `ble_advertisement`, `nfc_tag`, `camera_frame`, `biometric`, `audio_input`, `media_action`, `dark_mode`, `locale`, `font_scale`, `orientation`, `clipboard`).

- Parâmetros: `accuracy, action, address, airplane_mode, altitude, bearing, bytes_hex, call_state, camera_id, carrier_name, charging, command, enabled, enrolled, format, hardware_present, height, id_hex, label, latitude, level, locale, longitude, metered, name, network_type, online, orientation, outcome, payload_hex, pcm_hex, persist, plugged, power_save_mode, roaming, route, rssi, scale, seek_ms, sender, sensor, sensor_type, sim_state, speed, step_route, temperature_tenths_c, text, transport, uri, values, width`

### `behavior_switches`

Comutadores de comportamento targetSdk 26→36 ativos para o app instalado (PRF-07).

- Parâmetros: nenhum

### `runtime_health`

Saúde do runtime: sessão, app em execução, activity, finished, clock virtual, heap, fuel, fila do Looper, bytes de log e tamanho do registro — sem exigir app (reporta o que existe). Inclui `gc` (gerações minor/major, chaves fracas, pausas), heap peak e `jit` quando compilado com a feature jit. `gc=true` força coleta major. Fuel é cumulativo; cada tool inicia uma janela de execução limitada nova.

- Parâmetros: `gc`

### `compat_gaps`

Catálogo HONESTO das lacunas de compatibilidade do runtime v1 (notificações M9, args String/array na sonda, foco, teclas, gestures, intent implícito, transporte HTTP) + sumário executável do Apêndice C (data/api-coverage.toml: domínios/status das superfícies android.*) — o agente consulta antes de planejar.

- Parâmetros: nenhum

### `batch`

Executa uma lista de chamadas [{name, arguments}] EM ORDEM, uma vez cada, com resultados por passo. stop_on_error (default true) para no 1º erro; `resume_from` pula os N primeiros passos já executados — retomada idempotente. Cap RD_MCP_MAX_BATCH (default 64). batch não aninha. Validação de formato (name obrigatório/não vazio, sem batch aninhado, arguments objeto) rola sobre o lote INTEIRO antes de executar: passo inválido ⇒ NADA é executado (issue #80).

- Parâmetros: `calls, resume_from, stop_on_error` · obrigatórios: `calls`

### `get_api_usage`

AG-09 (metade estática): quais APIs android.* o app instalado REFERENCIA nos DEX — tipos (method_ids/field_ids/superclass/interfaces), métodos por classe, contagem por pacote e classificação Apêndice C (`api_coverage`: domínio funcional/status/gaps via data/api-coverage.toml). É análise ESTÁTICA do DEX; para coverage de EXECUÇÃO use get_coverage. Com `package` consulta outro app instalado; sem, o app ativo.

- Parâmetros: `package`

### `get_coverage`

AG-09 (metade dinâmica): cobertura de EXECUÇÃO do app em uso, nível de MÉTODO — instrumentação no interpretador registra cada corpo DEX executado. Requer app lançado (senão NOT_RUNNING); a contagem é do processo atual (reiniciar o app zera). Denominador: métodos com corpo nos DEX do app (abstract/native e classes host ficam fora).

- Parâmetros: nenhum

### `component_service`

M9: enable an APK-declared IME, accessibility, notification listener or widget provider. Runs its DEX lifecycle; returns the component object handle.

- Parâmetros: `class, role` · obrigatórios: `class, role`

### `widgets`

M9: list/allocate/bind/delete widgets or click a RemoteViews PendingIntent. Provider is an object handle returned by component_service.

- Parâmetros: `action, host, id, provider, view_id`

### `widget_screenshot`

M9: render the current RemoteViews widget through the normal deterministic PNG renderer.

- Parâmetros: `id` · obrigatórios: `id`

### `ime`

M9: inspect/focus the editor, select UTF-16 range, commit/compose text, delete surrounding code units or finish composition.

- Parâmetros: `action, after, before, codepoints, cursor, end, start, text, view`

### `accessibility`

M9: inspect visible accessibility nodes/events or perform a node action by object id. Action 16 click, 1 focus, 64 accessibility focus, 2097152 set text.

- Parâmetros: `action, action_id, node, text`

### `shortcuts`

M9: list current dynamic/pinned shortcuts or launch an enabled shortcut by id.

- Parâmetros: `action, id`

### `native_state`

M9: native feature availability, ABI, loaded ELF libraries and guest instruction/memory accounting. Never claims a library ran merely because it was loaded.

- Parâmetros: nenhum

### `notifications`

M9: list active notifications/channels or click/cancel a notification (tag/id and optional action index). Uses the app NotificationManager state.

- Parâmetros: `action, action_index, id, tag`

### `observe`

See the screen WITHOUT a screenshot: semantic model of the current frame derived from the recorded display list (works for Views, Compose and custom canvases) merged with the host view tree. Returns visible texts with bounds, probable tap targets (label + center x,y; source frame|view), images and a frame `version` that only changes when visible content changes, plus a diff (+added/-removed texts) since the previous observe. format=text (default, compact, LLM-friendly) or json (structured; boxes=true adds drawn boxes). ~10-100x cheaper than screenshot (no rasterize/PNG). Also available as subscribable MCP resources rustydroid://screen and rustydroid://screen.txt.

- Parâmetros: `boxes, diff, format`

### `tap_text`

Tap by visible label instead of coordinates: finds the tap target (or text) whose label matches `text` on the current frame (case/space-insensitive substring; exact=true for equality; nth picks among matches in reading order) and taps its center. Error VIEW_NOT_FOUND lists the visible texts. Works on Compose UIs (labels come from the drawn frame).

- Parâmetros: `exact, nth, observe, text` · obrigatórios: `text`

### `find_views`

Find structured host views by AND-combined selectors. Text is exact by default; contains=true opts into substring. Reports bounded matches, total, truncation, xywh bounds and an opaque session/revision token. Snapshots visit at most 10000 nodes; returned text is a UTF-8 prefix of at most 4096 bytes with explicit text_bytes/text_truncated. Tokens expire after potentially mutating tools, including waits and failed attempts. Not a Canvas/Compose query: use observe for those.

- Parâmetros: `class, clickable, contains, enabled, limit, resource_id, text, visible`

### `tap_view`

Tap a uniquely matched visible enabled host view, rejecting ambiguity, incomplete trees, stale expected_token and occluded centers before injecting input. Coordinates are clipped to the 720x1440 viewport. Does not claim listener success when none fired.

- Parâmetros: `class, clickable, contains, enabled, expected_token, observe, resource_id, text, visible`

### `wait_for_view`

Wait for a host selector to become present or absent, polling virtual time under existing wait/fuel budgets. A timeout is condition_met=false, never a successful assertion. Limited to 1000 time steps. Incomplete trees cannot prove absence.

- Parâmetros: `class, clickable, contains, enabled, observe, resource_id, state, step_ms, text, timeout_ms, visible`

### `session_info`

Inspect session identity, active versus running package, compiled capabilities and policy limits without launching an app or collecting garbage. Sandbox roots are reported only as a count, never paths. This complements runtime_health; compiled capability flags do not promise runtime availability.

- Parâmetros: nenhum

### `logs_tail`

Read at most max_bytes raw log bytes (default 4096, cap 65536). Without cursor, returns the newest bytes; next_cursor supports lossless incremental reads. Cursors bind this session, a clear-generation and byte offset; logs(clear=true) invalidates old cursors even after the buffer regrows. data_b64 is authoritative across UTF-8 boundaries; text is explicitly lossy when needed. Read-only; does not clear or cap the underlying legacy log buffer.

- Parâmetros: `cursor, max_bytes`

### `last_error`

Inspect the latest typed tool failure, including early validation failures, batch child failures and isolated runtime panics. Retains bounded UTF-8 error context, not input arguments; does not separately capture logs or stack traces. Original error messages may contain app-provided text. Successful tools do not erase it. clear=true clears only the snapshot, not the cumulative error count. Protocol parse/method errors are not tool failures.

- Parâmetros: `clear`
