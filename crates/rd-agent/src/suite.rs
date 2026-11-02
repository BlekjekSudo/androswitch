//! M6 — executor de testes: suíte JSON → relatório humano + JUnit XML (TST-01).
//!
//! Formato da suíte (ver `docs/suite-format.md`):
//!
//! ```json
//! {
//!   "name": "smoke",
//!   "scenarios": [
//!     { "name": "abre e confere texto",
//!       "steps": [
//!         { "name": "install", "arguments": { "apk": "app.apk" } },
//!         { "name": "launch",  "arguments": {} },
//!         { "name": "assert_text", "arguments": { "resource_id": "com.x:id/tv", "expected": "Olá" } },
//!         { "name": "tap", "arguments": { "x": 1, "y": 1 }, "expect": "error:INVALID_FORMAT" }
//!       ] }
//!   ]
//! }
//! ```
//!
//! Regras (Lei 1 — sem falha silenciosa):
//! - Cada cenário roda numa `AgentSession` NOVA (isolamento; 1 app por sessão — ADR 001).
//! - Um passo passa se `isError == false` (padrão `"expect": "ok"`), ou se falha com o
//!   código exato em `"expect": "error:CODE"`.
//! - A primeira falha encerra o cenário; os passos restantes são marcados como `skipped`.
//! - Formato inválido da suíte → `INVALID_SUITE` ANTES de executar qualquer passo.
//! - `install.apk` relativo é resolvido a partir da pasta da suíte (não do cwd).
//! - `batch` não é aninhado dentro de suíte (mesma regra do próprio batch).
//! - `timeout_ms` (opcional, > 0) dá orçamento de wall-clock ao passo: estourou
//!   → falha `STEP_TIMEOUT` (medição, sem preempção) e resto do cenário pulado.
//! - `retries` (opcional, ≥ 0, AG-07): tentativas extras após falha; a primeira
//!   tentativa que passa encerra; cada tentativa é julgada de novo.
//! - Hooks (AG-07): `setup` roda antes dos passos (falha aborta o cenário) e
//!   `teardown` roda depois mesmo com passos falhos (falha reprova o cenário).

use std::path::{Component, Path, PathBuf};
use std::time::Instant;

use rd_apk::RdError;
use serde_json::Value;

use crate::mcp::{AgentSession, ToolContent};
use crate::MODULE_ID;

/// Código de erro estável para suíte mal formada.
pub const INVALID_SUITE: &str = "INVALID_SUITE";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expect {
    /// O passo deve completar sem erro (padrão).
    Ok,
    /// O passo deve falhar com exatamente este código estável.
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub name: String,
    pub arguments: Value,
    pub expect: Expect,
    /// Orçamento de wall-clock do passo (adiamento M6). `None` = sem teto.
    /// Medido, não preemptivo: a chamada roda até o fim e o executor reprova
    /// o passo se o teto estourar (STEP_TIMEOUT) — ver docs/suite-format.md.
    pub timeout_ms: Option<u64>,
    /// Tentativas EXTRAS após falha (AG-07). `0` = sem retry (padrão).
    /// Cada tentativa é julgada de novo (expect + timeout_ms); a primeira
    /// que passa encerra o retry.
    pub retries: u32,
}

/// Fase do passo no cenário (AG-07 — hooks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Setup,
    Step,
    Teardown,
}

