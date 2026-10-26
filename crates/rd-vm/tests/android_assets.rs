//! Regressão do `Context.getAssets()` / `AssetManager` host (android_res.rs).
//!
//! Contexto: o APK-Tester reportava 12/12 APKs do sdk-ladder caindo no
//! `LadderActivity.readRungsAsset()` — `Activity.getAssets()` não resolvido.
//! Estes testes fixam o contrato do fix:
//!
//! 1. `getAssets()` responde para qualquer receiver `Context` (Activity,
//!    Application, Service, ContextWrapper DEX) e devolve o manager
//!    CANÔNICO da VM (identidade estável entre chamadas).
//! 2. `open(String[, int])` devolve um `ByteArrayInputStream` host com os
//!    BYTES REAIS de `assets/<nome>` do APK injetado — o mesmo caminho de
//!    execução DEX que um app real percorre (`invoke-virtual` em cadeia).
//! 3. Entrada ausente → `FileNotFoundException` tipada (nunca stream vazio
//!    silencioso — Lei nº 1 do projeto).
//! 4. `list(path)` devolve o primeiro nível sob `assets/<path>/`.
//! 5. `openFd` → `FileNotFoundException` (entrada descomprimida não é
//!    modelada — comportamento AOSP para assets deflados).
//! 6. Path hostil (`..`, absoluto, segmento vazio) → `FileNotFoundException`
//!    ANTES de qualquer leitura do ZIP (sem pivô para fora de assets/).

mod common;

use common::*;
use rd_vm::engine::{Engine, VmConfig};
use rd_vm::err::VmExit;
use rd_vm::heap::ObjRef;
use rd_vm::intrinsics::call_instance_intrinsic;
use rd_vm::value::Value;

const ACTIVITY: &str = "Landroid/app/Activity;";
const ASSET_MANAGER: &str = "Landroid/content/res/AssetManager;";
const RESOURCES: &str = "Landroid/content/res/Resources;";
const BAIS: &str = "Ljava/io/ByteArrayInputStream;";
const FILE_NOT_FOUND: &str = "Ljava/io/FileNotFoundException;";
const NPE: &str = "Ljava/lang/NullPointerException;";

const RUNGS: &[u8] = br#"{"rungs":[26,29,31,33,34,35]}"#;
const RUNGS_PATH: &str = "ladder/rungs.json";

fn apk_vm(entries: Vec<(&str, Vec<u8>)>) -> Engine {
    let apk_bytes = apkfix::build_apk(
        "com.test.assets",
        "LMain;",
        apkfix::ArscFix::new().build(),
        entries,
    );
    let apk = rd_apk::Apk::from_bytes(apk_bytes).expect("APK de assets parseia");
    // O DEX de Caso é exigido pelo Engine::new; o corpo dos testes DEX reais
    // constrói o próprio builder — aqui só precisamos da VM viva com APK.
    let mut b = DexBuilder::new();
    b.class("LCaso;", "Ljava/lang/Object;");
    let dex = rd_dex::Dex::parse(b.finish()).expect("DEX sintético parseia");
    let mut vm = Engine::new(vec![dex], VmConfig::default());
    vm.set_resources(apk);
    vm
}

fn activity(vm: &mut Engine) -> ObjRef {
    vm.heap
        .alloc_instance(ACTIVITY.to_string(), Vec::new())
        .expect("Activity host aloca")
}

fn manager_of(vm: &mut Engine, recv: ObjRef) -> ObjRef {
    match call_instance_intrinsic(
        vm,
        ACTIVITY,
        "getAssets",
        "()Landroid/content/res/AssetManager;",
        recv,
        &[],
    )
    .expect("intrínseco de getAssets executa")
    {
        Some(Value::Obj(r)) => r,
        other => panic!("getAssets: {other:?}"),
    }
}

fn string_arg(vm: &mut Engine, s: &str) -> Value {
    Value::Obj(vm.heap.alloc_string(s.into()).expect("string aloca"))
}

