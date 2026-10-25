#![cfg(feature = "native-execution")]
//! Issue #260 — native entry selection must follow AOSP component semantics:
//! only a real `<activity>` satisfies the launcher probe (a `<service>` or
//! `<receiver>` with an injected MAIN/LAUNCHER filter must not hijack it), and
//! the MCP route (pre-resolved descriptor, including `<activity-alias>` targets
//! and explicit non-launcher activities) must not be re-validated against a
//! launcher requirement it never promised.

use rd_engine_registry::NativeProcess;
use rd_testsupport::apkfix::{a, build_axml, AxNode, AxVal, ZipFix};
use rd_testsupport::vm::DexBuilder;
use serde_json::json;
use std::sync::Arc;

fn typed(error: rd_apk::RdError, code: &str) {
    assert_eq!(error.code, code);
    assert!(!error.cause.is_empty());
    assert!(error.suggestion.is_some());
    assert!(!error.module_id.is_empty());
}

fn launcher_filter() -> AxNode {
    AxNode::new(
        "intent-filter",
        vec![],
        vec![
            AxNode::new(
                "action",
                vec![a(
                    true,
                    "name",
                    AxVal::Str("android.intent.action.MAIN".to_string()),
                )],
                vec![],
            ),
            AxNode::new(
                "category",
                vec![a(
                    true,
                    "name",
                    AxVal::Str("android.intent.category.LAUNCHER".to_string()),
                )],
                vec![],
            ),
        ],
    )
}

fn manifest(min_sdk: i32, components: Vec<AxNode>) -> Vec<u8> {
    let root = AxNode::new(
        "manifest",
        vec![
            a(false, "package", AxVal::Str("com.test".to_string())),
            a(true, "versionCode", AxVal::Int(1)),
        ],
        vec![
            AxNode::new(
                "uses-sdk",
                vec![
                    a(true, "minSdkVersion", AxVal::Int(min_sdk)),
                    a(true, "targetSdkVersion", AxVal::Int(34)),
                ],
                vec![],
            ),
            AxNode::new("application", vec![], components),
        ],
    );
    build_axml(&root)
}

/// Same deterministic fixture policy as the sibling suites: no binary blobs,
/// every APK is rebuilt from primitives (AXML + DexBuilder + fake lib entry).
fn native_apk(manifest_bytes: Vec<u8>) -> Vec<u8> {
    let mut b = DexBuilder::new();
    b.class("Lcom/test/RealEntry;", "Ljava/lang/Object;");
    b.class("Lcom/test/Bench;", "Ljava/lang/Object;");
    b.class("Lcom/test/GhostService;", "Ljava/lang/Object;");
    b.class("Lcom/test/AliasTarget;", "Ljava/lang/Object;");
    let mut z = ZipFix::new();
    z.add("AndroidManifest.xml", manifest_bytes);
    z.add("classes.dex", b.finish());
    z.add(
        "lib/x86_64/fixture.so",
        b"fixture; metadata only, never executed".to_vec(),
    );
    z.build()
}

#[test]
fn service_or_receiver_with_injected_launcher_filter_does_not_shadow_the_activity() {
    let components = vec![
        AxNode::new(
            "service",
            vec![a(
                true,
                "name",
                AxVal::Str("com.test.GhostService".to_string()),
            )],
            vec![launcher_filter()],
        ),
        AxNode::new(
            "receiver",
            vec![a(
                true,
                "name",
                AxVal::Str("com.test.GhostReceiver".to_string()),
            )],
            vec![launcher_filter()],
        ),
        AxNode::new(
            "activity",
            vec![a(
                true,
                "name",
                AxVal::Str("com.test.RealEntry".to_string()),
            )],
            vec![launcher_filter()],
        ),
    ];
    let apk = rd_apk::Apk::from_bytes(native_apk(manifest(26, components))).unwrap();
    let process = NativeProcess::from_apk("rust-ndk", apk).unwrap();
    assert_eq!(
        process.report()["activity"],
        json!("Lcom/test/RealEntry;"),
        "issue #260: a <service>/<receiver> declared before the real <activity> must not be selected as the launch component"
    );
}

#[test]
fn default_launcher_activity_happy_path_is_preserved() {
    let components = vec![AxNode::new(
        "activity",
        vec![a(
            true,
            "name",
            AxVal::Str("com.test.RealEntry".to_string()),
        )],
        vec![launcher_filter()],
    )];
    let apk = rd_apk::Apk::from_bytes(native_apk(manifest(26, components))).unwrap();
    let process = NativeProcess::from_apk("rust-ndk", apk).unwrap();
    assert_eq!(process.report()["activity"], json!("Lcom/test/RealEntry;"));
}