impl Phase {
    /// Rótulo humano usado nas mensagens de falha e no log do JUnit.
    pub fn label(self) -> &'static str {
        match self {
            Phase::Setup => "setup",
            Phase::Step => "passo",
            Phase::Teardown => "teardown",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scenario {
    pub name: String,
    /// Hook de abertura (AG-07): roda antes dos passos; falha aqui aborta o
    /// cenário (passos e teardown são pulados).
    pub setup: Vec<Step>,
    pub steps: Vec<Step>,
    /// Hook de fechamento (AG-07): roda mesmo se os passos falharem
    /// (limpeza garantida); falha aqui reprova o cenário.
    pub teardown: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Suite {
    pub name: String,
    pub scenarios: Vec<Scenario>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StepStatus {
    Passed,
    Failed(String),
    Skipped,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StepOutcome {
    pub index: usize,
    /// Fase do passo (AG-07): setup, passo normal ou teardown.
    pub phase: Phase,
    pub tool: String,
    pub status: StepStatus,
    pub elapsed_ms: u128,
    /// Resultado cru da tool (texto), para o log do JUnit.
    pub output: String,
    /// Tentativas executadas (AG-07): 1 sem retry; `0` se pulado.
    pub attempts: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScenarioOutcome {
    pub name: String,
    pub passed: bool,
    pub steps: Vec<StepOutcome>,
    pub elapsed_ms: u128,
    /// Primeira falha, em texto humano (vazio se passou).
    pub failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SuiteReport {
    pub name: String,
    pub scenarios: Vec<ScenarioOutcome>,
    pub elapsed_ms: u128,
}

impl SuiteReport {
    pub fn passed_count(&self) -> usize {
        self.scenarios.iter().filter(|s| s.passed).count()
    }
    pub fn failed_count(&self) -> usize {
        self.scenarios.len() - self.passed_count()
    }
    pub fn all_passed(&self) -> bool {
        self.failed_count() == 0
    }
    /// Código de saída do CLI: 0 tudo passou · 1 houve falha.
    /// (2 é reservado para suíte inválida — ver `INVALID_SUITE`.)
    pub fn exit_code(&self) -> i32 {
        if self.all_passed() {
            0
        } else {
            1
        }
    }
}

/// Sugestão para um arquivo de matriz de compatibilidade passado a `rd test` (#368).
const MATRIX_HINT: &str = "parece uma matriz de compatibilidade (\"suites\"), que não é uma suíte de cenários; para rd test use uma suíte {\"name\", \"scenarios\"} (ex.: tests/suites/m8-compat.json)";

fn invalid(cause: impl Into<String>, suggestion: &str) -> RdError {
    RdError::new(INVALID_SUITE, cause, MODULE_ID).with_suggestion(suggestion)
}

/// Lê e valida a suíte. Nenhum passo é executado aqui.
pub fn parse_suite(text: &str) -> Result<Suite, RdError> {
    let v: Value = serde_json::from_str(text).map_err(|e| {
        invalid(
            format!("JSON inválido: {e}"),
            "a suíte precisa ser um objeto JSON",
        )
    })?;
    let obj = v.as_object().ok_or_else(|| {
        invalid(
            "a raiz da suíte não é um objeto",
            "use {\"name\": …, \"scenarios\": [...]}",
        )
    })?;
    let name = match obj
        .get("name")
        .and_then(|n| n.as_str())
        .filter(|n| !n.trim().is_empty())
    {
        Some(n) => n.to_string(),
        None => {
            // A compatibility matrix (`{"schema_version", "suites": [...]}`) is not a
            // scenario suite; point the user at the right runner (#368).
            let hint = if obj.contains_key("suites") {
                MATRIX_HINT
            } else {
                "dê um nome à suíte"
            };
            return Err(invalid("campo \"name\" ausente ou vazio", hint));
        }
    };
    let scen_arr = obj
        .get("scenarios")
        .and_then(|s| s.as_array())
        .ok_or_else(|| {
            invalid(
                "campo \"scenarios\" ausente ou não é lista",
                "scenarios é uma lista de cenários",
            )
        })?;
    if scen_arr.is_empty() {
        return Err(invalid(
            "suíte sem cenários",
            "adicione ao menos um cenário em \"scenarios\"",
        ));
    }

    let mut scenarios = Vec::with_capacity(scen_arr.len());
    for (si, sv) in scen_arr.iter().enumerate() {
        let so = sv.as_object().ok_or_else(|| {
            invalid(
                format!("cenário {si} não é objeto"),
                "cada cenário é um objeto",
            )
        })?;
        let sname = so
            .get("name")
            .and_then(|n| n.as_str())
            .filter(|n| !n.trim().is_empty())
            .ok_or_else(|| {
                invalid(
                    format!("cenário {si} sem \"name\""),
                    "dê nome a cada cenário",
                )
            })?
            .to_string();
        let steps_arr = so.get("steps").and_then(|s| s.as_array()).ok_or_else(|| {
            invalid(
                format!("cenário {sname:?} sem lista \"steps\""),
                "steps é uma lista de passos",
            )
        })?;
        if steps_arr.is_empty() {
            return Err(invalid(
                format!("cenário {sname:?} sem passos"),
                "todo cenário precisa de ao menos um passo",
            ));
        }
        let mut steps = Vec::with_capacity(steps_arr.len());
        for (pi, pv) in steps_arr.iter().enumerate() {
            steps.push(parse_step(&sname, "passo", pi, pv)?);
        }
        // hooks (AG-07): opcionais, mesma gramática dos passos
        let setup = parse_hook(&sname, "setup", so.get("setup"))?;
        let teardown = parse_hook(&sname, "teardown", so.get("teardown"))?;
        scenarios.push(Scenario {
            name: sname,
            setup,
            steps,
            teardown,
        });
    }
    Ok(Suite { name, scenarios })
}

/// Parse de um hook (`setup`/`teardown`): ausente → vazio; presente precisa
/// ser lista de passos (pode ser vazia).
fn parse_hook(scenario: &str, phase: &str, v: Option<&Value>) -> Result<Vec<Step>, RdError> {
    match v {
        None => Ok(Vec::new()),
        Some(arr) => {
            let arr = arr.as_array().ok_or_else(|| {
                invalid(
                    format!("cenário {scenario:?}: {phase:?} não é lista"),
                    &format!("{phase} é uma lista de passos (ou omita o campo)"),
                )
            })?;
            arr.iter()
                .enumerate()
                .map(|(pi, pv)| parse_step(scenario, phase, pi, pv))
                .collect()
        }
    }
}

fn parse_step(scenario: &str, phase: &str, pi: usize, pv: &Value) -> Result<Step, RdError> {
    let po = pv.as_object().ok_or_else(|| {
        invalid(
            format!("{scenario:?} {phase} {pi} não é objeto"),
            "cada passo é {\"name\": …, \"arguments\": {…}}",
        )
    })?;
    let name = po
        .get("name")
        .and_then(|n| n.as_str())
        .filter(|n| !n.trim().is_empty())
        .ok_or_else(|| {
            invalid(
                format!("{scenario:?} {phase} {pi} sem \"name\""),
                "informe o nome da tool",
            )
        })?
        .to_string();
    if name == "batch" {
        return Err(invalid(
            format!("{scenario:?} {phase} {pi}: batch não pode ser usado dentro de suíte"),
            "chame as tools diretamente como passos",
        ));
    }
    let arguments = match po.get("arguments") {
        None => Value::Object(Default::default()),
        Some(a) if a.is_object() => a.clone(),
        Some(_) => {
            return Err(invalid(
                format!("{scenario:?} {phase} {pi} ({name}): \"arguments\" precisa ser objeto"),
                "use {\"arguments\": {…}} ou omita o campo",
            ))
        }
    };
    let expect = match po.get("expect") {
        None => Expect::Ok,
        Some(e) => parse_expect(e).ok_or_else(|| {
            invalid(
                format!("{scenario:?} {phase} {pi} ({name}): \"expect\" inválido: {e}"),
                "use \"ok\" ou \"error:CODIGO_ESTAVEL\" (ex.: \"error:INVALID_FORMAT\")",
            )
        })?,
    };
    let timeout_ms = match po.get("timeout_ms") {
        None => None,
        Some(t) => {
            let ms = t.as_u64().filter(|v| *v > 0).ok_or_else(|| {
                invalid(
                    format!("{scenario:?} {phase} {pi} ({name}): \"timeout_ms\" inválido: {t}"),
                    "use um inteiro > 0 (milissegundos) ou omita o campo",
                )
            })?;
            Some(ms)
        }
    };
    // AG-07: retries — tentativas extras após falha (0 = padrão)
    let retries = match po.get("retries") {
        None => 0,
        Some(r) => r
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| {
                invalid(
                    format!("{scenario:?} {phase} {pi} ({name}): \"retries\" inválido: {r}"),
                    "use um inteiro ≥ 0 (tentativas extras) ou omita o campo",
                )
            })?,
    };
    Ok(Step {
        name,
        arguments,
        expect,
        timeout_ms,
        retries,
    })
}

fn parse_expect(e: &Value) -> Option<Expect> {
    let s = e.as_str()?.trim();
    if s == "ok" {
        return Some(Expect::Ok);
    }
    let code = s.strip_prefix("error:")?.trim();
    let valid = !code.is_empty()
        && code
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    valid.then(|| Expect::Error(code.to_string()))
}

/// Resolve `install.apk` relativo contra a pasta da suíte.
fn resolve_args(tool: &str, args: &Value, base: &Path) -> Value {
    let mut out = args.clone();
    if tool == "install" {
        if let Some(p) = args.get("apk").and_then(|v| v.as_str()) {
            let path = PathBuf::from(p);
            if path.is_relative() && !path.components().any(|c| matches!(c, Component::ParentDir)) {
                out["apk"] = Value::String(base.join(path).to_string_lossy().into_owned());
            }
        }
    }
    out
}

/// Falha "suave": a tool completou, mas o resultado diz que a condição não se confirmou.
/// Regra documentada em docs/suite-format.md: `"ok": false` ou `"found": false`.
fn soft_failure(tool: &str, parsed: &Value) -> Option<String> {
    // `assert_finished` não usa "ok": a asserção é "a activity terminou",
    // então `finished: false` reprova. Sem esta regra o passo passaria sempre.
    let finished_false =
        tool == "assert_finished" && parsed.get("finished") == Some(&Value::Bool(false));
    let negative = finished_false
        || parsed.get("ok") == Some(&Value::Bool(false))
        || parsed.get("found") == Some(&Value::Bool(false));
    if !negative {
        return None;
    }
    let compact = parsed.to_string();
    let shown: String = compact.chars().take(300).collect();
    Some(format!("resultado {shown}"))
}

/// Uma tentativa do passo: chama a tool e julga o resultado (expect +
/// timeout_ms). Devolve status, texto bruto e tempo da tentativa.
fn attempt_step(
    session: &mut AgentSession,
    step: &Step,
    base: &Path,
    phase: Phase,
    i: usize,
) -> (StepStatus, String, u128) {
    let label = phase.label();
    let t0 = Instant::now();
    let args = resolve_args(&step.name, &step.arguments, base);
    // Issue #317: o CLI (`rd test`) também isola — panic de runtime
    // vira passo falho tipado (INTERNAL_PANIC), não crash do processo.
    let r = session.call_tool_isolated(&step.name, &args);
    let text = match r.content.first() {
        Some(ToolContent::Text(t)) => t.clone(),
        Some(ToolContent::Image { mime, .. }) => format!("<imagem {mime}>"),
        None => String::new(),
    };
    let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let code = parsed
        .get("code")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    let cause = parsed
        .get("cause")
        .and_then(|c| c.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| text.clone());

    let soft = if r.is_error {
        None
    } else {
        soft_failure(&step.name, &parsed)
    };
    let elapsed_ms = t0.elapsed().as_millis();
    // adiado M6: orçamento de wall-clock por passo. É medição, não
    // preempção — a chamada completou, mas estourou o teto: o passo é
    // reprovado com código estável STEP_TIMEOUT (vale mesmo com `expect:
    // error:…`, pois o orçamento é contrato do passo, não da tool).
    let timed_out = step.timeout_ms.is_some_and(|t| elapsed_ms > u128::from(t));
    let status = if timed_out {
        StepStatus::Failed(format!(
            "{label} {i} ({}): STEP_TIMEOUT: {elapsed_ms} ms excedeu timeout_ms={} (a chamada completou, mas estourou o orçamento do passo)",
            step.name,
            step.timeout_ms.unwrap_or(0)
        ))
    } else {
        match (&step.expect, r.is_error) {
            // asserções do MCP respondem isError=false com "ok": false (ou
            // "found": false) quando a condição NÃO se confirma. Para um
            // executor de testes isso é falha — senão toda asserção errada
            // passaria em verde.
            (Expect::Ok, false) if soft.is_some() => StepStatus::Failed(format!(
                "{label} {i} ({}): asserção falhou: {}",
                step.name,
                soft.clone().unwrap_or_default()
            )),
            (Expect::Ok, false) => StepStatus::Passed,
            (Expect::Ok, true) => StepStatus::Failed(format!(
                "{label} {i} ({}): esperado ok, obtido erro {}: {}",
                step.name,
                if code.is_empty() { "?" } else { &code },
                cause
            )),
            (Expect::Error(want), true) if *want == code => StepStatus::Passed,
            (Expect::Error(want), true) => StepStatus::Failed(format!(
                "{label} {i} ({}): esperado erro {want}, obtido {}: {}",
                step.name,
                if code.is_empty() { "?" } else { &code },
                cause
            )),
            (Expect::Error(want), false) => StepStatus::Failed(format!(
                "{label} {i} ({}): esperado erro {want}, mas a tool completou sem erro",
                step.name
            )),
        }
    };
    (status, text, elapsed_ms)
}

/// Executa um passo com retries (AG-07): até `1 + retries` tentativas; a
/// primeira que passa encerra. `elapsed_ms` é a soma das tentativas;
/// `output`/mensagem de falha são da última tentativa executada.
fn run_step(
    session: &mut AgentSession,
    step: &Step,
    base: &Path,
    phase: Phase,
    i: usize,
) -> StepOutcome {
    let mut total_ms: u128 = 0;
    let mut attempts: u32 = 0;
    let last;
    loop {
        attempts += 1;
        let (status, text, ms) = attempt_step(session, step, base, phase, i);
        total_ms += ms;
        let passed = matches!(status, StepStatus::Passed);
        if passed || u64::from(attempts) > u64::from(step.retries) {
            last = (status, text);
            break;
        }
    }
    StepOutcome {
        index: i,
        phase,
        tool: step.name.clone(),
        status: last.0,
        elapsed_ms: total_ms,
        output: last.1,
        attempts,
    }
}

fn skipped(phase: Phase, i: usize, step: &Step) -> StepOutcome {
    StepOutcome {
        index: i,
        phase,
        tool: step.name.clone(),
        status: StepStatus::Skipped,
        elapsed_ms: 0,
        output: String::new(),
        attempts: 0,
    }
}

/// Executa um cenário numa sessão nova: `setup` → passos → `teardown`.
/// Primeira falha em setup/passos pula o resto da fase; teardown roda mesmo
/// com passos falhos (limpeza garantida, best-effort) e só é pulado se o
/// próprio setup falhou.
pub fn run_scenario(scenario: &Scenario, base: &Path) -> ScenarioOutcome {
    let started = Instant::now();
    let mut session = AgentSession::new();
    let mut steps =
        Vec::with_capacity(scenario.setup.len() + scenario.steps.len() + scenario.teardown.len());
    let mut failure: Option<String> = None;

    // setup (AG-07): falha aqui aborta o cenário inteiro
    for (i, step) in scenario.setup.iter().enumerate() {
        if failure.is_some() {
            steps.push(skipped(Phase::Setup, i, step));
            continue;
        }
        let out = run_step(&mut session, step, base, Phase::Setup, i);
        if let StepStatus::Failed(msg) = &out.status {
            failure = Some(msg.clone());
        }
        steps.push(out);
    }
    let setup_ok = failure.is_none();

    // passos: primeira falha encerra a fase
    for (i, step) in scenario.steps.iter().enumerate() {
        if !setup_ok || failure.is_some() {
            steps.push(skipped(Phase::Step, i, step));
            continue;
        }
        let out = run_step(&mut session, step, base, Phase::Step, i);
        if let StepStatus::Failed(msg) = &out.status {
            failure = Some(msg.clone());
        }
        steps.push(out);
    }

    // teardown (AG-07): roda mesmo com passos falhos; cada passo de teardown
    // é executado (best-effort — um não pula os outros); falha aqui reprova
    // o cenário se ele ainda não falhou.
    for (i, step) in scenario.teardown.iter().enumerate() {
        if !setup_ok {
            steps.push(skipped(Phase::Teardown, i, step));
            continue;
        }
        let out = run_step(&mut session, step, base, Phase::Teardown, i);
        if let StepStatus::Failed(msg) = &out.status {
            if failure.is_none() {
                failure = Some(msg.clone());
            }
        }
        steps.push(out);
    }

    ScenarioOutcome {
        name: scenario.name.clone(),
        passed: failure.is_none(),
        steps,
        elapsed_ms: started.elapsed().as_millis(),
        failure,
    }
}

/// Executa a suíte inteira. `base` é a pasta da suíte (resolve caminhos relativos).
pub fn run_suite(suite: &Suite, base: &Path) -> SuiteReport {
    let started = Instant::now();
    let scenarios = suite
        .scenarios
        .iter()
        .map(|s| run_scenario(s, base))
        .collect();
    SuiteReport {
        name: suite.name.clone(),
        scenarios,
        elapsed_ms: started.elapsed().as_millis(),
    }
}

/// Escapa texto para atributo/conteúdo XML.
pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // caracteres de controle não são permitidos em XML 1.0
            c if (c as u32) < 0x20 && c != '\n' && c != '\t' && c != '\r' => out.push('\u{FFFD}'),
            c => out.push(c),
        }
    }
    out
}

/// JUnit XML (um `<testcase>` por cenário; passos no `<system-out>`).
pub fn to_junit_xml(report: &SuiteReport) -> String {
    let secs = |ms: u128| format!("{:.3}", ms as f64 / 1000.0);
    let failures = report.failed_count();
    let mut x = String::new();
    x.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    x.push_str(&format!(
        "<testsuites name=\"rustydroid\" tests=\"{}\" failures=\"{}\" errors=\"0\" time=\"{}\">\n",
        report.scenarios.len(),
        failures,
        secs(report.elapsed_ms)
    ));
    x.push_str(&format!(
        "  <testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" errors=\"0\" skipped=\"0\" time=\"{}\">\n",
        xml_escape(&report.name),
        report.scenarios.len(),
        failures,
        secs(report.elapsed_ms)
    ));
    for sc in &report.scenarios {
        x.push_str(&format!(
            "    <testcase classname=\"{}\" name=\"{}\" time=\"{}\"",
            xml_escape(&report.name),
            xml_escape(&sc.name),
            secs(sc.elapsed_ms)
        ));
        let mut log = String::new();
        for st in &sc.steps {
            let tag = match &st.status {
                StepStatus::Passed => "PASS".to_string(),
                StepStatus::Failed(m) => format!("FAIL {m}"),
                StepStatus::Skipped => "SKIP".to_string(),
            };
            // AG-07: passos de hook ganham prefixo de fase; retries > 1 são
            // anotados. O formato do passo normal ([i] tool …) não muda.
            let prefix = match st.phase {
                Phase::Step => format!("{}", st.index),
                p => format!("{} {}", p.label(), st.index),
            };
            let tries = if st.attempts > 1 {
                format!(", {} tentativas", st.attempts)
            } else {
                String::new()
            };
            log.push_str(&format!(
                "[{prefix}] {} {} ({} ms{tries})\n",
                st.tool, tag, st.elapsed_ms
            ));
            if !st.output.is_empty() {
                log.push_str(&st.output);
                log.push('\n');
            }
        }
        if sc.passed {
            x.push_str(">\n");
        } else {
            let msg = sc.failure.clone().unwrap_or_default();
            x.push_str(">\n");
            x.push_str(&format!(
                "      <failure message=\"{}\" type=\"ScenarioFailed\">{}</failure>\n",
                xml_escape(&msg),
                xml_escape(&msg)
            ));
        }
        x.push_str(&format!(
            "      <system-out>{}</system-out>\n    </testcase>\n",
            xml_escape(&log)
        ));
    }
    x.push_str("  </testsuite>\n</testsuites>\n");
    x
}

/// Resumo humano para o terminal.
pub fn to_human(report: &SuiteReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Suíte: {} ({} cenário(s))\n",
        report.name,
        report.scenarios.len()
    ));
    for sc in &report.scenarios {
        let tag = if sc.passed { "PASS" } else { "FAIL" };
        out.push_str(&format!(
            "  {tag}  {} ({} passo(s), {} ms)\n",
            sc.name,
            sc.steps.len(),
            sc.elapsed_ms
        ));
        if let Some(f) = &sc.failure {
            out.push_str(&format!("        → {f}\n"));
        }
    }
    out.push_str(&format!(
        "Resultado: {} passou, {} falhou — {} ms\n",
        report.passed_count(),
        report.failed_count(),
        report.elapsed_ms
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK_SUITE: &str = r#"{
        "name": "demo",
        "scenarios": [
          {"name": "a", "steps": [
             {"name": "device_profile", "arguments": {}},
             {"name": "tap", "arguments": {"x": 1, "y": 1}, "expect": "error:NO_APP"}
          ]}
        ]}"#;

    #[test]
    fn parse_accepts_minimal_suite_and_defaults() {
        let s = parse_suite(OK_SUITE).unwrap();
        assert_eq!(s.name, "demo");
        assert_eq!(s.scenarios[0].steps[0].expect, Expect::Ok);
        assert_eq!(s.scenarios[0].steps[0].timeout_ms, None);
        // AG-07: padrões — sem retries, sem hooks
        assert_eq!(s.scenarios[0].steps[0].retries, 0);
        assert!(s.scenarios[0].setup.is_empty());
        assert!(s.scenarios[0].teardown.is_empty());
        assert_eq!(
            s.scenarios[0].steps[1].expect,
            Expect::Error("NO_APP".into())
        );
        // timeout_ms válido é aceito
        let s = parse_suite(
            r#"{"name": "t", "scenarios": [{"name": "c", "steps": [
                {"name": "tap", "arguments": {"x": 1, "y": 1}, "timeout_ms": 500}]}]}"#,
        )
        .unwrap();
        assert_eq!(s.scenarios[0].steps[0].timeout_ms, Some(500));
        // AG-07: retries e hooks válidos são aceitos (mesma gramática de passo)
        let s = parse_suite(
            r#"{"name": "t", "scenarios": [{"name": "c",
                "setup": [{"name": "install", "arguments": {"apk": "a.apk"}}],
                "steps": [{"name": "launch", "retries": 3, "timeout_ms": 100}],
                "teardown": [{"name": "package_clear", "expect": "error:NO_APP"}]}]}"#,
        )
        .unwrap();
        assert_eq!(s.scenarios[0].setup.len(), 1);
        assert_eq!(s.scenarios[0].steps[0].retries, 3);
        assert_eq!(s.scenarios[0].steps[0].timeout_ms, Some(100));
        assert_eq!(
            s.scenarios[0].teardown[0].expect,
            Expect::Error("NO_APP".into())
        );
    }

    /// AG-07: teardown roda mesmo depois de falha nos passos (limpeza
    /// garantida) e a falha registrada continua sendo a do passo.
    #[test]
    fn teardown_runs_even_after_step_failure() {
        let suite = parse_suite(
            r#"{"name": "hooks", "scenarios": [{"name": "c",
                "setup": [{"name": "device_profile"}],
                "steps": [{"name": "launch"}],
                "teardown": [{"name": "device_profile"}]}]}"#,
        )
        .unwrap();
        let out = run_scenario(&suite.scenarios[0], Path::new("."));
        assert!(!out.passed);
        assert_eq!(out.steps.len(), 3);
        assert_eq!(out.steps[0].phase, Phase::Setup);
        assert!(matches!(out.steps[0].status, StepStatus::Passed));
        assert_eq!(out.steps[1].phase, Phase::Step);
        assert!(matches!(out.steps[1].status, StepStatus::Failed(_)));
        assert_eq!(out.steps[2].phase, Phase::Teardown);
        assert!(
            matches!(out.steps[2].status, StepStatus::Passed),
            "teardown não é pulado por falha nos passos: {:?}",
            out.steps[2].status
        );
        assert!(out.failure.unwrap().contains("passo 0 (launch)"));
    }

    /// AG-07: falha no teardown reprova um cenário cujos passos passaram.
    #[test]
    fn teardown_failure_fails_a_passed_scenario() {
        let suite = parse_suite(
            r#"{"name": "hooks", "scenarios": [{"name": "c",
                "steps": [{"name": "device_profile"}],
                "teardown": [{"name": "launch"}]}]}"#,
        )
        .unwrap();
        let out = run_scenario(&suite.scenarios[0], Path::new("."));
        assert!(!out.passed);
        assert!(matches!(out.steps[0].status, StepStatus::Passed));
        let msg = out.failure.unwrap();
        assert!(msg.contains("teardown 0 (launch)"), "{msg}");
    }

    /// AG-07: falha no setup aborta o cenário — passos e teardown pulados.
    #[test]
    fn setup_failure_skips_steps_and_teardown() {
        let suite = parse_suite(
            r#"{"name": "hooks", "scenarios": [{"name": "c",
                "setup": [{"name": "launch"}],
                "steps": [{"name": "device_profile"}],
                "teardown": [{"name": "device_profile"}]}]}"#,
        )
        .unwrap();
        let out = run_scenario(&suite.scenarios[0], Path::new("."));
        assert!(!out.passed);
        assert!(out.failure.clone().unwrap().contains("setup 0 (launch)"));
        assert_eq!(out.steps[1].phase, Phase::Step);
        assert_eq!(out.steps[1].status, StepStatus::Skipped);
        assert_eq!(out.steps[2].phase, Phase::Teardown);
        assert_eq!(out.steps[2].status, StepStatus::Skipped);
    }

    /// AG-07: retries — passo que falha executa 1+retries tentativas; depois
    /// da falha final o cenário segue a regra normal (resto pulado).
    #[test]
    fn retries_rerun_the_failed_step() {
        let suite = parse_suite(
            r#"{"name": "retries", "scenarios": [{"name": "c", "steps": [
                {"name": "launch", "retries": 2},
                {"name": "device_profile"}]}]}"#,
        )
        .unwrap();
        let out = run_scenario(&suite.scenarios[0], Path::new("."));
        assert!(!out.passed);
        assert_eq!(out.steps[0].attempts, 3, "1 + retries tentativas");
        assert!(matches!(out.steps[0].status, StepStatus::Failed(_)));
        assert_eq!(out.steps[1].status, StepStatus::Skipped);
        assert_eq!(out.steps[1].attempts, 0);
    }

    /// AG-07: passo que passa não retrya (curto-circuito na 1ª tentativa).
    #[test]
    fn success_does_not_retry() {
        let suite = parse_suite(
            r#"{"name": "retries", "scenarios": [{"name": "c", "steps": [
                {"name": "device_profile", "retries": 4}]}]}"#,
        )
        .unwrap();
        let out = run_scenario(&suite.scenarios[0], Path::new("."));
        assert!(out.passed, "{:?}", out.failure);
        assert_eq!(out.steps[0].attempts, 1);
    }

    /// AG-07: o log do JUnit marca a fase dos hooks e anota retries.
    #[test]
    fn junit_log_marks_hook_phase_and_retries() {
        let suite = parse_suite(
            r#"{"name": "hooks", "scenarios": [{"name": "c",
                "setup": [{"name": "device_profile"}],
                "steps": [{"name": "launch", "retries": 1}],
                "teardown": [{"name": "device_profile"}]}]}"#,
        )
        .unwrap();
        let report = run_suite(&suite, Path::new("."));
        let xml = to_junit_xml(&report);
        assert!(xml.contains("[setup 0] device_profile PASS"), "{xml}");
        assert!(xml.contains("2 tentativas"), "{xml}");
        assert!(xml.contains("[teardown 0] device_profile PASS"), "{xml}");
    }

    /// APK mínimo (1 classe) para os testes de timeout por passo.
    fn timeout_fixture_apk() -> String {
        use rd_testsupport::apkfix::build_apk_sdk;
        use rd_testsupport::apkfix::ArscFix;
        use rd_testsupport::vm::DexBuilder;
        let arsc = ArscFix::new();
        let mut b = DexBuilder::new();
        b.class("Lcom/test/to/Main;", "Landroid/app/Activity;");
        let apk = build_apk_sdk(
            "com.test.to",
            "com.test.to.Main",
            arsc.build(),
            vec![("classes.dex", b.finish())],
            26,
            30,
        );
        let dir = std::env::temp_dir().join(format!(
            "rd-agent-suite-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("tmpdir");
        let path = dir.join("app.apk");
        std::fs::write(&path, apk).expect("apk fixture");
        path.to_string_lossy().to_string()
    }

    /// Adiado M6 — timeout por passo: orçamento estourado reprova o passo com
    /// STEP_TIMEOUT e pula os restantes; orçamento folgadoo deixa passar.
    /// (Medição de wall-clock em build de teste: o parse do APK na instalação
    /// custa muito mais que 1 ms.)
    #[test]
    fn step_timeout_fails_the_step_and_skips_the_rest() {
        let apk = timeout_fixture_apk();

        let suite = parse_suite(&format!(
            r#"{{"name": "to", "scenarios": [{{"name": "estoura", "steps": [
                {{"name": "install", "arguments": {{"apk": "{apk}"}}, "timeout_ms": 1}},
                {{"name": "device_profile", "arguments": {{}}}}
            ]}}]}}"#
        ))
        .unwrap();
        let report = run_suite(&suite, Path::new("."));
        assert!(!report.all_passed(), "estouro de orçamento é falha");
        let sc = &report.scenarios[0];
        assert!(
            sc.failure.as_deref().unwrap_or("").contains("STEP_TIMEOUT"),
            "falha nomeia o código estável: {:?}",
            sc.failure
        );
        assert!(
            matches!(sc.steps[0].status, StepStatus::Failed(_)),
            "{:?}",
            sc.steps[0]
        );
        assert!(
            matches!(sc.steps[1].status, StepStatus::Skipped),
            "passos seguintes são pulados: {:?}",
            sc.steps[1]
        );
        assert_eq!(report.exit_code(), 1);

        // o MESMO cenário com orçamento folgado passa inteiro
        let suite = parse_suite(&format!(
            r#"{{"name": "to", "scenarios": [{{"name": "folgado", "steps": [
                {{"name": "install", "arguments": {{"apk": "{apk}"}}, "timeout_ms": 60000}},
                {{"name": "device_profile", "arguments": {{}}}}
            ]}}]}}"#
        ))
        .unwrap();
        let report = run_suite(&suite, Path::new("."));
        assert!(
            report.all_passed(),
            "orçamento folgado não interfere: {:?}",
            report.scenarios[0].failure
        );
    }

    #[test]
    fn parse_rejects_malformed_before_running() {
        let cases = [
            ("não é json", "JSON inválido"),
            (r#"[]"#, "não é um objeto"),
            (r#"{"scenarios": []}"#, "\"name\""),
            (r#"{"name": "x", "scenarios": []}"#, "sem cenários"),
            (
                r#"{"name": "x", "scenarios": [{"name": "c", "steps": []}]}"#,
                "sem passos",
            ),
            (
                r#"{"name": "x", "scenarios": [{"name": "c", "steps": [{"name": "batch"}]}]}"#,
                "batch",
            ),
            (
                r#"{"name": "x", "scenarios": [{"name": "c", "steps": [{"name": "tap", "arguments": 5}]}]}"#,
                "arguments",
            ),
            (
                r#"{"name": "x", "scenarios": [{"name": "c", "steps": [{"name": "tap", "expect": "talvez"}]}]}"#,
                "expect",
            ),
            (
                r#"{"name": "x", "scenarios": [{"name": "c", "steps": [{"name": "tap", "expect": "error:"}]}]}"#,
                "expect",
            ),
            // adiado M6: timeout_ms precisa ser inteiro > 0
            (
                r#"{"name": "x", "scenarios": [{"name": "c", "steps": [{"name": "tap", "timeout_ms": 0}]}]}"#,
                "timeout_ms",
            ),
            (
                r#"{"name": "x", "scenarios": [{"name": "c", "steps": [{"name": "tap", "timeout_ms": -5}]}]}"#,
                "timeout_ms",
            ),
            (
                r#"{"name": "x", "scenarios": [{"name": "c", "steps": [{"name": "tap", "timeout_ms": "logo"}]}]}"#,
                "timeout_ms",
            ),
            // AG-07: retries precisa ser inteiro ≥ 0
            (
                r#"{"name": "x", "scenarios": [{"name": "c", "steps": [{"name": "tap", "retries": -1}]}]}"#,
                "retries",
            ),
            (
                r#"{"name": "x", "scenarios": [{"name": "c", "steps": [{"name": "tap", "retries": 1.5}]}]}"#,
                "retries",
            ),
            (
                r#"{"name": "x", "scenarios": [{"name": "c", "steps": [{"name": "tap", "retries": "muitas"}]}]}"#,
                "retries",
            ),
            // AG-07: hooks precisam ser listas de passos válidos
            (
                r#"{"name": "x", "scenarios": [{"name": "c", "steps": [{"name": "tap"}], "setup": {}}]}"#,
                "setup",
            ),
            (
                r#"{"name": "x", "scenarios": [{"name": "c", "steps": [{"name": "tap"}], "teardown": [{"name": "batch"}]}]}"#,
                "batch",
            ),
        ];
        for (text, needle) in cases {
            let e = parse_suite(text).expect_err(text);
            assert_eq!(e.code, INVALID_SUITE, "{text}");
            assert!(
                e.cause.contains(needle),
                "esperava {needle:?} em {:?}",
                e.cause
            );
        }
    }

    #[test]
    fn soft_failure_reads_ok_false_and_found_false() {
        let none = "wait";
        assert!(soft_failure(none, &serde_json::json!({"ok": true})).is_none());
        assert!(soft_failure(none, &serde_json::json!({"ok": false, "actual": "x"})).is_some());
        assert!(soft_failure("wait_for", &serde_json::json!({"found": false})).is_some());
        // ausência do campo não é falha: só o "não" explícito reprova
        assert!(soft_failure(none, &serde_json::json!({"finished": false})).is_none());
        assert!(soft_failure(none, &serde_json::Value::Null).is_none());
    }

    #[test]
    fn assert_finished_fails_when_activity_not_finished() {
        // achado da revisão: assert_finished devolve só {"finished": bool}
        assert!(soft_failure("assert_finished", &serde_json::json!({"finished": false})).is_some());
        assert!(soft_failure("assert_finished", &serde_json::json!({"finished": true})).is_none());
        // a mesma chave em outra tool não reprova (regra é por tool)
        assert!(soft_failure("tap", &serde_json::json!({"finished": false})).is_none());
    }

    #[test]
    fn expect_parsing() {
        assert_eq!(parse_expect(&serde_json::json!("ok")), Some(Expect::Ok));
        assert_eq!(
            parse_expect(&serde_json::json!("error:ASSERT_FAILED")),
            Some(Expect::Error("ASSERT_FAILED".into()))
        );
        assert_eq!(parse_expect(&serde_json::json!("error:assert")), None);
        assert_eq!(parse_expect(&serde_json::json!(3)), None);
    }

    #[test]
    fn scenario_stops_on_first_failure_and_skips_rest() {
        let suite = parse_suite(
            r#"{"name": "s", "scenarios": [{"name": "falha", "steps": [
                {"name": "tap", "arguments": {"x": 1, "y": 1}},
                {"name": "wait", "arguments": {"ms": 1}}
            ]}]}"#,
        )
        .unwrap();
        let out = run_scenario(&suite.scenarios[0], Path::new("."));
        assert!(!out.passed);
        assert!(matches!(out.steps[0].status, StepStatus::Failed(_)));
        assert_eq!(out.steps[1].status, StepStatus::Skipped);
        let msg = out.failure.unwrap();
        assert!(msg.contains("passo 0 (tap)"), "{msg}");
    }

    #[test]
    fn expected_error_code_mismatch_is_a_failure() {
        let suite = parse_suite(
            r#"{"name": "s", "scenarios": [{"name": "c", "steps": [
                {"name": "tap", "arguments": {"x": 1, "y": 1}, "expect": "error:CODIGO_ERRADO"}
            ]}]}"#,
        )
        .unwrap();
        let out = run_scenario(&suite.scenarios[0], Path::new("."));
        assert!(!out.passed);
        assert!(out.failure.unwrap().contains("esperado erro CODIGO_ERRADO"));
    }

    #[test]
    fn expected_error_is_a_pass_when_code_matches() {
        // launch sem APK instalado → NO_APP (código real do rd-agent)
        let suite = parse_suite(
            r#"{"name": "s", "scenarios": [{"name": "c", "steps": [
                {"name": "launch", "arguments": {}, "expect": "error:NO_APP"}
            ]}]}"#,
        )
        .unwrap();
        let out = run_scenario(&suite.scenarios[0], Path::new("."));
        assert!(out.passed, "{:?}", out.failure);
        assert_eq!(out.steps[0].status, StepStatus::Passed);
    }

    #[test]
    fn each_scenario_gets_a_fresh_session() {
        // o 2º cenário não herda o app do 1º: o launch sem app falha
        let suite = parse_suite(
            r#"{"name": "s", "scenarios": [
                {"name": "c1", "steps": [{"name": "device_profile"}]},
                {"name": "c2", "steps": [{"name": "app_info", "expect": "error:NO_APP"}]}
            ]}"#,
        )
        .unwrap();
        let report = run_suite(&suite, Path::new("."));
        assert!(report.scenarios[0].passed);
        // c2 começa do zero: não há APK, mesmo com c1 já tendo rodado
        assert!(
            report.scenarios[1].passed,
            "{:?}",
            report.scenarios[1].failure
        );
    }

    #[test]
    fn install_relative_path_resolves_against_suite_dir() {
        let args = serde_json::json!({"apk": "apps/x.apk"});
        let out = resolve_args("install", &args, Path::new("/suites"));
        assert_eq!(out["apk"], "/suites/apps/x.apk");
        // caminho com .. não é reescrito (a policy de sandbox decide)
        let args = serde_json::json!({"apk": "../x.apk"});
        assert_eq!(
            resolve_args("install", &args, Path::new("/s"))["apk"],
            "../x.apk"
        );
        // absoluto fica como está
        let args = serde_json::json!({"apk": "/abs/x.apk"});
        assert_eq!(
            resolve_args("install", &args, Path::new("/s"))["apk"],
            "/abs/x.apk"
        );
    }

    #[test]
    fn xml_escape_covers_specials_and_controls() {
        assert_eq!(xml_escape(r#"a<b>&"c'"#), "a&lt;b&gt;&amp;&quot;c&apos;");
        assert_eq!(xml_escape("x\u{1}y"), "x\u{FFFD}y");
    }

    #[test]
    fn junit_has_one_testcase_per_scenario_and_failure_element() {
        let suite = parse_suite(
            r#"{"name": "suite <&>", "scenarios": [
                {"name": "ok", "steps": [{"name": "device_profile"}]},
                {"name": "ruim", "steps": [{"name": "tap", "arguments": {"x": 1, "y": 1}}]}
            ]}"#,
        )
        .unwrap();
        let report = run_suite(&suite, Path::new("."));
        let xml = to_junit_xml(&report);
        assert!(xml.starts_with("<?xml"));
        assert!(xml.contains("tests=\"2\" failures=\"1\""), "{xml}");
        assert_eq!(xml.matches("<testcase ").count(), 2);
        assert_eq!(xml.matches("<failure ").count(), 1);
        assert!(xml.contains("suite &lt;&amp;&gt;"));
        assert_eq!(report.exit_code(), 1);
    }

    #[test]
    fn human_summary_mentions_failure() {
        let suite = parse_suite(
            r#"{"name": "h", "scenarios": [{"name": "c", "steps": [{"name": "tap", "arguments": {"x": 1, "y": 1}}]}]}"#,
        )
        .unwrap();
        let text = to_human(&run_suite(&suite, Path::new(".")));
        assert!(text.contains("FAIL  c"));
        assert!(text.contains("0 passou, 1 falhou"));
    }

    #[test]
    fn matrix_file_gets_a_runner_hint_and_keeps_the_error_code() {
        let matrix = r#"{"schema_version": 1, "suites": [{"name": "a"}]}"#;
        let e = parse_suite(matrix).expect_err("matrix is not a scenario suite");
        assert_eq!(e.code, INVALID_SUITE);
        let hint = e.suggestion.unwrap_or_default();
        assert!(hint.contains("tests/suites/m8-compat.json"), "{hint}");
        // A plain suite without name keeps the generic hint.
        let plain = parse_suite(r#"{"scenarios": []}"#).expect_err("no name");
        assert_eq!(plain.code, INVALID_SUITE);
        assert!(!plain.suggestion.unwrap_or_default().contains("matriz"));
    }
}