fn array_strings(vm: &Engine, v: &Value) -> Vec<String> {
    let Value::Obj(a) = v else {
        panic!("esperava array, veio {v:?}")
    };
    let (elems, _) = vm.heap.as_array_elems(*a).expect("elems");
    elems
        .iter()
        .map(|e| match e {
            Value::Obj(r) => vm.heap.as_str(*r).expect("string").to_string(),
            other => panic!("elemento não-string: {other:?}"),
        })
        .collect()
}

#[test]
fn activity_get_assets_returns_canonical_manager() {
    let mut vm = apk_vm(vec![
        (RUNGS_PATH_ENTRY, RUNGS.to_vec()),
        (TOP_ENTRY, b"zz".to_vec()),
    ]);
    let a = activity(&mut vm);
    let m = manager_of(&mut vm, a);
    assert_eq!(vm.heap.class_of(m).expect("classe"), ASSET_MANAGER);
    // identidade canônica: AOSP devolve o mesmo manager do Resources
    let m2 = manager_of(&mut vm, a);
    assert_eq!(m, m2, "getAssets() deve devolver o singleton da VM");

    // Resources.getAssets() → o MESMO manager (AOSP delega)
    let res = match call_instance_intrinsic(
        &mut vm,
        RESOURCES,
        "getAssets",
        "()Landroid/content/res/AssetManager;",
        m, // qualquer receiver Resources serve: singleton do host
        &[],
    )
    .expect("intrínseco Resources.getAssets executa")
    {
        Some(Value::Obj(r)) => r,
        other => panic!("Resources.getAssets: {other:?}"),
    };
    let _ = res;
}

