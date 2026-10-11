//! issue #75: o bloco de exemplo do `rd inspect` no README precisa ser igual
//! à saída REAL do binário para o APK de referência (F-Droid 2.0.1).
//!
//! O APK golden não é versionado (política GH-05: nenhum binário no repo) e
//! não é baixado pelo CI — então o teste roda quando o APK está presente
//! (alguém executou `./golden-apks/fetch.sh`) e se ignora quando não está.
//! É o mesmo papel do `generated_docs_match_catalog` do rd-agent: a
//! documentação é um artefato verificável, não memória oral.

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Extrai as linhas do primeiro bloco ```console … ``` do README, sem o `$`.
fn readme_console_block() -> Vec<String> {
    let text = std::fs::read_to_string(repo_root().join("README.md")).expect("README legível");
    let mut inside = false;
    let mut block = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```console") {
            inside = true;
            continue;
        }
        if inside && line.trim_start().starts_with("```") {
            break;
        }
        if inside {
            block.push(line.to_string());
        }
    }
    assert!(
        !block.is_empty(),
        "bloco ```console``` não encontrado no README"
    );
    block
        .into_iter()
        .filter(|l| !l.trim_start().starts_with('$'))
        .map(|l| l.to_string())
        .collect()
}

#[test]
fn readme_inspect_example_matches_binary_output() {
    let apk = repo_root().join("golden-apks/org.fdroid.fdroid.apk");
    if !apk.exists() {
        eprintln!(
            "PULANDO: {} ausente — rode ./golden-apks/fetch.sh para checar o exemplo do README",
            apk.display()
        );
        return;
    }

    let out = Command::new(env!("CARGO_BIN_EXE_rd"))
        .args(["inspect", &apk.to_string_lossy()])
        .output()
        .expect("spawn rd");
    assert!(
        out.status.success(),
        "rd inspect falhou: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let actual: Vec<&str> = stdout.lines().collect();

    // cada linha do exemplo (na ordem do bloco) tem de existir na saída real
    let mut cursor = 0usize;
    for expected in readme_console_block() {
        let found = actual[cursor..]
            .iter()
            .position(|l| *l == expected)
            .map(|p| cursor + p);
        match found {
            Some(pos) => cursor = pos + 1,
            None => panic!(
                "issue #75: linha do README ausente na saída atual do `rd inspect`\n\
                 README : {expected:?}\n\
                 saída  :\n{stdout}"
            ),
        }
    }
}