#[test]
fn artifact_route_still_requires_a_real_launcher_activity() {
    // No launcher at all: the artifact/adapter route cannot guess an entry.
    let components = vec![AxNode::new(
        "activity",
        vec![a(true, "name", AxVal::Str("com.test.Bench".to_string()))],
        vec![],
    )];
    let apk = rd_apk::Apk::from_bytes(native_apk(manifest(26, components))).unwrap();
    typed(
        NativeProcess::from_apk("rust-ndk", apk).unwrap_err(),
        "MISSING_ENTRY",
    );
}

#[test]
fn alias_launcher_stays_rejected_on_artifact_route_but_works_via_mcp_desc() {
    let components = vec![
        AxNode::new(
            "activity",
            vec![a(
                true,
                "name",
                AxVal::Str("com.test.AliasTarget".to_string()),
            )],
            vec![],
        ),
        AxNode::new(
            "activity-alias",
            vec![
                a(true, "name", AxVal::Str("com.test.EntryAlias".to_string())),
                a(
                    true,
                    "targetActivity",
                    AxVal::Str("com.test.AliasTarget".to_string()),
                ),
            ],
            vec![launcher_filter()],
        ),
    ];
    let apk = rd_apk::Apk::from_bytes(native_apk(manifest(26, components))).unwrap();
    // Artifact route: the alias carries the launcher filter, but from_apk
    // cannot resolve targets — the honest error names the missing activity.
    typed(
        NativeProcess::from_apk("rust-ndk", apk.clone()).unwrap_err(),
        "MISSING_ENTRY",
    );
    // MCP route (issue #109 resolution): the descriptor is the alias TARGET.
    let process =
        NativeProcess::from_apk_with_activity("rust-ndk", Arc::new(apk), "Lcom/test/AliasTarget;")
            .unwrap();
    assert_eq!(
        process.report()["activity"],
        json!("Lcom/test/AliasTarget;")
    );
}

#[test]
fn mcp_route_launches_explicit_non_launcher_activity_without_a_launcher_probe() {
    let components = vec![AxNode::new(
        "activity",
        vec![a(true, "name", AxVal::Str("com.test.Bench".to_string()))],
        vec![],
    )];
    let apk = rd_apk::Apk::from_bytes(native_apk(manifest(26, components))).unwrap();
    let process =
        NativeProcess::from_apk_with_activity("rust-ndk", Arc::new(apk), "Lcom/test/Bench;")
            .unwrap();
    assert_eq!(process.report()["activity"], json!("Lcom/test/Bench;"));
}

#[test]
fn mcp_route_keeps_the_structural_apk_gates() {
    // minSdk floor is enforced on the shared-Arc route too.
    let components = vec![AxNode::new(
        "activity",
        vec![a(true, "name", AxVal::Str("com.test.Bench".to_string()))],
        vec![],
    )];
    let apk = rd_apk::Apk::from_bytes(native_apk(manifest(25, components))).unwrap();
    typed(
        NativeProcess::from_apk_with_activity("rust-ndk", Arc::new(apk), "Lcom/test/Bench;")
            .unwrap_err(),
        "UNSUPPORTED_API_FLOOR",
    );
    // Adapter id is still validated first (NOT_IMPLEMENTED precedes the desc).
    let components = vec![AxNode::new(
        "activity",
        vec![a(
            true,
            "name",
            AxVal::Str("com.test.RealEntry".to_string()),
        )],
        vec![launcher_filter()],
    )];
    let apk = rd_apk::Apk::from_bytes(native_apk(manifest(26, components))).unwrap();
    typed(
        NativeProcess::from_apk_with_activity("not-an-engine", Arc::new(apk), "Lcom/test/Bench;")
            .unwrap_err(),
        "NOT_IMPLEMENTED",
    );
}

#[test]
fn mcp_route_rejects_descriptors_that_cannot_name_a_class() {
    let components = vec![AxNode::new(
        "activity",
        vec![a(
            true,
            "name",
            AxVal::Str("com.test.RealEntry".to_string()),
        )],
        vec![launcher_filter()],
    )];
    let apk = rd_apk::Apk::from_bytes(native_apk(manifest(26, components))).unwrap();
    let shared = Arc::new(apk);
    for bad in [
        "",
        "com.test.Bench",
        "Lcom/test/Bench",
        "L;",
        "Lcom//Bench;",
        "L/Bench;",
        "L../Bench;",
        "Lcom/../Bench;",
        "Lcom/./Bench;",
    ] {
        let error =
            NativeProcess::from_apk_with_activity("rust-ndk", shared.clone(), bad).unwrap_err();
        assert_eq!(error.code, "INVALID_FORMAT", "descriptor {bad:?}");
        assert!(
            error.cause.contains(bad),
            "diagnostic must echo the rejected descriptor: {:?}",
            error.cause
        );
    }
}