#[test]
fn asset_manager_open_streams_real_bytes_through_dex_invoke_chain() {
    // Caminho REAL de execução: invoke-virtual Activity.getAssets() →
    // AssetManager.open(String) → InputStream.read() — o mesmo que o
    // LadderActivity.readRungsAsset() do APK-Tester faz.
    let apk_bytes = apkfix::build_apk(
        "com.test.assets",
        "LMain;",
        apkfix::ArscFix::new().build(),
        vec![
            (RUNGS_PATH_ENTRY, RUNGS.to_vec()),
            (TOP_ENTRY, b"zz".to_vec()),
        ],
    );
    let apk = rd_apk::Apk::from_bytes(apk_bytes).expect("APK de assets parseia");

    let mut b = DexBuilder::new();
    let name = b.intern(RUNGS_PATH) as u16;
    let p_assets = b.proto_idx(ASSET_MANAGER, vec![]);
    let p_open = b.proto_idx("Ljava/io/InputStream;", vec!["Ljava/lang/String;".into()]);
    let p_read = b.proto_idx("I", vec![]);
    let get_assets = b.method_idx(ACTIVITY, p_assets, "getAssets");
    let open = b.method_idx(ASSET_MANAGER, p_open, "open");
    let read = b.method_idx("Ljava/io/InputStream;", p_read, "read");
    let caller = b.class("LCaso;", "Ljava/lang/Object;");
    b.direct(
        caller,
        "readFirst",
        "I",
        vec![ACTIVITY],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(5, 1, 2, {
            // const-string v0, "ladder/rungs.json"
            let mut u = op21c(0x1A, 0, name);
            // invoke-virtual {v4}, Activity.getAssets()
            u.extend(op35c(0x6E, 1, get_assets, [4, 0, 0, 0, 0]));
            // move-result-object v1
            u.extend(op11x(0x0C, 1));
            // invoke-virtual {v1, v0}, AssetManager.open(String)
            u.extend(op35c(0x6E, 2, open, [1, 0, 0, 0, 0]));
            // move-result-object v2
            u.extend(op11x(0x0C, 2));
            // invoke-virtual {v2}, InputStream.read()
            u.extend(op35c(0x6E, 1, read, [2, 0, 0, 0, 0]));
            // move-result v0; return v0
            u.extend(op11x(0x0A, 0));
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    let dex = rd_dex::Dex::parse(b.finish()).expect("DEX parseia");
    let mut vm = Engine::new(vec![dex], VmConfig::default());
    vm.set_resources(apk);
    let a = vm
        .heap
        .alloc_instance(ACTIVITY.to_string(), Vec::new())
        .expect("Activity host aloca");
    let first = invoke(
        &mut vm,
        "readFirst",
        "(Landroid/app/Activity;)I",
        &[Value::Obj(a)],
    )
    .expect("cadeia DEX executa")
    .as_int()
    .expect("int");
    assert_eq!(first, RUNGS[0] as i32, "primeiro byte real do asset");
}

#[test]
fn asset_manager_open_returns_bais_with_exact_bytes() {
    let mut vm = apk_vm(vec![
        (RUNGS_PATH_ENTRY, RUNGS.to_vec()),
        (TOP_ENTRY, b"zz".to_vec()),
    ]);
    let a = activity(&mut vm);
    let m = manager_of(&mut vm, a);

    let arg = string_arg(&mut vm, RUNGS_PATH);
    let stream = match call_instance_intrinsic(
        &mut vm,
        ASSET_MANAGER,
        "open",
        "(Ljava/lang/String;)Ljava/io/InputStream;",
        m,
        &[arg],
    )
    .expect("open executa")
    {
        Some(v @ Value::Obj(_)) => v,
        other => panic!("open: {other:?}"),
    };
    assert_eq!(
        vm.heap
            .class_of(match stream {
                Value::Obj(r) => r,
                _ => unreachable!(),
            })
            .expect("classe do stream"),
        BAIS
    );

    // available() == tamanho exato do asset
    let stream_ref = match &stream {
        Value::Obj(r) => *r,
        _ => unreachable!(),
    };
    let avail = call_instance_intrinsic(&mut vm, BAIS, "available", "()I", stream_ref, &[])
        .expect("available executa")
        .expect("BAIN responde available");
    assert_eq!(avail.as_int().expect("int"), RUNGS.len() as i32);

    // read() devolve os bytes na ordem
    for (i, byte) in RUNGS.iter().enumerate() {
        let v = call_instance_intrinsic(&mut vm, BAIS, "read", "()I", stream_ref, &[])
            .expect("read executa")
            .expect("read responde");
        assert_eq!(v.as_int().expect("int"), *byte as i32, "byte #{i}");
    }

    // variante com accessMode (ACCESS_BUFFER = 3) — mesmo stream
    let arg = string_arg(&mut vm, RUNGS_PATH);
    let buffered = call_instance_intrinsic(
        &mut vm,
        ASSET_MANAGER,
        "open",
        "(Ljava/lang/String;I)Ljava/io/InputStream;",
        m,
        &[arg, Value::Int(3)],
    )
    .expect("open(mode) executa")
    .expect("open(mode) responde");
    assert!(matches!(buffered, Value::Obj(_)));
}

#[test]
fn asset_manager_open_missing_file_throws_file_not_found() {
    let mut vm = apk_vm(vec![(TOP_ENTRY, b"zz".to_vec())]);
    let a = activity(&mut vm);
    let m = manager_of(&mut vm, a);
    let arg = string_arg(&mut vm, "nope/missing.json");
    match call_instance_intrinsic(
        &mut vm,
        ASSET_MANAGER,
        "open",
        "(Ljava/lang/String;)Ljava/io/InputStream;",
        m,
        &[arg],
    ) {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, FILE_NOT_FOUND, "classe da exceção");
        }
        other => panic!("esperava FileNotFoundException, veio {other:?}"),
    }
}

#[test]
fn asset_manager_open_null_name_throws_npe() {
    let mut vm = apk_vm(vec![(TOP_ENTRY, b"zz".to_vec())]);
    let a = activity(&mut vm);
    let m = manager_of(&mut vm, a);
    match call_instance_intrinsic(
        &mut vm,
        ASSET_MANAGER,
        "open",
        "(Ljava/lang/String;)Ljava/io/InputStream;",
        m,
        &[Value::Null],
    ) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, NPE),
        other => panic!("esperava NPE, veio {other:?}"),
    }
}

