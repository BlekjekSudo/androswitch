# Suítes de teste — formato e execução (`rd test`)

> M6 — TST-01 do checklist: `rd test suite.json` → resumo humano + **JUnit XML** + exit code.
> Implementação: `crates/rd-agent/src/suite.rs` (núcleo) e `rd test` em `crates/rd-cli`.

## Uso

```bash
rd test tests/suites/contract-no-app.json                 # resumo humano
rd test suite.json --junit out/junit.xml                  # + relatório JUnit XML
rd test suite.json --json                                 # resumo em JSON (stdout)
```

**Códigos de saída**

| exit | significado |
|---|---|
| `0` | todos os cenários passaram |
| `1` | pelo menos um cenário falhou (o JUnit traz o detalhe) |
| `64` | suíte inválida ou arquivo ilegível — **nenhum passo foi executado** |

## Formato

```json
{
  "name": "smoke",
  "scenarios": [
    {
      "name": "abre o app e confere o texto",
      "steps": [
        { "name": "install", "arguments": { "apk": "apps/app.apk" } },
        { "name": "launch",  "arguments": {} },
        { "name": "wait",    "arguments": { "ms": 50 } },
        { "name": "assert_text",
          "arguments": { "resource_id": "com.test:id/tv", "expected": "Olá" } }
      ]
    },
    {
      "name": "lançar sem APK responde NO_APP",
      "steps": [
        { "name": "launch", "arguments": {}, "expect": "error:NO_APP" }
      ]
    }
  ]
}
```

- `name` (suíte): obrigatório, não vazio.
- `scenarios`: lista obrigatória, com pelo menos um cenário.
- Cenário: `name` e `steps` (pelo menos um passo).
- Passo: `name` = nome de uma tool MCP (ver `docs/mcp-tools.md`); `arguments` = objeto (opcional, padrão `{}`); `expect` (opcional, padrão `"ok"`); `timeout_ms` (opcional, inteiro > 0 — orçamento de wall-clock do passo).

## Regras de execução

1. **Validação antes de executar.** Qualquer erro de formato (campo ausente, `arguments` não objeto, `expect` inválido, `batch` dentro de suíte) aborta com `INVALID_SUITE` e exit `64`. Nenhum passo roda.
2. **Isolamento.** Cada cenário roda em uma sessão nova do agente (um app por sessão — ADR 001). Um cenário não herda o APK instalado por outro.
3. **Passo `ok` (padrão)** passa quando a tool completa **e** o resultado não diz que a condição falhou:
   - `isError = true` → falha, com o código e a causa devolvidos pela tool.
   - `"ok": false` ou `"found": false` no resultado → **falha**.
   - `assert_finished` devolve `{"finished": bool}`: `"finished": false` → **falha**. As asserções do MCP (`assert_text`, `assert_view`, `assert_activity`, `wait_for`…) devolvem sucesso na chamada com `ok:false` quando a condição não se confirma; o executor trata isso como falha. Sem essa regra, toda asserção errada passaria em verde.
4. **Passo `"expect": "error:CODIGO"`** passa somente quando a tool falha com exatamente aquele código estável (ex.: `NO_APP`, `NOT_RUNNING`, `INVALID_FORMAT`). Se a tool completar, ou falhar com outro código, o passo falha.
5. **Primeira falha encerra o cenário.** Os passos seguintes são marcados `SKIP` no JUnit. Os outros cenários continuam.
6. **Caminhos relativos.** `install.apk` relativo é resolvido a partir da pasta do arquivo de suíte, não do diretório atual. Caminhos com `..` não são reescritos (a policy de sandbox do agente decide).
7. **`timeout_ms` por passo.** Se presente, o passo tem orçamento de wall-clock: a chamada da tool roda até o fim (não há preempção) e, se o tempo medido exceder o teto, o passo falha com o código estável **`STEP_TIMEOUT`** e os passos seguintes são pulados. Vale mesmo com `expect: "error:…"` — o orçamento é contrato do passo, não da tool. `timeout_ms: 0`, negativo ou não numérico é `INVALID_SUITE` (validação antes de executar).

## Saída

**Resumo humano**

```
Suíte: smoke (2 cenário(s))
  PASS  abre o app e confere o texto (4 passo(s), 41 ms)
  FAIL  lançar sem APK responde NO_APP (1 passo(s), 3 ms)
        → passo 0 (launch): esperado erro NO_APP, mas a tool completou sem erro
Resultado: 1 passou, 1 falhou — 46 ms
```

**JUnit XML** — um `<testcase>` por cenário. Falha vira `<failure message="…">`. O `<system-out>` traz cada passo com status, tempo e o resultado cru da tool, para auditoria (a mesma filosofia de "saída literal" do projeto).

```xml
<testsuites name="rustydroid" tests="2" failures="1" errors="0" time="0.046">
  <testsuite name="smoke" tests="2" failures="1" ...>
    <testcase classname="smoke" name="abre o app e confere o texto" time="0.041">
      <system-out>[0] install PASS (…) …</system-out>
    </testcase>
    <testcase classname="smoke" name="lançar sem APK responde NO_APP" time="0.003">
      <failure message="passo 0 (launch): …" type="ScenarioFailed">…</failure>
      <system-out>…</system-out>
    </testcase>
  </testsuite>
</testsuites>
```

## Limites atuais (M6)

- Não há coverage (AG-09) nem `run_scenario` com retries e hooks (AG-07) — ficam para as próximas rodadas.
- `timeout` por passo não existe ainda; use `wait_for` com `ms` explícito.
- Suítes que exigem APK precisam que o APK esteja no disco no momento da execução (ex.: baixado por `golden-apks/fetch.sh`, que não é versionado — GH-05).
- A suíte é executada pelo binário `rd`; não há driver em outra linguagem.

## Arquivos versionados

- `tests/suites/contract-no-app.json` — contratos de erro e de estado que rodam **sem APK** (o CI executa esta suíte).
- `docs/compat-matrix.md` lista as suítes em `tests/suites/` automaticamente (`cargo xtask compat-report`).
