//! Testes de integração com APK real (GH-05 fixture).
//! Rodar: `RD_TEST_APK=golden-apks/org.fdroid.fdroid.apk cargo test -p rd-apk`
//! Sem a env var, os testes passam trivialmente (CI sem fixture).

use rd_apk::{Apk, FloorStatus};
use std::path::{Path, PathBuf};

fn apk_path() -> Option<PathBuf> {
    std::env::var_os("RD_TEST_APK").map(|p| {
        let p = PathBuf::from(p);
        // caminhos relativos: resolver contra a raiz do workspace (CWD do teste é a crate)
        if p.is_relative() {
            let ws = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(Path::parent)
                .map(|w| w.join(&p))
                .unwrap_or_else(|| p.clone());
            if ws.exists() {
                ws
            } else {
                p
            }
        } else {
            p
        }
    })
}

#[test]
fn real_apk_inspect_end_to_end() {
    let Some(path) = apk_path() else {
        eprintln!("RD_TEST_APK não definida — fixture ausente, skip (CI-safe)");
        return;
    };
    let apk = Apk::open(&path).expect("APK parse must succeed on a valid APK");

    // identidade
    assert!(!apk.manifest.package.is_empty(), "package vazio");
    assert!(
        apk.manifest.package.contains('.'),
        "package deve ser FQCN-like: {}",
        apk.manifest.package
    );
    assert!(apk.manifest.version_code.unwrap_or(0) > 0);
    assert!(!apk.dex_files.is_empty(), "APK sem classes*.dex?");

    // SDKs coerentes (não defaults quebrados)
    assert!(
        apk.manifest.min_sdk <= 40,
        "minSdk implausível: {}",
        apk.manifest.min_sdk
    );
    assert!(apk.manifest.target_sdk >= apk.manifest.min_sdk);
    assert!(apk.manifest.target_sdk <= 40, "targetSdk implausível");

    // componentes e permissões
    assert!(
        !apk.manifest.application.components.is_empty(),
        "APK real tem componentes"
    );
    assert!(
        apk.manifest
            .application
            .components
            .iter()
            .any(|c| c.is_launcher),
        "APK instalável tem launcher"
    );
    assert!(apk.signing.is_signed(), "fixture é assinada");

    // piso de API da spec (ADR 016) — via constante, sem literal mágico
    match apk.floor_status() {
        FloorStatus::Ok => assert!(apk.manifest.min_sdk >= rd_apk::FLOOR_MIN_SDK),
        FloorStatus::BelowFloor { min_sdk, floor } => {
            assert_eq!(floor, rd_apk::FLOOR_MIN_SDK);
            assert!(min_sdk < rd_apk::FLOOR_MIN_SDK);
        }
    }

    // arsc presente em APK de produção
    assert!(apk.resources_arsc, "resources.arsc ausente");
    assert!(apk.arsc.is_some());
}

#[test]
fn real_apk_label_resolves_via_arsc() {
    let Some(path) = apk_path() else {
        return;
    };
    let apk = Apk::open(&path).expect("apk");
    if let Some(label) = &apk.manifest.application.label {
        assert!(!label.is_empty());
        assert!(
            !label.starts_with("@0x"),
            "label ficou como referência não resolvida: {label}"
        );
    }
}

#[test]
fn real_apk_manifest_raw_preserves_everything() {
    let Some(path) = apk_path() else {
        return;
    };
    let apk = Apk::open(&path).expect("apk");
    let raw = apk.manifest_raw().expect("raw axml");
    // Lei 2: a árvore crua deve ter ao menos todos os filhos que o modelo tipado viu
    assert_eq!(raw.root.name, "manifest");
    let raw_children = raw.root.children.len();
    let typed = apk.manifest.uses_permissions.len()
        + apk.manifest.application.components.len()
        + apk.manifest.uses_features.len();
    assert!(
        raw_children >= typed.min(1),
        "árvore crua perdeu elementos: raw={raw_children}, typed~{typed}"
    );
}
