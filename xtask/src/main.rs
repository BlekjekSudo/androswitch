//! xtask — tarefas de build do RustyDroid (GH-01).
//!
//! `cargo xtask compat-report` → gera `docs/compat-matrix.md` a partir dos
//! dados versionados + estado real das crates. Nunca manual: o placar oficial
//! do projeto é gerado.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

fn main() {
    if let Err(e) = run() {
        eprintln!("xtask error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()))
        // xtask roda de dentro de xtask/ — sobe para a raiz
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    match args.next().as_deref() {
        Some("compat-report") => {
            let out = args
                .next()
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("docs").join("compat-matrix.md"));
            compat_report(&root, &out)
        }
        Some("engine-report") => {
            let out = args
                .next()
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("docs/engine-registry.md"));
            let registry = rd_engine_registry::Registry::load(&root.join("data/engines.toml"))?;
            std::fs::write(&out, registry.markdown())
                .with_context(|| format!("writing {}", out.display()))?;
            println!("engine registry report: {}", out.display());
            Ok(())
        }
        Some("engine-spec") => {
            let out = args
                .next()
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("RustyDroid.md"));
            engine_spec(&root, &out)
        }
        Some("--help") | Some("help") | None => {
            println!("xtask — RustyDroid build tasks");
            println!();
            println!("USAGE: cargo xtask <COMMAND>");
            println!();
            println!("COMMANDS:");
            println!("  compat-report [out.md]  gera docs/compat-matrix.md (placar oficial)");
            println!("  engine-report [out.md]  gera docs/engine-registry.md (Apêndice G)");
            println!("  engine-spec [out.md]    regenera apenas a tabela marcada em RustyDroid.md");
            Ok(())
        }
        Some(other) => bail!("unknown task {other:?} — try `cargo xtask --help`"),
    }
}

/// Estado real das crates (fonte da verdade: este arquivo, junto do roadmap).
/// issue #33: sincronizado com M2 (42b6532) — rd-dex e rd-vm NÃO são mais stub.
const CRATE_STATUS: &[(&str, &str, &str)] = &[
    (
        "rd-apk",
        "M0",
        "ZIP + AXML + arsc + assinaturas + manifest model completo",
    ),
    (
        "rd-framework",
        "M0",
        "fd-permissions (motor completo + gating maxSdk/since_api) · fd-devices · fd-behavior",
    ),
    (
        "rd-cli",
        "M0–M6",
        "inspect · perm · device · behavior · dex disasm · vm exec · app run · mcp · test (suítes JUnit, M6)",
    ),
    (
        "rd-dex",
        "M1",
        "parser DEX completo + disassembler smali validado vs baksmali + fuzz targets",
    ),
    (
        "rd-vm",
        "M2–M3",
        "interpretador Dalvik + golden harness vs JVM + framework host headless (Activity/View/Handler/Looper) + LayoutInflater AXML/ARSC",
    ),
    (
        "rd-testsupport",
        "M0–M6",
        "fixtures de teste (APK/DEX/AXML/ARSC sintéticos; dev-only, fora do binário)",
    ),
    (
        "rd-render",
        "M4",
        "UI dump uiautomator + screenshot headless (software renderer determinístico + PNG sem deps); winit/web são stub → M11",
    ),
    (
        "rd-agent",
        "M5–M6",
        "MCP v1 (30 tools + batch + policy sandbox) · executor de suítes `rd test` (JSON → JUnit XML, M6)",
    ),
    ("rd-engine-registry", "Apêndice G", "registro/detecção dos 28 itens; features/contratos; NÃO executa engines"),
    ("rd-jni", "M3+", "stub NOT_IMPLEMENTED"),
    ("rd-ndk", "M9", "stub NOT_IMPLEMENTED"),
];