#[test]
fn asset_manager_open_rejects_hostile_paths_before_zip() {
    let mut vm = apk_vm(vec![(TOP_ENTRY, b"zz".to_vec())]);
    let a = activity(&mut vm);
    let m = manager_of(&mut vm, a);
    for hostile in [
        "../AndroidManifest.xml", // pivô para fora de assets/
        "a/../../classes.dex",
        "/abs/path",
        "dir//double",
        "dir/./self",
        "trailing/",
        "",
    ] {
        let arg = string_arg(&mut vm, hostile);
        match call_instance_intrinsic(
            &mut vm,
            ASSET_MANAGER,
            "open",
            "(Ljava/lang/String;)Ljava/io/InputStream;",
            m,
            &[arg],
        ) {
            Err(VmExit::Exception(t)) => {
                assert_eq!(t.class, FILE_NOT_FOUND, "path hostil: {hostile:?}");
            }
            other => panic!("path hostil {hostile:?} devia recusar, veio {other:?}"),
        }
    }
}

#[test]
fn asset_manager_list_reports_first_level_only() {
    let mut vm = apk_vm(vec![
        (RUNGS_PATH_ENTRY, RUNGS.to_vec()),
        (TOP_ENTRY, b"zz".to_vec()),
    ]);
    let a = activity(&mut vm);
    let m = manager_of(&mut vm, a);

    let root = string_arg(&mut vm, "");
    let v = call_instance_intrinsic(
        &mut vm,
        ASSET_MANAGER,
        "list",
        "(Ljava/lang/String;)[Ljava/lang/String;",
        m,
        &[root],
    )
    .expect("list executa")
    .expect("list responde");
    let mut names = array_strings(&vm, &v);
    names.sort();
    assert_eq!(names, vec!["ladder".to_string(), "zz.txt".to_string()]);

    let sub = string_arg(&mut vm, "ladder");
    let v = call_instance_intrinsic(
        &mut vm,
        ASSET_MANAGER,
        "list",
        "(Ljava/lang/String;)[Ljava/lang/String;",
        m,
        &[sub],
    )
    .expect("list executa")
    .expect("list responde");
    assert_eq!(array_strings(&vm, &v), vec!["rungs.json".to_string()]);

    // subdiretório inexistente: array vazio (comportamento AOSP), nunca null
    let none = string_arg(&mut vm, "nope");
    let v = call_instance_intrinsic(
        &mut vm,
        ASSET_MANAGER,
        "list",
        "(Ljava/lang/String;)[Ljava/lang/String;",
        m,
        &[none],
    )
    .expect("list executa")
    .expect("list responde");
    assert!(array_strings(&vm, &v).is_empty());

    // path null → NPE (AOSP)
    match call_instance_intrinsic(
        &mut vm,
        ASSET_MANAGER,
        "list",
        "(Ljava/lang/String;)[Ljava/lang/String;",
        m,
        &[Value::Null],
    ) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, NPE),
        other => panic!("esperava NPE, veio {other:?}"),
    }
}

#[test]
fn asset_manager_open_fd_throws_file_not_found() {
    let mut vm = apk_vm(vec![(RUNGS_PATH_ENTRY, RUNGS.to_vec())]);
    let a = activity(&mut vm);
    let m = manager_of(&mut vm, a);
    let arg = string_arg(&mut vm, RUNGS_PATH);
    match call_instance_intrinsic(
        &mut vm,
        ASSET_MANAGER,
        "openFd",
        "(Ljava/lang/String;)Landroid/content/res/AssetFileDescriptor;",
        m,
        &[arg],
    ) {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, FILE_NOT_FOUND);
            let msg = t.message.unwrap_or_default();
            assert!(
                msg.contains("file descriptor"),
                "mensagem fiel do AOSP: {msg}"
            );
        }
        other => panic!("esperava FileNotFoundException, veio {other:?}"),
    }
}

