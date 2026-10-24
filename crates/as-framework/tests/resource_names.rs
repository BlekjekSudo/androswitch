//! SDK/aapt2 metadata contracts; manifest-only renaming must not rename ARSC.
use as_framework::App;
use as_platform::{host::HostPlatform, host_fs::HostFileSystem};
use rd_vm::{Value, VmExit};
use std::{path::PathBuf, rc::Rc};
const ACTIVITY: &str = "Lorg/androswitch/contracts/ResourceNamesActivity;";
#[allow(clippy::assertions_on_constants)]
fn app(fixture: &str) -> App {
    assert!(rd_vm::PORTABLE_CORE);
    let root = std::env::temp_dir().join(format!(
        "as-resource-names-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let fs = Rc::new(HostFileSystem::new(root).unwrap());
    let pm = as_package::PackageManager::new(fs.clone());
    let apk = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../target/resource-names-fixture/{fixture}.apk"));
    let record = pm
        .install(
            std::fs::read(apk).expect("run fixtures/m9/resource-names/build.ps1"),
            true,
            None,
        )
        .unwrap();
    let mut app = App::new(
        pm.load_apk(&record.package, None).unwrap().1,
        fs,
        Rc::new(HostPlatform::default()),
        640,
        360,
        None,
    )
    .unwrap();
    app.launch().unwrap();
    app
}
fn invoke(app: &mut App, name: &str, id: i32) -> Result<Value, VmExit> {
    app.vm
        .invoke_static(ACTIVITY, name, "(I)Ljava/lang/String;", &[Value::Int(id)])
}
fn text(app: &App, value: Value) -> String {
    app.vm
        .heap
        .as_str(value.as_ref().unwrap().unwrap())
        .unwrap()
        .into()
}
fn query(app: &mut App, name: &str, id: i32) -> String {
    let value = invoke(app, name, id).unwrap();
    text(app, value)
}
fn id(app: &mut App, which: i32) -> i32 {
    app.vm
        .invoke_static(ACTIVITY, "id", "(I)I", &[Value::Int(which)])
        .unwrap()
        .as_int()
        .unwrap()
}
fn identifier(
    app: &mut App,
    name: Option<&str>,
    kind: Option<&str>,
    owner: Option<&str>,
) -> Result<i32, VmExit> {
    let args = [name, kind, owner].map(|text| match text {
        Some(text) => Value::Obj(app.vm.heap.alloc_string(text.into()).unwrap()),
        None => Value::Null,
    });
    app.vm
        .invoke_static(
            ACTIVITY,
            "identifier",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)I",
            &args,
        )
        .map(|value| value.as_int().unwrap())
}
#[test]
fn sdk_resource_package_and_names_remain_owned_by_arsc_after_manifest_only_rename() {
    for (fixture, manifest, package_id) in [
        ("resources", "org.fixture.resources", 0x7f),
        ("renamed", "net.example.manifestonly", 0x7f),
        ("high-package", "net.example.highresourceid", 0x80),
    ] {
        let mut app = app(fixture);
        assert_eq!(app.android.apk.manifest.package, manifest);
        let actual = app
            .vm
            .invoke_static(ACTIVITY, "manifest", "()Ljava/lang/String;", &[])
            .unwrap();
        assert_eq!(text(&app, actual), manifest);
        let dump = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            format!("../../target/resource-names-fixture/{fixture}-resources.txt"),
        ))
        .unwrap();
        assert!(dump.contains(&format!(
            "Package name=org.fixture.resources id={package_id:02x}"
        )));
        for (index, kind, entry) in [
            (0, "string", "greeting"),
            (1, "string", "greeting_alias"),
            (2, "integer", "answer"),
            (3, "bool", "enabled"),
            (4, "id", "anchor"),
            (5, "style", "ScopeStyle"),
            (6, "layout", "screen"),
            (7, "string", "future_only"),
            (8, "string", "localized"),
        ] {
            let resource = id(&mut app, index);
            assert_eq!((resource as u32) >> 24, package_id);
            assert_eq!(query(&mut app, "owner", resource), "org.fixture.resources");
            assert_eq!(query(&mut app, "type", resource), kind);
            assert_eq!(query(&mut app, "entry", resource), entry);
            assert_eq!(
                query(&mut app, "full", resource),
                format!("org.fixture.resources:{kind}/{entry}")
            );
            assert!(dump.contains(&format!(
                "resource 0x{:08x} {kind}/{entry}",
                resource as u32
            )));
        }
    }
}
#[test]
fn alias_identity_and_configuration_only_metadata_do_not_follow_value_resolution() {
    let mut app = app("resources");
    let original = id(&mut app, 0);
    let alias = id(&mut app, 1);
    assert_ne!(original, alias);
    let table = app.android.apk.arsc.as_ref().unwrap();
    assert_eq!(
        table.resolve_string(original as u32),
        table.resolve_string(alias as u32)
    );
    assert_eq!(query(&mut app, "entry", alias), "greeting_alias");
    assert_eq!(query(&mut app, "entry", original), "greeting");
    let future = id(&mut app, 7);
    assert_eq!(
        app.android
            .apk
            .arsc
            .as_ref()
            .unwrap()
            .resolve_value_for_api(future as u32, 26),
        None
    );
    assert_eq!(
        query(&mut app, "full", future),
        "org.fixture.resources:string/future_only"
    );
    let style = id(&mut app, 5);
    assert_eq!(
        query(&mut app, "full", style),
        "org.fixture.resources:style/ScopeStyle"
    );
}
#[test]
fn unknown_ids_raise_typed_notfound_for_each_api_without_manifest_or_highbyte_fallback() {
    for fixture in ["resources", "renamed", "high-package"] {
        let mut app = app(fixture);
        for resource in [
            0,
            -1,
            0x7f000000,
            0x7f7f0000,
            0x7e050001,
            0x01000002,
            0x80000000u32 as i32,
        ] {
            for method in ["owner", "type", "entry", "full"] {
                let error = invoke(&mut app, method, resource).unwrap_err();
                assert!(
                    matches!(error,VmExit::Exception(e) if e.class=="Landroid/content/res/Resources$NotFoundException;")
                );
            }
            assert_eq!(
                app.vm
                    .invoke_static(ACTIVITY, "missing", "(I)Z", &[Value::Int(resource)])
                    .unwrap(),
                Value::Int(1)
            );
            assert_eq!(
                app.vm
                    .invoke_static(ACTIVITY, "missingRuntime", "(I)Z", &[Value::Int(resource)])
                    .unwrap(),
                Value::Int(1)
            );
        }
    }
}
#[test]
fn only_explicit_verified_system_metadata_is_available() {
    let mut app = app("resources");
    assert_eq!(query(&mut app, "owner", 0x01020002), "android");
    assert_eq!(query(&mut app, "type", 0x01020002), "id");
    assert_eq!(query(&mut app, "entry", 0x01020002), "content");
    assert_eq!(query(&mut app, "full", 0x01020002), "android:id/content");
    // Without a full framework table, valid unsupported system IDs cannot
    // truthfully be classified missing. Do not invent metadata or absence.
    for resource in [0x0104000a, 0x0102ffff] {
        for method in ["owner", "type", "entry", "full"] {
            let error = invoke(&mut app, method, resource).unwrap_err();
            assert!(matches!(error,VmExit::Error(e) if e.code=="API_UNAVAILABLE"));
        }
    }
}
#[test]
fn reverse_lookup_preserves_alias_type_config_and_table_owner_in_all_packages() {
    for fixture in ["resources", "renamed", "high-package"] {
        let mut app = app(fixture);
        for (index, kind, entry) in [
            (0, "string", "greeting"),
            (1, "string", "greeting_alias"),
            (2, "integer", "answer"),
            (3, "bool", "enabled"),
            (4, "id", "anchor"),
            (5, "style", "ScopeStyle"),
            (6, "layout", "screen"),
            (7, "string", "future_only"),
            (8, "string", "localized"),
            (9, "integer", "greeting"),
            (10, "string", "with.dot"),
        ] {
            let actual = id(&mut app, index);
            assert_eq!(
                identifier(
                    &mut app,
                    Some(entry),
                    Some(kind),
                    Some("org.fixture.resources")
                )
                .unwrap(),
                actual
            );
            let qualified = format!("org.fixture.resources:{kind}/{entry}");
            for prefix in ["", "@", "@*"] {
                assert_eq!(
                    identifier(&mut app, Some(&format!("{prefix}{qualified}")), None, None)
                        .unwrap(),
                    actual
                );
            }
            assert_eq!(
                identifier(
                    &mut app,
                    Some(&qualified),
                    Some("wrong"),
                    Some("wrong.owner")
                )
                .unwrap(),
                actual
            );
            assert_eq!(
                identifier(
                    &mut app,
                    Some(&format!("{kind}/{entry}")),
                    None,
                    Some("org.fixture.resources")
                )
                .unwrap(),
                actual
            );
            assert_eq!(
                identifier(
                    &mut app,
                    Some(&format!("org.fixture.resources:{entry}")),
                    Some(kind),
                    None
                )
                .unwrap(),
                actual
            );
        }
        // A renamed manifest never renames the package in the resource table.
        let manifest = app.android.apk.manifest.package.clone();
        if manifest != "org.fixture.resources" {
            assert_eq!(
                identifier(&mut app, Some("greeting"), Some("string"), Some(&manifest)).unwrap(),
                0
            );
        }
        assert_eq!(
            identifier(&mut app, Some("greeting"), Some("string"), None).unwrap(),
            0
        );
        assert_ne!(id(&mut app, 0), id(&mut app, 1));
        assert_ne!(id(&mut app, 0), id(&mut app, 9));
    }
}
#[test]
fn absent_malformed_and_null_names_use_android_defaults_without_inventing_resources() {
    let mut app = app("resources");
    for name in [
        "",
        "missing",
        "google_app_id",
        "Greeting",
        "?string/greeting",
        "@+string/greeting",
        "org.fixture.resources:string/",
        "org.fixture.resources:/greeting",
        ":string/greeting",
        "wrong.owner:string/greeting",
        "org.fixture.resources:wrong/greeting",
    ] {
        assert_eq!(
            identifier(
                &mut app,
                Some(name),
                Some("string"),
                Some("org.fixture.resources")
            )
            .unwrap(),
            0,
            "{name}"
        );
    }
    assert_eq!(
        identifier(
            &mut app,
            Some("greeting"),
            None,
            Some("org.fixture.resources")
        )
        .unwrap(),
        0
    );
    let error = identifier(&mut app, None, None, None).unwrap_err();
    assert!(matches!(error,VmExit::Exception(e) if e.class=="Ljava/lang/NullPointerException;"));
    assert_eq!(
        identifier(&mut app, Some("android:id/content"), None, None).unwrap(),
        0x01020002
    );
    for name in ["android:string/ok", "android:id/unknown"] {
        assert!(
            matches!(identifier(&mut app,Some(name),None,None),Err(VmExit::Error(e)) if e.code=="API_UNAVAILABLE")
        );
    }
    assert!(
        matches!(identifier(&mut app,Some(&"x".repeat(4097)),None,None),Err(VmExit::Error(e)) if e.code=="RESOURCE_NAME_BUDGET")
    );
    assert!(
        matches!(identifier(&mut app,Some("greeting"),Some("string"),Some(&"x".repeat(4097))),Err(VmExit::Error(e)) if e.code=="RESOURCE_NAME_BUDGET")
    );
    assert!(
        matches!(identifier(&mut app,Some("^index_0"),None,None),Err(VmExit::Error(e)) if e.code=="API_UNAVAILABLE")
    );
}
#[test]
fn raw_signed_numeric_identifiers_match_java_over_api26_unicode_digit_ranges() {
    let oracle = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/resource-names-fixture/identifier-jvm-oracle.txt"),
    )
    .unwrap();
    assert_eq!(oracle.lines().count(), 97);
    for fixture in ["resources", "renamed", "high-package"] {
        let mut app = app(fixture);
        for line in oracle.lines() {
            let columns: Vec<_> = line.split('\t').collect();
            let units: Vec<u16> = if columns[0].is_empty() {
                vec![]
            } else {
                columns[0]
                    .split(',')
                    .map(|n| u16::from_str_radix(n, 16).unwrap())
                    .collect()
            };
            let name = String::from_utf16(&units).unwrap();
            let expected = columns[2].parse::<i32>().unwrap();
            assert_eq!(
                identifier(
                    &mut app,
                    Some(&name),
                    Some("string"),
                    Some("org.fixture.resources")
                )
                .unwrap(),
                expected,
                "{fixture}: {columns:?}"
            );
        }
        // Decimal IDs need not refer to an existing resource. Defaults do not
        // participate in the first Integer.parseInt path, including nulls.
        assert_eq!(identifier(&mut app, Some("123"), None, None).unwrap(), 123);
        assert_eq!(
            identifier(&mut app, Some("-2147483648"), None, None).unwrap(),
            i32::MIN
        );
    }
}