fn compat_report(root: &Path, out: &Path) -> Result<()> {
    // M6: drift protection — crate nova sem linha na tabela quebra o relatório.
    let missing = missing_crate_entries(root)?;
    if !missing.is_empty() {
        bail!(
            "crates sem entrada em CRATE_STATUS (xtask/src/main.rs): {}",
            missing.join(", ")
        );
    }
    let perm_toml = std::fs::read_to_string(root.join("data/permissions.toml"))
        .context("data/permissions.toml ausente")?;
    let devices_toml = std::fs::read_to_string(root.join("data/devices.toml"))
        .context("data/devices.toml ausente")?;
    let switches_toml = std::fs::read_to_string(root.join("data/behavior-switches.toml"))
        .context("data/behavior-switches.toml ausente")?;

    let perms: toml::Value = toml::from_str(&perm_toml)?;
    let devices: toml::Value = toml::from_str(&devices_toml)?;
    let switches: toml::Value = toml::from_str(&switches_toml)?;

    let perm_count = perms
        .get("permission")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let role_count = perms
        .get("role")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let device_count = devices
        .get("device")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let switch_count = switches
        .get("switch")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    let mut md = String::new();
    md.push_str("# compat-matrix — placar oficial do RustyDroid\n\n");
    md.push_str("> **GERADO** por `cargo xtask compat-report` — não editar à mão.\n");
    md.push_str("> Lei de Ouro: nenhuma API ausente quebra o app em silêncio — ");
    md.push_str("tudo responde `NOT_IMPLEMENTED(id)` e aparece aqui.\n\n");
    // M6: o status das fases vem do docs/ROADMAP.md (fonte da verdade), não de
    // uma lista fixa — a lista fixa ficou defasada (M4/M5 apareciam como ⬜).
    let roadmap = std::fs::read_to_string(root.join("docs").join("ROADMAP.md"))
        .context("docs/ROADMAP.md ausente")?;
    let phases = parse_roadmap_phases(&roadmap)?;
    md.push_str("## Estado por fase (roadmap, lido de docs/ROADMAP.md)\n\n");
    md.push_str("| Fase | Entrega | Status |\n|---|---|---|\n");
    for ph in &phases {
        let status = if ph.done {
            "**✅ implementado**"
        } else {
            "⬜"
        };
        md.push_str(&format!("| {} | {} | {} |\n", ph.id, ph.entrega, status));
    }

    md.push_str("\n## Crates\n\n| Crate | Fase | Escopo |\n|---|---|---|\n");
    for (name, fase, escopo) in CRATE_STATUS {
        md.push_str(&format!("| `{name}` | {fase} | {escopo} |\n"));
    }

    md.push_str("\n## Suítes automatizadas (`rd test`, M6)\n\n");
    let suites = collect_suites(root)?;
    if suites.is_empty() {
        md.push_str("_nenhuma suíte em `tests/suites/`_\n");
    } else {
        md.push_str("| Suíte | Arquivo | Cenários | Passos |\n|---|---|---|---|\n");
        for su in &suites {
            md.push_str(&format!(
                "| {} | `tests/suites/{}` | {} | {} |\n",
                su.name, su.file, su.scenarios, su.steps
            ));
        }
        md.push_str("\nExecutar: `rd test tests/suites/<arquivo>.json --junit <saida>.xml` (formato em `docs/suite-format.md`).\n");
    }

    let registry = rd_engine_registry::Registry::load(&root.join("data/engines.toml"))?;
    md.push_str("\n## Engines (Apêndice G — registro/detecção, não execução)\n\n");
    md.push_str("**28 registros completos; 12 módulos opt-in.** Features não implementam runtimes; todos os adaptadores do registro retornam `NOT_IMPLEMENTED(LANG-xx)`. DEX parcial é da VM existente. M9/M11 continuam pendentes. Detecção golden não é golden de execução.\n\n");
    md.push_str(&registry.appendix_table());
    md.push_str("\nRegras, dependências e limites em [engine-registry.md](engine-registry.md); fonte: `data/engines.toml`.\n");

    md.push_str("\n## Dados versionados (M0)\n\n");
    md.push_str(&format!(
        "- permissions.toml: **{perm_count}** permissões + {role_count} roles (Apêndice B)\n"
    ));
    md.push_str(&format!(
        "- devices.toml: **{device_count}** perfis (piso: `moto-e5`)\n"
    ));
    md.push_str(&format!(
        "- behavior-switches.toml: **{switch_count}** comutadores targetSdk 26→36 (PRF-07)\n"
    ));
    md.push_str("\n## Compatibilidade por módulo (checklist PARTE 0)\n\n");
    md.push_str(
        "- Núcleo (0.0): parcial (CORE-01..06 M0; rd-dex M1; VM M2 — métodos puros executam)\n",
    );
    md.push_str("- Framework (M3): Activity lifecycle + views host + Handler/Looper + touch + **LayoutInflater de layout XML real** (setContentView(I): resid → @layout/key no arsc → AXML do APK → árvore host; attrs id/text/orientation/visibility/enabled/onClick/layout_height; setText(I), getString(I), findViewById) — classes de view sem host (ImageView/…) e configs específicas (land/locale) respondem tipadas\n");
    md.push_str("- Render/agente: `NOT_IMPLEMENTED(module_id)` estruturado (M4+)\n");

    std::fs::create_dir_all(out.parent().unwrap_or(Path::new(".")))?;
    std::fs::write(out, &md).with_context(|| format!("writing {}", out.display()))?;
    println!("compat-matrix gerada: {}", out.display());
    Ok(())
}

