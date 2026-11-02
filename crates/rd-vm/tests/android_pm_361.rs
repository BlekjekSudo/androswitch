//! Issue #361 — PackageManager.getPackageInfo respeita os bits GET_* e
//! popula activities/services/receivers/providers/permissions/
//! requestedPermissions a partir do MANIFESTO (antes: arrays sempre Null e
//! flags ignorados — APK-Tester Behemoth renderizava "activities=0
//! services=0 receivers=0 providers=0" com todos os GET_* pedidos).
//!
//! Semântica AOSP coberta: flag não setada → null; flag setada → array
//! alocado (mesmo vazio); activities inclui <activity-alias>; receivers
//! também são ActivityInfo[]; enabled=false só entra com
//! MATCH_DISABLED_COMPONENTS; requestedPermissionsFlags REQUIRED|GRANTED;
//! forma API 33 (PackageInfoFlags com backing long) lê o mesmo valor.

mod common;

use common::*;
use rd_testsupport::apkfix::{a, build_axml, AxNode, AxVal, ZipFix};
use rd_vm::heap::ObjRef;
use rd_vm::intrinsics::call_instance_intrinsic;
use rd_vm::value::Value;
use rd_vm::{Engine, VmConfig};

const APM: &str = "Landroid/app/ApplicationPackageManager;";
const PKG: &str = "com.test.pm";
const MAIN_CLASS: &str = "com.test.pm.Main";
const ALIAS_CLASS: &str = "com.test.pm.LauncherAlias";

const GET_ACTIVITIES: i32 = 0x1;
const GET_RECEIVERS: i32 = 0x2;
const GET_SERVICES: i32 = 0x4;
const GET_PROVIDERS: i32 = 0x8;
const GET_PERMISSIONS: i32 = 0x1000;
const MATCH_DISABLED_COMPONENTS: i32 = 0x200;

fn ax(name: &str, val: AxVal) -> rd_testsupport::apkfix::AxAttr {
    a(true, name, val)
}

fn manifest() -> Vec<u8> {
    let app = AxNode::new(
        "application",
        vec![],
        vec![
            AxNode::new(
                "activity",
                vec![ax("name", AxVal::Str(".Main".into()))],
                vec![],
            ),
            AxNode::new(
                "activity-alias",
                vec![
                    ax("name", AxVal::Str(".LauncherAlias".into())),
                    ax("targetActivity", AxVal::Str(".Main".into())),
                    ax("exported", AxVal::Bool(true)),
                ],
                vec![],
            ),
            AxNode::new(
                "service",
                vec![
                    ax("name", AxVal::Str(".Svc".into())),
                    ax("exported", AxVal::Bool(true)),
                    ax("permission", AxVal::Str("android.permission.BIND".into())),
                ],
                vec![],
            ),
            AxNode::new(
                "receiver",
                vec![
                    ax("name", AxVal::Str(".Rcv".into())),
                    ax("enabled", AxVal::Bool(false)),
                    ax("exported", AxVal::Bool(true)),
                ],
                vec![],
            ),
            AxNode::new(
                "provider",
                vec![
                    ax("name", AxVal::Str(".Prov".into())),
                    ax("authorities", AxVal::Str("com.test.pm.files".into())),
                    ax("exported", AxVal::Bool(true)),
                ],
                vec![],
            ),
        ],
    );
    let root = AxNode::new(
        "manifest",
        vec![
            a(false, "package", AxVal::Str(PKG.into())),
            ax("versionCode", AxVal::Int(9)),
        ],
        vec![
            AxNode::new(
                "uses-sdk",
                vec![
                    ax("minSdkVersion", AxVal::Int(29)),
                    ax("targetSdkVersion", AxVal::Int(34)),
                ],
                vec![],
            ),
            AxNode::new(
                "uses-permission",
                vec![ax("name", AxVal::Str("android.permission.INTERNET".into()))],
                vec![],
            ),
            AxNode::new(
                "uses-permission",
                vec![ax("name", AxVal::Str("android.permission.CAMERA".into()))],
                vec![],
            ),
            AxNode::new(
                "permission",
                vec![
                    ax("name", AxVal::Str("com.test.pm.PERM".into())),
                    ax("protectionLevel", AxVal::Int(2)),
                ],
                vec![],
            ),
            app,
        ],
    );
    build_axml(&root)
}

