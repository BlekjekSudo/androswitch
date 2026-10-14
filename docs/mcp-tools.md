<!-- GERADO: `rd mcp --gen-docs docs/mcp-tools.md` — NÃO editar à mão. -->

# Ferramentas do Agente MCP (M5)

Servidor `rustydroid-agent` v0.1.0 · protocolo `2024-11-05` · transporte stdio (JSON-RPC 2.0, uma mensagem por linha).

Total: **32 tools**. Este catálogo é gerado de `rd_agent::mcp::tool_defs()` — o MESMO registro respondido em `tools/list` e impresso por `rd mcp --describe-tools`.

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

- Parâmetros: `activity, package`

### `tap`

Toque simulado nas coordenadas (px do viewport 720×1440). Retorna se algum listener foi acionado.

- Parâmetros: `x, y` · obrigatórios: `x, y`

### `swipe`

Gesto de arraste determinístico (x1,y1)→(x2,y2) em `ms` (cap RD_MCP_MAX_WAIT_MS). Reporta as views atravessadas em ordem; down+up na MESMA view vira clique (listener disparado) — semântica Android tap vs scroll.

- Parâmetros: `ms, x1, x2, y1, y2` · obrigatórios: `x1, y1, x2, y2`

### `type_text`

Digita texto na primeira EditText visível+habilitada da activity (modelo v1 sem foco real — issue #54). `replace=true` substitui; default anexa. Devolve o texto final.

- Parâmetros: `replace, text` · obrigatórios: `text`

### `key`

Evento de tecla: BACK (finaliza a activity corrente), DEL (apaga último caractere da EditText), ENTER (entregue, sem IME action no v1). Outras teclas: erro tipado NOT_IMPLEMENTED com a lista suportada.

- Parâmetros: `key` · obrigatórios: `key`

### `wait`

Avança o clock virtual (ms) e executa os runnables do Looper com deadline vencida. Retorna quantos rodaram. Cap RD_MCP_MAX_WAIT_MS (default 60_000) — contra tempo ilimitado.

- Parâmetros: `ms` · obrigatórios: `ms`

### `wait_for`

Espera CONDICIONAL: avança o clock virtual em passos de `step_ms` (default 100) até a view (resource_id/text/class) existir ou `timeout_ms` vencer (cap RD_MCP_MAX_WAIT_MS). `class` casa o nome completo ou o nome simples exato (issue #78). found=false é resposta honesta (não é erro) — o agente decide.

- Parâmetros: `class, resource_id, step_ms, text, timeout_ms` · obrigatórios: `timeout_ms`

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

Perfil de device corrente (piso contratual moto-e5) com os budgets de hardware.

- Parâmetros: nenhum

### `behavior_switches`

Comutadores de comportamento targetSdk 26→36 ativos para o app instalado (PRF-07).

- Parâmetros: nenhum

### `runtime_health`

Saúde do runtime: sessão, app em execução, activity, finished, clock virtual, heap, fuel, fila do Looper, bytes de log e tamanho do registro — sem exigir app (reporta o que existe).

- Parâmetros: nenhum

### `compat_gaps`

Catálogo HONESTO das lacunas de compatibilidade do runtime v1 (notificações M9, args String/array na sonda, foco, teclas, gestures, intent implícito, transporte HTTP) — o agente consulta antes de planejar.

- Parâmetros: nenhum

### `batch`

Executa uma lista de chamadas [{name, arguments}] EM ORDEM, uma vez cada, com resultados por passo. stop_on_error (default true) para no 1º erro; `resume_from` pula os N primeiros passos já executados — retomada idempotente. Cap RD_MCP_MAX_BATCH (default 64). batch não aninha. Validação de formato (name obrigatório/não vazio, sem batch aninhado, arguments objeto) rola sobre o lote INTEIRO antes de executar: passo inválido ⇒ NADA é executado (issue #80).

- Parâmetros: `calls, resume_from, stop_on_error` · obrigatórios: `calls`

### `get_api_usage`

AG-09 (metade estática): quais APIs android.* o app instalado REFERENCIA nos DEX — tipos (method_ids/field_ids/superclass/interfaces), métodos por classe e contagem por domínio (android.app, android.view…). É análise ESTÁTICA do DEX; para coverage de EXECUÇÃO use get_coverage. Com `package` consulta outro app instalado; sem, o app ativo.

- Parâmetros: `package`

### `get_coverage`

AG-09 (metade dinâmica): cobertura de EXECUÇÃO do app em uso, nível de MÉTODO — instrumentação no interpretador registra cada corpo DEX executado. Requer app lançado (senão NOT_RUNNING); a contagem é do processo atual (reiniciar o app zera). Denominador: métodos com corpo nos DEX do app (abstract/native e classes host ficam fora).

- Parâmetros: nenhum

### `notifications`

Notificações do app. FORA DO ESCOPO do M5 (entram no M9 com widgets/IME) — responde erro tipado NOT_IMPLEMENTED (honesto).

- Parâmetros: nenhum

