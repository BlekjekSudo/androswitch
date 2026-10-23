use as_platform::host::HostPlatform;
use as_runtime::{Runtime, RUNTIME_API};
use as_testsupport::apkfix::{
    a, build_apk_sdk, build_axml, build_manifest_axml_sdk, ArscFix, AxNode, AxVal, ZipFix,
};

fn inspect(bytes: Vec<u8>) -> as_runtime::InspectReport {
    Runtime::<HostPlatform>::inspect_bytes(bytes).unwrap()
}

fn apk(package: &str, min: i32, target: i32) -> Vec<u8> {
    build_apk_sdk(
        package,
        &format!("{package}.MainActivity"),
        ArscFix::new().build(),
        vec![],
        min,
        target,
    )
}

fn with_native(abis: &[&str]) -> Vec<u8> {
    let mut zip = ZipFix::new();
    zip.add(
        "AndroidManifest.xml",
        build_manifest_axml_sdk("org.example.native", "MainActivity", 21, 35),
    );
    for abi in abis {
        zip.add(&format!("lib/{abi}/libexample.so"), vec![1, 2, 3]);
    }
    zip.build()
}

#[test]
fn legacy_min_sdk_and_higher_target_are_allowed_without_claiming_execution() {
    for min_sdk in [1, 19, 21, 24, 26] {
        let report = inspect(apk("org.example.legacy", min_sdk, 37));
        assert!(report.preflight.sdk_allowed);
        assert!(report.preflight.static_requirements_met);
        assert_eq!(report.preflight.runtime_api, RUNTIME_API);
        assert!(!report.preflight.execution_implemented);
        assert!(report.preflight.install_implemented);
        assert!(!report.preflight.device_verified);
    }
}

#[test]
fn newer_min_sdk_is_a_preflight_blocker_but_inspection_succeeds() {
    let report = inspect(apk("org.example.newer", 27, 35));
    assert_eq!(report.status, "ok");
    assert!(!report.preflight.sdk_allowed);
    assert_eq!(report.preflight.blockers[0].code, "SDK_TOO_NEW");
}

#[test]
fn native_preflight_selects_arm64_without_requiring_every_abi() {
    assert!(
        !inspect(with_native(&["armeabi-v7a", "x86_64"]))
            .preflight
            .native_abi_allowed
    );
    assert!(
        inspect(with_native(&["arm64-v8a", "armeabi-v7a"]))
            .preflight
            .native_abi_allowed
    );
    assert!(
        inspect(apk("org.example.pure", 21, 35))
            .preflight
            .native_abi_allowed
    );
}

#[test]
fn multiple_dex_files_are_preserved_as_inventory_not_execution_evidence() {
    let mut zip = ZipFix::new();
    zip.add(
        "AndroidManifest.xml",
        build_manifest_axml_sdk("org.example.multi", "Main", 21, 35),
    );
    zip.add("classes.dex", vec![0]);
    zip.add("classes2.dex", vec![0]);
    let report = inspect(zip.build());
    assert_eq!(report.inventory.dex_files.len(), 2);
    assert!(!report.preflight.execution_implemented);
}

#[test]
fn split_markers_are_reported_for_split_base_and_required_split_metadata() {
    let cases = [
        AxNode::new(
            "manifest",
            vec![
                a(false, "package", AxVal::Str("org.example.split".into())),
                a(false, "split", AxVal::Str("config.arm64_v8a".into())),
            ],
            vec![],
        ),
        AxNode::new(
            "manifest",
            vec![a(false, "package", AxVal::Str("org.example.split".into()))],
            vec![AxNode::new(
                "uses-split",
                vec![a(true, "name", AxVal::Str("feature".into()))],
                vec![],
            )],
        ),
        AxNode::new(
            "manifest",
            vec![a(false, "package", AxVal::Str("org.example.split".into()))],
            vec![AxNode::new(
                "application",
                vec![a(true, "isSplitRequired", AxVal::Bool(true))],
                vec![],
            )],
        ),
        AxNode::new(
            "manifest",
            vec![a(false, "package", AxVal::Str("org.example.split".into()))],
            vec![AxNode::new(
                "application",
                vec![],
                vec![AxNode::new(
                    "meta-data",
                    vec![
                        a(
                            true,
                            "name",
                            AxVal::Str("com.android.vending.splits.required".into()),
                        ),
                        a(true, "value", AxVal::Bool(true)),
                    ],
                    vec![],
                )],
            )],
        ),
    ];
    for manifest in cases {
        let mut zip = ZipFix::new();
        zip.add("AndroidManifest.xml", build_axml(&manifest));
        let report = inspect(zip.build());
        assert!(!report.preflight.single_apk);
        assert!(report
            .preflight
            .blockers
            .iter()
            .any(|gap| gap.code == "SPLIT_APK_REQUIRED"));
    }
}

#[test]
fn package_renaming_does_not_change_runtime_policy() {
    for package in [
        "org.example.first",
        "net.different.second",
        "com.example.third",
    ] {
        let report = inspect(apk(package, 21, 35));
        assert_eq!(report.manifest.package, package);
        assert!(report.preflight.static_requirements_met);
    }
}

#[test]
fn invalid_apk_returns_structured_parser_error() {
    let error = Runtime::<HostPlatform>::inspect_bytes(b"not an apk".to_vec()).unwrap_err();
    assert_eq!(error.operation, "inspect");
    assert_eq!(error.module_id, "rd-apk");
    assert!(!error.code.is_empty());
}

#[test]
#[ignore = "requires the SHA-256-pinned external F-Droid fixture; run scripts/fetch-fixtures.ps1 first"]
fn real_fdroid_apk_is_inspected_without_upstream_min_sdk_floor() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/cache/org.fdroid.fdroid_2000052.apk");
    let report = Runtime::default()
        .inspect(path.to_str().unwrap())
        .expect("required real APK fixture");
    assert_eq!(report.manifest.package, "org.fdroid.fdroid");
    assert_eq!(report.manifest.version_code, Some(2000052));
    assert_eq!(report.inventory.file_size_bytes, 12_569_951);
    assert_eq!(report.manifest.min_sdk, 24);
    assert!(report.preflight.sdk_allowed);
    assert!(!report.signing.detected_schemes.is_empty());
    assert!(!report.signing.cryptographically_verified);
    assert!(!report.preflight.execution_implemented);
}