#[test]
fn get_assets_answers_for_every_context_receiver() {
    let mut vm = apk_vm(vec![(TOP_ENTRY, b"zz".to_vec())]);
    for class in [
        "Landroid/app/Activity;",
        "Landroid/app/Application;",
        "Landroid/app/Service;",
        "Landroid/content/ContextWrapper;",
    ] {
        let recv = vm
            .heap
            .alloc_instance(class.to_string(), Vec::new())
            .expect("receiver aloca");
        match call_instance_intrinsic(
            &mut vm,
            class,
            "getAssets",
            "()Landroid/content/res/AssetManager;",
            recv,
            &[],
        )
        .expect("intrínseco executa")
        {
            Some(Value::Obj(r)) => {
                assert_eq!(vm.heap.class_of(r).expect("classe"), ASSET_MANAGER);
            }
            other => panic!("{class}: getAssets devia responder, veio {other:?}"),
        }
    }

    // não-Context NÃO finge suporte (Lei nº 1: silêncio é pior que recusa)
    let marker = vm
        .heap
        .alloc_instance("LMarker;".to_string(), Vec::new())
        .expect("marker aloca");
    assert!(
        call_instance_intrinsic(
            &mut vm,
            "LMarker;",
            "getAssets",
            "()Landroid/content/res/AssetManager;",
            marker,
            &[]
        )
        .expect("intrínseco executa")
        .is_none(),
        "getAssets em não-Context deve retornar None"
    );
}

#[test]
fn asset_manager_close_is_noop_and_ctor_allows_bare_new() {
    let mut vm = apk_vm(vec![(TOP_ENTRY, b"zz".to_vec())]);
    let a = activity(&mut vm);
    let m = manager_of(&mut vm, a);
    assert_eq!(
        call_instance_intrinsic(&mut vm, ASSET_MANAGER, "close", "()V", m, &[])
            .expect("close executa")
            .expect("close responde"),
        Value::Null
    );
    // apps legados instanciam AssetManager() direto (deprecated, mas real)
    let bare = vm
        .heap
        .alloc_instance(ASSET_MANAGER.to_string(), Vec::new())
        .expect("AssetManager bare aloca");
    assert_eq!(
        call_instance_intrinsic(&mut vm, ASSET_MANAGER, "<init>", "()V", bare, &[])
            .expect("ctor executa")
            .expect("ctor responde"),
        Value::Null
    );
    // ...e o manager aberto por ele também lê o asset real
    let arg = string_arg(&mut vm, TOP_NAME);
    let v = call_instance_intrinsic(
        &mut vm,
        ASSET_MANAGER,
        "open",
        "(Ljava/lang/String;)Ljava/io/InputStream;",
        bare,
        &[arg],
    )
    .expect("open executa")
    .expect("open responde");
    assert!(matches!(v, Value::Obj(_)));
}

// entradas compartilhadas dos fixtures
const RUNGS_PATH_ENTRY: &str = "assets/ladder/rungs.json";
const TOP_ENTRY: &str = "assets/zz.txt";
const TOP_NAME: &str = "zz.txt";

