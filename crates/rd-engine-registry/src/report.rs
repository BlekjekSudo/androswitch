//! Feature-independent documentation: never infer runtime status from build flags.

use crate::{DetectionState, Registry, RuntimeStatus};

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\r', '\n'], " ")
}

impl Registry {
    /// The marked table in RustyDroid.md is generated from the same catalog.
    pub fn appendix_table(&self) -> String {
        let mut out = String::from(
            "| # | Linguagem/Framework | Artefato no APK | Módulo | Estratégia | Status |\n\
             |---|---|---|---|---|---|\n",
        );
        for e in self.engines() {
            let runtime = match e.runtime_status {
                RuntimeStatus::PartialDex => "DEX parcial",
                RuntimeStatus::NotImplemented => "NOT_IMPLEMENTED",
            };
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | Registro ✅; runtime {} |\n",
                e.number,
                cell(&e.name),
                cell(&e.artifact),
                e.modules.join("+"),
                cell(&e.strategy),
                runtime
            ));
        }
        out
    }

    pub fn markdown(&self) -> String {
        let mut out = String::from(
            "# Registro de engines — Apêndice G\n\n\
             > **GERADO** por `cargo xtask engine-report` a partir de `data/engines.toml`.\n\
             > Escopo: **registro/detecção dos 28 itens**, não implementação de runtimes.\n\n\
             DEX/ELF não identificam a linguagem de origem. `signature` é uma\n\
             heurística de nomes/manifest, não validação binária; `candidate` é\n\
             compatibilidade de formato; `not_detected` não exclui um framework.\n\
             Um APK híbrido pode ter várias assinaturas. Nenhuma autoriza execução.\n\n\
             ## Consulta\n\n\
             ```sh\n\
             cargo build -p rd-cli\n\
             ./target/debug/rd engine list --json\n\
             ./target/debug/rd engine detect app.apk --json\n\
             ./target/debug/rd engine check flutter --json # exit 2, NOT_IMPLEMENTED\n\
             ./target/debug/rd inspect app.apk --json     # campo engines\n\
             ```\n\n\
             A tool MCP `app_info` expõe o mesmo relatório em `engines`; não há\n\
             nova tool. O catálogo/relatório funciona no build mínimo, sem features\n\
             opcionais. `--data` / `RD_DATA_DIR` explícitos exigem `engines.toml`;\n\
             arquivo ausente, inválido ou schema desconhecido retorna erro tipado,\n\
             nunca fallback silencioso. Sem override, o catálogo embutido permite\n\
             distribuir o binário sozinho. MCP usa o catálogo embutido.\n\n\
             ## Matriz completa\n\n",
        );
        out.push_str(&self.appendix_table());
        out.push_str("\n## Features e dependências\n\n");
        out.push_str("Features selecionam contratos, **não compilam/baixam engines**. Todas são\nopt-in, sem features default. Dependências são transitivas e propagadas\npelo `rd-cli`/`rd-agent`; o catálogo permanece completo em qualquer build.\nA seleção de runtimes no manifest/spec não mudou nesta entrega.\n\n");
        out.push_str(
            "| Módulo | Cargo feature | Dependências diretas | Nota |\n|---|---|---|---|\n",
        );
        for m in self.modules() {
            out.push_str(&format!(
                "| {} | `{}` | {} | {} |\n",
                m.id,
                m.feature,
                if m.depends_on.is_empty() {
                    "—".into()
                } else {
                    m.depends_on.join(", ")
                },
                cell(&m.notes)
            ));
        }
        out.push_str("\nExemplo: `cargo build -p rd-cli --no-default-features --features lang-04`\nseleciona LANG-04 → LANG-02 → LANG-01. O campo `feature_enabled` confirma\nseleção, enquanto `registry_adapter_status` continua `NOT_IMPLEMENTED`.\nLANG-08 representa a união Mono/IL2CPP; selecionar o módulo inclui LANG-07.\nCompose/render, WebView, JNI, NDK e bridges reais continuam trabalho futuro.\n\n");
        out.push_str("## Regras de detecção e rotas\n\n");
        out.push_str("`all` exige todas as pistas; `any` exige pelo menos uma quando não vazio.\nRegras só usam nomes ZIP canônicos (case-sensitive) e classes/metadados\nde componentes do manifest. Bibliotecas nativas exigem exatamente\n`lib/<abi>/<arquivo>.so`; DEX exige `classes.dex` ou `classesN.dex` (N ≥ 2).\nNenhum asset/lib é descomprimido, extraído, carregado ou executado.\n\n");
        for e in self.engines() {
            out.push_str(&format!("### {} — `{}`\n\n- **Rota existente/futura:** `{}`\n- **Runtime:** {}\n- **Limite:** {}\n",
                e.number, e.id, cell(&e.adapter_crate), e.runtime_status.label(), cell(&e.notes)));
            for r in &e.rule {
                let state = match r.state {
                    DetectionState::Candidate => "candidate",
                    DetectionState::Signature => "signature",
                    DetectionState::NotDetected => "not_detected",
                };
                out.push_str(&format!(
                    "- **{}:** all=[{}]; any=[{}] — {}\n",
                    state,
                    r.all
                        .iter()
                        .map(|p| format!("`{}`", cell(p)))
                        .collect::<Vec<_>>()
                        .join(", "),
                    r.any
                        .iter()
                        .map(|p| format!("`{}`", cell(p)))
                        .collect::<Vec<_>>()
                        .join(", "),
                    cell(&r.explanation)
                ));
            }
            out.push('\n');
        }
        out.push_str("## Contrato e erros\n\n");
        out.push_str("`EngineAdapter` define `parse(Artifact) -> Process`, `event(process, event)`\ne `syscall(process, call)`. Os 28 adaptadores do registro são indisponíveis:\ntoda operação retorna `{code, cause, suggestion, module_id}` com\n`code: NOT_IMPLEMENTED` e `module_id: LANG-xx`. Nenhum processo fictício é\nretornado, mesmo com `--all-features`. A cobertura DEX existente pertence a\n`rd-vm`, pelas rotas `rd vm`/`rd app`, não a estes adaptadores.\n\n");
        out.push_str("| Código | Situação |\n|---|---|\n| `NOT_IMPLEMENTED` | Operação de runtime/contrato ainda ausente (CLI exit 2) |\n| `UNKNOWN_ENGINE` | ID não registrado (exit 1) |\n| `INVALID_ENGINE_CATALOG` | Schema, IDs, módulos, ciclos, regras ou status inválidos |\n| `CATALOG_TOO_LARGE` | Catálogo maior que 1 MiB |\n| `DETECTION_LIMIT` | Mais de 100.000 entradas ZIP |\n| `IO_ERROR` | Arquivo de catálogo/APK inacessível |\n\n");
        out.push_str("Diagnósticos do relatório: `DUPLICATE_ZIP_ENTRY`,\n`NON_CANONICAL_ARTIFACT_PATH`, `UNKNOWN_ABI`, `AMBIGUOUS_SOURCE_LANGUAGE`.\nNomes duplicados contam uma vez. Evidências/diagnósticos contêm no máximo\n4 amostras por predicado/diagnóstico, mantendo a contagem total.\nTodos os 28 IDs aparecem, inclusive `not_detected`; ordem e amostras são\ndeterminísticas. `execution_claimed: false` e\n`artifact_validation: not-performed` são contratos explícitos.\n\n");
        out.push_str("## Testes e definição de conclusão\n\n");
        out.push_str("- `tests/engines/signatures.json`: exatamente 28 casos golden de DETECÇÃO,\n  um por ID. Fixtures APK/AXML/ZIP são geradas em memória; nenhum binário\n  é commitado. Testes cobrem híbridos, pistas incompletas, ambiguidades,\n  paths hostis, duplicatas, teto de entradas e amostragem.\n- Tests de contrato exercitam parse/event/syscall para os 28 itens: sempre\n  erro tipado. Builds mínimo, features individuais e todas são independentes.\n- `cargo xtask engine-report` regenera este arquivo;\n  `cargo xtask engine-spec` regenera apenas a tabela marcada no Apêndice G;\n  `cargo xtask compat-report` atualiza o placar oficial. Testes barram drift.\n- Integração OSS explícita: baixe um APK com `golden-apks/fetch.sh` e rode\n  `RD_TEST_APK=/caminho/app.apk cargo test -p rd-engine-registry --test registry\n  real_oss_apk_inventory -- --ignored`. Sem a variável/arquivo, o teste falha;\n  não há falso sucesso por pular a fixture. No teste normal ele é `ignored`.\n\n");
        out.push_str("**Registro/detecção concluídos ≠ runtime concluído.** Uma engine só pode\nmudar de `NOT_IMPLEMENTED` após crate/adaptador real feature-gated, ADR,\nparse/event/syscall funcionais, golden flow com APK OSS e gate Moto E5.\nM9/M11 continuam pendentes. Veja [ADR 003](adr/003-appendix-g-engine-registry.md)\ne [ROADMAP](ROADMAP.md).\n");
        out
    }
}