/// Preserve the rest of the master spec byte-for-byte; only the marked table changes.
fn engine_spec(root: &Path, out: &Path) -> Result<()> {
    let spec = std::fs::read_to_string(root.join("RustyDroid.md"))?;
    let registry = rd_engine_registry::Registry::load(&root.join("data/engines.toml"))?;
    let start = "<!-- BEGIN ENGINE REGISTRY -->";
    let end = "<!-- END ENGINE REGISTRY -->";
    let (before, rest) = spec
        .split_once(start)
        .context("engine table start marker missing")?;
    let (_, after) = rest
        .split_once(end)
        .context("engine table end marker missing")?;
    let generated = format!("{before}{start}\n{}{end}{after}", registry.appendix_table());
    std::fs::write(out, generated).with_context(|| format!("writing {}", out.display()))?;
    println!("Appendix G table: {}", out.display());
    Ok(())
}

/// Linha da tabela de fases do docs/ROADMAP.md.
#[derive(Debug, PartialEq)]
struct PhaseRow {
    id: String,
    entrega: String,
    done: bool,
}

/// Lê a tabela `| Fase | Entrega central | DoD | Status |` do ROADMAP.
/// Só consome as linhas contíguas depois do cabeçalho; status ✅ = concluído.
fn parse_roadmap_phases(text: &str) -> Result<Vec<PhaseRow>> {
    let mut rows = Vec::new();
    let mut in_table = false;
    for line in text.lines() {
        let t = line.trim();
        if !in_table {
            if t.starts_with("| Fase |") {
                in_table = true;
            }
            continue;
        }
        if !t.starts_with('|') {
            break; // fim da tabela
        }
        let cells: Vec<&str> = t.trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() < 4 {
            continue; // separador |---|
        }
        let id = cells[0].trim_matches('*').trim();
        let is_phase =
            id.len() >= 2 && id.starts_with('M') && id[1..].chars().all(|c| c.is_ascii_digit());
        if !is_phase {
            continue;
        }
        rows.push(PhaseRow {
            id: id.to_string(),
            entrega: cells[1].to_string(),
            done: cells[cells.len() - 1].contains('✅'),
        });
    }
    if rows.is_empty() {
        bail!("tabela de fases não encontrada em docs/ROADMAP.md (cabeçalho `| Fase |`)");
    }
    Ok(rows)
}

/// Crates em `crates/` que não aparecem em CRATE_STATUS.
fn missing_crate_entries(root: &Path) -> Result<Vec<String>> {
    let mut missing = Vec::new();
    for entry in std::fs::read_dir(root.join("crates")).context("lendo crates/")? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !CRATE_STATUS.iter().any(|(n, _, _)| *n == name) {
            missing.push(name);
        }
    }
    missing.sort();
    Ok(missing)
}

/// Suíte em `tests/suites/*.json` (contagens lidas do próprio arquivo).
struct SuiteInfo {
    file: String,
    name: String,
    scenarios: usize,
    steps: usize,
}