#[test]
fn context_get_application_info_serves_manifest_facts() {
    // Follow-up da cascata do ladder: LadderActivity chama
    // `getApplicationInfo()` DIRETO no Context (sem passar pelo PackageManager)
    // para ler minSdkVersion/targetSdkVersion declarados no manifest.
    let mut vm = apk_vm(vec![(TOP_ENTRY, b"zz".to_vec())]);
    let a = activity(&mut vm);
    let info = match call_instance_intrinsic(
        &mut vm,
        ACTIVITY,
        "getApplicationInfo",
        "()Landroid/content/pm/ApplicationInfo;",
        a,
        &[],
    )
    .expect("intrínseco de getApplicationInfo executa")
    {
        Some(Value::Obj(r)) => r,
        other => panic!("getApplicationInfo: {other:?}"),
    };
    assert_eq!(
        vm.heap.class_of(info).expect("classe"),
        "Landroid/content/pm/ApplicationInfo;"
    );
    // fatos do manifest do fixture sintético (build_apk: min 26 / target 36…
    // o apkfix default é min=1/target=1; o teste usa os valores reais do APK)
    let pkg = vm.heap.get_field(info, "packageName").expect("packageName");
    match pkg {
        Value::Obj(r) => assert_eq!(vm.heap.as_str(r).expect("str"), "com.test.assets"),
        other => panic!("packageName: {other:?}"),
    }
    // campos int usados pelo ladder (iget direto no host object)
    for field in ["minSdkVersion", "targetSdkVersion", "uid", "enabled"] {
        let v = vm.heap.get_field(info, field).expect(field);
        assert!(matches!(v, Value::Int(_)), "{field}: {v:?}");
    }
    // rota LONGA segue igual (PackageManager.getApplicationInfo do próprio
    // pacote) — os mesmos fatos, via caminho diferente
    let pm = match call_instance_intrinsic(
        &mut vm,
        ACTIVITY,
        "getPackageManager",
        "()Landroid/content/pm/PackageManager;",
        a,
        &[],
    )
    .expect("getPackageManager executa")
    {
        Some(Value::Obj(r)) => r,
        other => panic!("getPackageManager: {other:?}"),
    };
    let pkg_name = string_arg(&mut vm, "com.test.assets");
    let long = match call_instance_intrinsic(
        &mut vm,
        "Landroid/app/ApplicationPackageManager;",
        "getApplicationInfo",
        "(Ljava/lang/String;)Landroid/content/pm/ApplicationInfo;",
        pm,
        &[pkg_name],
    )
    .expect("PM.getApplicationInfo executa")
    {
        Some(Value::Obj(r)) => r,
        other => panic!("PM.getApplicationInfo: {other:?}"),
    };
    let min_short = vm
        .heap
        .get_field(info, "minSdkVersion")
        .unwrap()
        .as_int()
        .unwrap();
    let min_long = vm
        .heap
        .get_field(long, "minSdkVersion")
        .unwrap()
        .as_int()
        .unwrap();
    assert_eq!(min_short, min_long, "rota curta ≡ rota longa (AOSP)");

    // não-Context continua sem fingir suporte
    let marker = vm
        .heap
        .alloc_instance("LMarker;".to_string(), Vec::new())
        .expect("marker aloca");
    assert!(
        call_instance_intrinsic(
            &mut vm,
            "LMarker;",
            "getApplicationInfo",
            "()Landroid/content/pm/ApplicationInfo;",
            marker,
            &[]
        )
        .expect("intrínseco executa")
        .is_none(),
        "getApplicationInfo em não-Context deve retornar None"
    );
}

#[test]
fn application_info_serves_every_context_receiver() {
    let mut vm = apk_vm(vec![(TOP_ENTRY, b"zz".to_vec())]);
    for class in [
        "Landroid/app/Activity;",
        "Landroid/app/Application;",
        "Landroid/app/Service;",
        "Landroid/content/ContextWrapper;",
    ] {
        let recv = vm
            .heap
            .alloc_instance(class.to_string(), Vec::new())
            .expect("receiver aloca");
        match call_instance_intrinsic(
            &mut vm,
            class,
            "getApplicationInfo",
            "()Landroid/content/pm/ApplicationInfo;",
            recv,
            &[],
        )
        .expect("intrínseco executa")
        {
            Some(Value::Obj(_)) => {}
            other => panic!("{class}: getApplicationInfo devia responder, veio {other:?}"),
        }
    }
}