fn pm_vm() -> Engine {
    let mut b = DexBuilder::new();
    b.class("LCaso;", "Ljava/lang/Object;");
    let dex = rd_dex::Dex::parse(b.finish()).expect("DEX sintético parseia");
    let mut z = ZipFix::new();
    z.add("AndroidManifest.xml", manifest());
    z.add("classes.dex", b.finish());
    z.add("resources.arsc", apkfix::ArscFix::new().build());
    let apk = rd_apk::Apk::from_bytes(z.build()).expect("APK sintético parseia");
    let mut vm = Engine::new(vec![dex], VmConfig::default());
    vm.set_resources(apk);
    vm
}

fn get_pi(vm: &mut Engine, flags: Value) -> ObjRef {
    let apm = vm
        .heap
        .alloc_instance(APM.to_string(), Vec::new())
        .expect("APM aloca");
    let pkg = Value::Obj(vm.heap.alloc_string(PKG.to_string()).unwrap());
    let pi = call_instance_intrinsic(
        vm,
        APM,
        "getPackageInfo",
        "(Ljava/lang/String;I)Landroid/content/pm/PackageInfo;",
        apm,
        &[pkg, flags],
    )
    .expect("getPackageInfo resolve")
    .expect("getPackageInfo devolve PackageInfo");
    match pi {
        Value::Obj(r) => r,
        other => panic!("esperava PackageInfo Obj, veio {other:?}"),
    }
}

/// Lê o campo array do PackageInfo: None = campo Null (AOSP flag off).
fn arr_field(vm: &Engine, pi: ObjRef, field: &str) -> Option<Vec<Value>> {
    match vm.heap.get_field(pi, field).expect("campo existe") {
        Value::Null => None,
        Value::Obj(r) => Some(vm.heap.as_array_elems(r).expect("array").0.to_vec()),
        other => panic!("{field} não é array/null: {other:?}"),
    }
}

fn strings_of(vm: &Engine, items: &[Value]) -> Vec<String> {
    items
        .iter()
        .map(|v| match v {
            Value::Obj(r) => vm.heap.as_str(*r).unwrap().to_string(),
            other => panic!("item não é String: {other:?}"),
        })
        .collect()
}

fn names_of(vm: &Engine, items: &[Value]) -> Vec<String> {
    items
        .iter()
        .map(|v| match v {
            Value::Obj(r) => match vm.heap.get_field(*r, "name").expect("name") {
                Value::Obj(s) => vm.heap.as_str(s).unwrap().to_string(),
                other => panic!("name não é string: {other:?}"),
            },
            other => panic!("item não é Obj: {other:?}"),
        })
        .collect()
}

fn ints_of(_vm: &Engine, items: &[Value]) -> Vec<i32> {
    items
        .iter()
        .map(|v| v.as_int().unwrap_or_else(|_| panic!("não é int: {v:?}")))
        .collect()
}

/// flags=0 (só GET_META_DATA implícito): TODOS os arrays Null, como no AOSP.
#[test]
fn zero_flags_leaves_all_arrays_null() {
    let mut vm = pm_vm();
    let pi = get_pi(&mut vm, Value::Int(0));
    for f in [
        "activities",
        "services",
        "receivers",
        "providers",
        "permissions",
        "requestedPermissions",
    ] {
        assert!(
            arr_field(&vm, pi, f).is_none(),
            "{f} deve ser Null com flags=0"
        );
    }
}

/// GET_ACTIVITIES|GET_SERVICES|GET_RECEIVERS|GET_PROVIDERS: activities=2
/// (Main + alias), services=1, receivers=0 (disabled sem MATCH), providers=1.
#[test]
fn component_arrays_follow_get_flags_and_manifest() {
    let mut vm = pm_vm();
    let flags = GET_ACTIVITIES | GET_SERVICES | GET_RECEIVERS | GET_PROVIDERS;
    let pi = get_pi(&mut vm, Value::Int(flags));

    let acts = arr_field(&vm, pi, "activities").expect("activities não-null");
    let mut names = names_of(&vm, &acts);
    names.sort();
    assert_eq!(
        names,
        vec![ALIAS_CLASS.to_string(), MAIN_CLASS.to_string()],
        "activities inclui <activity> E <activity-alias>"
    );

    let svcs = arr_field(&vm, pi, "services").expect("services não-null");
    assert_eq!(names_of(&vm, &svcs), vec!["com.test.pm.Svc".to_string()]);

    assert!(
        arr_field(&vm, pi, "receivers")
            .expect("flag setada → array alocado mesmo vazio (AOSP)")
            .is_empty(),
        "único receiver está disabled e MATCH_DISABLED_COMPONENTS não foi pedido"
    );

    let provs = arr_field(&vm, pi, "providers").expect("providers não-null");
    assert_eq!(names_of(&vm, &provs), vec!["com.test.pm.Prov".to_string()]);
    let authority = match &provs[0] {
        Value::Obj(r) => match vm.heap.get_field(*r, "authority").expect("authority") {
            Value::Obj(s) => vm.heap.as_str(s).unwrap().to_string(),
            other => panic!("authority não é string: {other:?}"),
        },
        other => panic!("provider não é Obj: {other:?}"),
    };
    assert_eq!(authority, "com.test.pm.files", "authority vem do manifesto");

    // service.exported vem do manifesto (antes: 0 hardcoded)
    let exported = match &svcs[0] {
        Value::Obj(r) => vm
            .heap
            .get_field(*r, "exported")
            .expect("exported")
            .as_int()
            .expect("exported int"),
        other => panic!("service não é Obj: {other:?}"),
    };
    assert_eq!(exported, 1, "exported_effective do manifesto");
}