fn collect_suites(root: &Path) -> Result<Vec<SuiteInfo>> {
    let dir = root.join("tests").join("suites");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let file = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let text = std::fs::read_to_string(&path)?;
        let v: serde_json::Value = serde_json::from_str(&text)
            .with_context(|| format!("suíte {file} não é JSON válido"))?;
        let name = v
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or(&file)
            .to_string();
        let scen = v
            .get("scenarios")
            .and_then(|s| s.as_array())
            .cloned()
            .unwrap_or_default();
        let steps = scen
            .iter()
            .map(|c| {
                c.get("steps")
                    .and_then(|s| s.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0)
            })
            .sum();
        out.push(SuiteInfo {
            file,
            name,
            scenarios: scen.len(),
            steps,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Raiz do repositório (xtask/ sobe um nível).
    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask/ tem diretório pai")
            .to_path_buf()
    }

    /// M6: a tabela de fases vem do ROADMAP real (13 fases M0..M12, M0 concluída).
    #[test]
    fn phases_are_read_from_roadmap() {
        let text = std::fs::read_to_string(repo_root().join("docs/ROADMAP.md")).expect("ROADMAP");
        let rows = parse_roadmap_phases(&text).expect("tabela de fases");
        assert_eq!(rows.len(), 13, "M0..M12");
        assert_eq!(rows[0].id, "M0");
        assert!(rows[0].done, "M0 está marcada como concluída no ROADMAP");
    }

    #[test]
    fn roadmap_parser_only_takes_the_status_table() {
        let sample = "| Fase | X | Y | Status |\n|---|---|---|---|\n| **M1** | a | b | ✅ |\n| **M2** | c | d | ⬜ |\n\nfora | M9 | x | ✅ |\n| M8 | depois | da | ✅ |\n";
        let rows = parse_roadmap_phases(sample).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0],
            PhaseRow {
                id: "M1".into(),
                entrega: "a".into(),
                done: true
            }
        );
        assert!(!rows[1].done);
        assert!(parse_roadmap_phases("sem tabela").is_err());
    }

    /// M6: toda crate do workspace precisa de linha na tabela (drift guard).
    #[test]
    fn every_workspace_crate_has_a_status_row() {
        let missing = missing_crate_entries(&repo_root()).expect("lista crates/");
        assert!(missing.is_empty(), "crates sem entrada: {missing:?}");
    }

    /// M6: as suítes versionadas são contadas a partir dos arquivos.
    #[test]
    fn suites_are_collected_with_counts() {
        let suites = collect_suites(&repo_root()).expect("suítes");
        let contract = suites
            .iter()
            .find(|s| s.file == "contract-no-app.json")
            .expect("suíte de contrato versionada");
        assert_eq!(contract.scenarios, 8);
        assert_eq!(contract.steps, 10);
    }

    /// issue #73: README, CONTRIBUTING e docs/compat-matrix.md documentam
    /// `cargo xtask compat-report`. Sem o alias versionado em
    /// `.cargo/config.toml`, num clone limpo o comando falha com
    /// "no such command: `xtask`".
    #[test]
    fn cargo_alias_xtask_is_versioned() {
        let cfg = std::fs::read_to_string(repo_root().join(".cargo/config.toml"))
            .expect(".cargo/config.toml versionado (issue #73)");
        let table: toml::Value = toml::from_str(&cfg).expect(".cargo/config.toml é TOML válido");
        let alias = table
            .get("alias")
            .and_then(|a| a.get("xtask"))
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("alias `xtask` ausente em .cargo/config.toml:\n{cfg}"));
        assert!(
            alias.contains("--package xtask") || alias.contains("-p xtask"),
            "alias deve apontar para o pacote xtask: {alias:?}"
        );
    }

    /// issue #86: a regra "sem `unsafe` fora de rd-ndk/rd-jni" (CONTRIBUTING
    /// item 5, SECURITY.md) é imposta pelo compilador. Este teste impede que
    /// alguém remova a configuração de lint — sem ela, um `unsafe` em rd-vm,
    /// rd-apk ou rd-dex compilaria e passaria em todos os jobs do CI.
    #[test]
    fn every_crate_declares_the_unsafe_policy() {
        let root = repo_root();
        let workspace: toml::Value = toml::from_str(
            &std::fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml da raiz"),
        )
        .expect("Cargo.toml da raiz é TOML válido");
        let ws_unsafe = workspace
            .get("workspace")
            .and_then(|w| w.get("lints"))
            .and_then(|l| l.get("rust"))
            .and_then(|r| r.get("unsafe_code"))
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("[workspace.lints.rust] unsafe_code ausente (issue #86)"));
        assert_eq!(
            ws_unsafe, "forbid",
            "o default do workspace tem de ser forbid (issue #86)"
        );

        let members = workspace
            .get("workspace")
            .and_then(|w| w.get("members"))
            .and_then(|m| m.as_array())
            .expect("workspace.members");
        assert!(!members.is_empty());
        for member in members {
            let dir = member.as_str().expect("member é string");
            let crate_name = dir.rsplit('/').next().unwrap_or(dir);
            let text = std::fs::read_to_string(root.join(dir).join("Cargo.toml"))
                .unwrap_or_else(|e| panic!("Cargo.toml de {dir}: {e}"));
            let table: toml::Value = toml::from_str(&text).expect("TOML válido");
            let lints = table
                .get("lints")
                .unwrap_or_else(|| panic!("{dir} não declara [lints] (issue #86)"));
            let inherited = lints.get("workspace").and_then(|v| v.as_bool()) == Some(true);
            let own = lints
                .get("rust")
                .and_then(|r| r.get("unsafe_code"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            assert!(
                inherited || own.is_some(),
                "{dir} não herda nem declara unsafe_code (issue #86)"
            );
            let level = own.unwrap_or_else(|| ws_unsafe.to_string());
            // rd-ndk/rd-jni são a exceção documentada (ponte nativa), mas nem
            // elas podem liberar `unsafe` sem `#[allow]` local justificado.
            let expected = if matches!(crate_name, "rd-ndk" | "rd-jni") {
                "deny"
            } else {
                "forbid"
            };
            assert_eq!(
                level, expected,
                "{dir}: unsafe_code={level:?}, esperado {expected:?} (issue #86)"
            );
            assert_ne!(level, "allow", "{dir}: `allow` desliga a regra");
        }
    }

    /// issue #87: a spec (`RustyDroid.md`) citava Health Connect/HEALTH na API
    /// 36 enquanto os dados versionados usam 34 (Android 14 — Health Connect
    /// entrou na plataforma; `HealthPermissions` é "Added in API level 34").
    /// Este teste trava a coerência entre spec e dados para o assunto.
    #[test]
    fn health_connect_api_is_consistent_between_spec_and_data() {
        let root = repo_root();

        // 1) dado: comutador `health-connect-platform`
        let switches: toml::Value = toml::from_str(
            &std::fs::read_to_string(root.join("data/behavior-switches.toml"))
                .expect("data/behavior-switches.toml"),
        )
        .expect("behavior-switches.toml é TOML válido");
        let switch_api = switches
            .get("switch")
            .and_then(|v| v.as_array())
            .and_then(|a| {
                a.iter().find(|s| {
                    s.get("id").and_then(|i| i.as_str()) == Some("health-connect-platform")
                })
            })
            .and_then(|s| s.get("api"))
            .and_then(|v| v.as_integer())
            .expect("switch health-connect-platform com `api`");
        assert_eq!(
            switch_api, 34,
            "Android 14 (API 34) — Health Connect entrou na plataforma (issue #87)"
        );

        // 2) dado: permissões do grupo HEALTH usam o mesmo since_api
        let perms: toml::Value = toml::from_str(
            &std::fs::read_to_string(root.join("data/permissions.toml"))
                .expect("data/permissions.toml"),
        )
        .expect("permissions.toml é TOML válido");
        let health: Vec<(String, i64)> = perms
            .get("permission")
            .and_then(|v| v.as_array())
            .expect("tabela de permissões")
            .iter()
            .filter(|p| p.get("group").and_then(|g| g.as_str()) == Some("HEALTH"))
            .map(|p| {
                (
                    p.get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("?")
                        .to_string(),
                    p.get("since_api")
                        .and_then(|v| v.as_integer())
                        .unwrap_or(-1),
                )
            })
            .collect();
        assert!(
            health.len() >= 3,
            "grupo HEALTH deveria ter as 3 permissões health.*: {health:?}"
        );
        for (name, api) in &health {
            assert_eq!(
                *api, switch_api,
                "{name}: since_api={api} difere do comutador ({switch_api}) — issue #87"
            );
        }

        // 3) spec: toda menção a Health Connect com API/@ cita o mesmo número
        let spec = std::fs::read_to_string(root.join("RustyDroid.md")).expect("RustyDroid.md");
        let cited = cited_health_apis(&spec);
        assert!(
            !cited.is_empty(),
            "nenhuma menção a 'Health Connect' com API na spec — o teste ficou cego"
        );
        for api in cited {
            assert_eq!(
                api, switch_api as u32,
                "spec cita Health Connect na API {api}, os dados dizem {switch_api} (issue #87)"
            );
        }
    }

    /// Níveis de API citados ao lado de "Health Connect" na spec (`API 34`,
    /// `@34`), lidos numa janela depois de cada ocorrência.
    fn cited_health_apis(spec: &str) -> Vec<u32> {
        let mut out = Vec::new();
        let mut rest = spec;
        while let Some(idx) = rest.find("Health Connect") {
            let after = &rest[idx + "Health Connect".len()..];
            // só a própria linha — senão a janela invade o parágrafo seguinte
            let window: String = after.chars().take_while(|c| *c != '\n').take(80).collect();
            out.extend(api_numbers_after(&window, "@"));
            out.extend(api_numbers_after(&window, "API "));
            rest = after;
        }
        out
    }

    /// Números que aparecem logo depois do marcador (`@34`, `API 34`).
    fn api_numbers_after(text: &str, marker: &str) -> Vec<u32> {
        let mut out = Vec::new();
        let mut rest = text;
        while let Some(idx) = rest.find(marker) {
            let digits: String = rest[idx + marker.len()..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(n) = digits.parse::<u32>() {
                out.push(n);
            }
            rest = &rest[idx + marker.len()..];
        }
        out
    }

    /// issue #73: o placar versionado precisa ser idêntico ao gerado (senão
    /// quem seguir a documentação regenera algo diferente do commitado).
    #[test]
    fn compat_report_matches_versioned_scoreboard() {
        let root = repo_root();
        let generated =
            std::env::temp_dir().join(format!("rd-xtask-compat-{}.md", std::process::id()));
        compat_report(&root, &generated).expect("compat-report roda sobre os dados versionados");
        let got = std::fs::read_to_string(&generated).expect("arquivo gerado");
        let want = std::fs::read_to_string(root.join("docs/compat-matrix.md"))
            .expect("docs/compat-matrix.md versionado");
        let _ = std::fs::remove_file(&generated);
        assert_eq!(
            got, want,
            "docs/compat-matrix.md desatualizado — rode `cargo xtask compat-report`"
        );
    }

    #[test]
    fn engine_report_and_appendix_table_match_the_versioned_catalog() {
        let root = repo_root();
        let registry = rd_engine_registry::Registry::load(&root.join("data/engines.toml")).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("docs/engine-registry.md")).unwrap(),
            registry.markdown()
        );
        let spec = std::fs::read_to_string(root.join("RustyDroid.md")).unwrap();
        let (_, table) = spec.split_once("<!-- BEGIN ENGINE REGISTRY -->\n").unwrap();
        let (table, _) = table.split_once("<!-- END ENGINE REGISTRY -->").unwrap();
        assert_eq!(
            table,
            registry.appendix_table(),
            "run cargo xtask engine-spec"
        );
    }

    #[test]
    fn language_features_match_catalog_dependencies_and_forwarding() {
        let root = repo_root();
        let registry = rd_engine_registry::Registry::load(&root.join("data/engines.toml")).unwrap();
        for pkg in ["rd-engine-registry", "rd-agent", "rd-cli"] {
            let text =
                std::fs::read_to_string(root.join("crates").join(pkg).join("Cargo.toml")).unwrap();
            let cargo: toml::Value = toml::from_str(&text).unwrap();
            let features = cargo.get("features").unwrap();
            assert!(
                features
                    .get("default")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .is_empty(),
                "{pkg}: optional by default"
            );
            for module in registry.modules() {
                let actual: Vec<String> = features
                    .get(&module.feature)
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_string())
                    .collect();
                let expected = match pkg {
                    "rd-engine-registry" => {
                        module.depends_on.iter().map(|d| d.to_lowercase()).collect()
                    }
                    "rd-agent" => vec![format!("rd-engine-registry/{}", module.feature)],
                    _ => vec![
                        format!("rd-engine-registry/{}", module.feature),
                        format!("rd-agent/{}", module.feature),
                    ],
                };
                assert_eq!(actual, expected, "{pkg}: {} drift", module.feature);
            }
        }
    }

    #[test]
    fn manifest_explicitly_scopes_registry_without_selecting_new_runtimes() {
        let root = repo_root();
        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("rustydroid.manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["registries"]["appendix_g"]["engines"], 28);
        assert_eq!(
            manifest["registries"]["appendix_g"]["scope"],
            rd_engine_registry::SCOPE
        );
        assert_eq!(
            manifest["registries"]["appendix_g"]["runtime_selection_unchanged"],
            true
        );
        let selected = manifest["selected"].as_array().unwrap();
        assert!(
            !selected
                .iter()
                .any(|m| m.as_str().unwrap().starts_with("LANG-")),
            "registry delivery must not silently select runtime implementations"
        );
    }
}
