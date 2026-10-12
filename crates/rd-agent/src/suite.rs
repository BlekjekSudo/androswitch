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
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scenario {
    pub name: String,
    pub steps: Vec<Step>,
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
    pub tool: String,
    pub status: StepStatus,
    pub elapsed_ms: u128,
    /// Resultado cru da tool (texto), para o log do JUnit.
    pub output: String,
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
    let name = obj
        .get("name")
        .and_then(|n| n.as_str())
        .filter(|n| !n.trim().is_empty())
        .ok_or_else(|| invalid("campo \"name\" ausente ou vazio", "dê um nome à suíte"))?
        .to_string();
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
            steps.push(parse_step(&sname, pi, pv)?);
        }
        scenarios.push(Scenario { name: sname, steps });
    }
    Ok(Suite { name, scenarios })
}

fn parse_step(scenario: &str, pi: usize, pv: &Value) -> Result<Step, RdError> {
    let po = pv.as_object().ok_or_else(|| {
        invalid(
            format!("{scenario:?} passo {pi} não é objeto"),
            "cada passo é {\"name\": …, \"arguments\": {…}}",
        )
    })?;
    let name = po
        .get("name")
        .and_then(|n| n.as_str())
        .filter(|n| !n.trim().is_empty())
        .ok_or_else(|| {
            invalid(
                format!("{scenario:?} passo {pi} sem \"name\""),
                "informe o nome da tool",
            )
        })?
        .to_string();
    if name == "batch" {
        return Err(invalid(
            format!("{scenario:?} passo {pi}: batch não pode ser usado dentro de suíte"),
            "chame as tools diretamente como passos",
        ));
    }
    let arguments = match po.get("arguments") {
        None => Value::Object(Default::default()),
        Some(a) if a.is_object() => a.clone(),
        Some(_) => {
            return Err(invalid(
                format!("{scenario:?} passo {pi} ({name}): \"arguments\" precisa ser objeto"),
                "use {\"arguments\": {…}} ou omita o campo",
            ))
        }
    };
    let expect = match po.get("expect") {
        None => Expect::Ok,
        Some(e) => parse_expect(e).ok_or_else(|| {
            invalid(
                format!("{scenario:?} passo {pi} ({name}): \"expect\" inválido: {e}"),
                "use \"ok\" ou \"error:CODIGO_ESTAVEL\" (ex.: \"error:INVALID_FORMAT\")",
            )
        })?,
    };
    Ok(Step {
        name,
        arguments,
        expect,
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

/// Executa um cenário numa sessão nova. Para na primeira falha.
pub fn run_scenario(scenario: &Scenario, base: &Path) -> ScenarioOutcome {
    let started = Instant::now();
    let mut session = AgentSession::new();
    let mut steps = Vec::with_capacity(scenario.steps.len());
    let mut failure: Option<String> = None;

    for (i, step) in scenario.steps.iter().enumerate() {
        if failure.is_some() {
            steps.push(StepOutcome {
                index: i,
                tool: step.name.clone(),
                status: StepStatus::Skipped,
                elapsed_ms: 0,
                output: String::new(),
            });
            continue;
        }
        let t0 = Instant::now();
        let args = resolve_args(&step.name, &step.arguments, base);
        let r = session.call_tool(&step.name, &args);
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
        let status = match (&step.expect, r.is_error) {
            // asserções do MCP respondem isError=false com "ok": false (ou
            // "found": false) quando a condição NÃO se confirma. Para um
            // executor de testes isso é falha — senão toda asserção errada
            // passaria em verde.
            (Expect::Ok, false) if soft.is_some() => StepStatus::Failed(format!(
                "passo {i} ({}): asserção falhou: {}",
                step.name,
                soft.clone().unwrap_or_default()
            )),
            (Expect::Ok, false) => StepStatus::Passed,
            (Expect::Ok, true) => StepStatus::Failed(format!(
                "passo {i} ({}): esperado ok, obtido erro {}: {}",
                step.name,
                if code.is_empty() { "?" } else { &code },
                cause
            )),
            (Expect::Error(want), true) if *want == code => StepStatus::Passed,
            (Expect::Error(want), true) => StepStatus::Failed(format!(
                "passo {i} ({}): esperado erro {want}, obtido {}: {}",
                step.name,
                if code.is_empty() { "?" } else { &code },
                cause
            )),
            (Expect::Error(want), false) => StepStatus::Failed(format!(
                "passo {i} ({}): esperado erro {want}, mas a tool completou sem erro",
                step.name
            )),
        };
        if let StepStatus::Failed(msg) = &status {
            failure = Some(msg.clone());
        }
        steps.push(StepOutcome {
            index: i,
            tool: step.name.clone(),
            status,
            elapsed_ms: t0.elapsed().as_millis(),
            output: text,
        });
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
            log.push_str(&format!(
                "[{}] {} {} ({} ms)\n",
                st.index, st.tool, tag, st.elapsed_ms
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
        assert_eq!(
            s.scenarios[0].steps[1].expect,
            Expect::Error("NO_APP".into())
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
}