/// MATCH_DISABLED_COMPONENTS inclui o receiver enabled=false.
#[test]
fn match_disabled_components_includes_disabled_receiver() {
    let mut vm = pm_vm();
    let flags = GET_RECEIVERS | MATCH_DISABLED_COMPONENTS;
    let pi = get_pi(&mut vm, Value::Int(flags));
    let rcv = arr_field(&vm, pi, "receivers").expect("receivers não-null");
    assert_eq!(
        names_of(&vm, &rcv),
        vec!["com.test.pm.Rcv".to_string()],
        "disabled entra só com MATCH_DISABLED_COMPONENTS"
    );
}

/// GET_PERMISSIONS: requestedPermissions = uses-permissions na ordem do
/// manifesto; requestedPermissionsFlags REQUIRED|GRANTED; permissions =
/// <permission> declarados.
#[test]
fn get_permissions_populates_requested_and_declared() {
    let mut vm = pm_vm();
    let pi = get_pi(&mut vm, Value::Int(GET_PERMISSIONS));

    let req = arr_field(&vm, pi, "requestedPermissions").expect("requestedPermissions");
    assert_eq!(
        strings_of(&vm, &req),
        vec![
            "android.permission.INTERNET".to_string(),
            "android.permission.CAMERA".to_string(),
        ]
    );

    let flags = arr_field(&vm, pi, "requestedPermissionsFlags").expect("flags paralelo");
    assert_eq!(ints_of(&vm, &flags), vec![3, 3], "REQUIRED(1)|GRANTED(2)");

    let perms = arr_field(&vm, pi, "permissions").expect("permissions declaradas");
    assert_eq!(names_of(&vm, &perms), vec!["com.test.pm.PERM".to_string()]);
}

/// Flags parciais: só o bit pedido é populado; os demais continuam Null
/// (GET_ACTIVITIES sozinho → services/providers/permissões Null).
#[test]
fn unset_flags_stay_null_with_partial_request() {
    let mut vm = pm_vm();
    let pi = get_pi(&mut vm, Value::Int(GET_ACTIVITIES));
    assert!(arr_field(&vm, pi, "activities").is_some());
    assert!(arr_field(&vm, pi, "services").is_none());
    assert!(arr_field(&vm, pi, "providers").is_none());
    assert!(arr_field(&vm, pi, "requestedPermissions").is_none());
}

/// Forma API 33: PackageInfoFlags com backing long `value` — os MESMOS
/// bits produzem o MESMO PackageInfo.
#[test]
fn package_info_flags_object_reads_backing_long() {
    let mut vm = pm_vm();
    let pif = vm
        .heap
        .alloc_instance(
            "Landroid/content/pm/PackageManager$PackageInfoFlags;".to_string(),
            vec![(
                "value".to_string(),
                Value::Long((GET_ACTIVITIES | GET_PROVIDERS) as i64),
            )],
        )
        .expect("PackageInfoFlags aloca");
    let apm = vm
        .heap
        .alloc_instance(APM.to_string(), Vec::new())
        .expect("APM aloca");
    let pkg = Value::Obj(vm.heap.alloc_string(PKG.to_string()).unwrap());
    let pi = call_instance_intrinsic(
        &mut vm,
        APM,
        "getPackageInfo",
        "(Ljava/lang/String;I)Landroid/content/pm/PackageInfo;",
        apm,
        &[pkg, Value::Obj(pif)],
    )
    .expect("getPackageInfo resolve")
    .expect("PackageInfo");
    let pi = match pi {
        Value::Obj(r) => r,
        other => panic!("esperava Obj: {other:?}"),
    };
    assert!(arr_field(&vm, pi, "activities").is_some());
    assert!(arr_field(&vm, pi, "providers").is_some());
    assert!(arr_field(&vm, pi, "services").is_none());
}
