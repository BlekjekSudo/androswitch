//! Shared API edge cases in addition to the ordinary SDK/NDK 219-case APK.
use as_framework::App;
use as_platform::{host::HostPlatform, host_fs::HostFileSystem};
use rd_vm::{heap::ElemKind, ObjRef, Value, VmExit};
use std::{path::PathBuf, rc::Rc};
struct Test {
    app: App,
    root: PathBuf,
}
impl Test {
    fn new() -> Self {
        Self::configured(None)
    }
    fn configured(handled: Option<bool>) -> Self {
        let root = std::env::temp_dir().join(format!(
            "as-foundation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let fs = Rc::new(HostFileSystem::new(&root).unwrap());
        let pm = as_package::PackageManager::new(fs.clone());
        let apk = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../local-artifacts/conformance/AndroSwitchConformance.apk");
        let record = pm
            .install(
                std::fs::read(apk).expect("run fixtures/conformance/build.ps1"),
                true,
                None,
            )
            .unwrap();
        let mut apk = pm.load_apk(&record.package, None).unwrap().1;
        if handled == Some(true) {
            for component in &mut apk.manifest.application.components {
                if component.is_launcher {
                    component.config_changes = Some("orientation|screenSize".into());
                }
            }
        }
        let mut app = App::new(apk, fs, Rc::new(HostPlatform::default()), 640, 360, None).unwrap();
        if handled.is_some() {
            app.android.bind_graphics_caps(caps(0)).unwrap();
            app.android.bind_egl_backend(Rc::new(Display));
        }
        app.launch().unwrap();
        Self { app, root }
    }
    fn make(&mut self, class: &str) -> ObjRef {
        self.app
            .vm
            .heap
            .alloc_instance(class.into(), vec![])
            .unwrap()
    }
    fn text(&mut self, text: &str) -> Value {
        Value::Obj(self.app.vm.heap.alloc_string(text.into()).unwrap())
    }
    fn call(&mut self, r: ObjRef, name: &str, sig: &str, args: &[Value]) -> Value {
        self.app.vm.invoke_instance(r, name, sig, args).unwrap()
    }
    fn static_call(&mut self, class: &str, name: &str, sig: &str, args: &[Value]) -> Value {
        self.app.vm.invoke_static(class, name, sig, args).unwrap()
    }
    fn keep(&mut self, key: &str, r: ObjRef) {
        self.app
            .vm
            .statics
            .insert(("Ltests/Roots;".into(), key.into()), Value::Obj(r));
    }
    fn compile(&mut self, db: ObjRef, sql: &str) -> ObjRef {
        let sql = self.text(sql);
        self.call(
            db,
            "compileStatement",
            "(Ljava/lang/String;)Landroid/database/sqlite/SQLiteStatement;",
            &[sql],
        )
        .as_ref()
        .unwrap()
        .unwrap()
    }
}
impl Drop for Test {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn exception(result: Result<Value, VmExit>, class: &str) {
    assert!(
        matches!(result,Err(VmExit::Exception(ref e)) if e.class==class),
        "{result:?}"
    );
}
#[test]
fn arraycopy_overlaps_checks_components_and_preserves_partial_reference_copy() {
    let mut t = Test::new();
    let a = t
        .app
        .vm
        .heap
        .alloc_array_zeroed(ElemKind::Int, 4, "I")
        .unwrap();
    t.app
        .vm
        .heap
        .array_elems_mut(a)
        .unwrap()
        .clone_from_slice(&[Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(4)]);
    let sig = "(Ljava/lang/Object;ILjava/lang/Object;II)V";
    t.static_call(
        "Ljava/lang/System;",
        "arraycopy",
        sig,
        &[
            Value::Obj(a),
            Value::Int(0),
            Value::Obj(a),
            Value::Int(1),
            Value::Int(3),
        ],
    );
    assert_eq!(
        t.app.vm.heap.as_array_elems(a).unwrap().0,
        &[Value::Int(1), Value::Int(1), Value::Int(2), Value::Int(3)]
    );
    exception(
        t.app.vm.invoke_static(
            "Ljava/lang/System;",
            "arraycopy",
            sig,
            &[
                Value::Obj(a),
                Value::Int(0),
                Value::Obj(a),
                Value::Int(0),
                Value::Int(-1),
            ],
        ),
        "Ljava/lang/IndexOutOfBoundsException;",
    );
    let objects = t
        .app
        .vm
        .heap
        .alloc_array_zeroed(ElemKind::Obj, 2, "Ljava/lang/Object;")
        .unwrap();
    let strings = t
        .app
        .vm
        .heap
        .alloc_array_zeroed(ElemKind::Obj, 2, "Ljava/lang/String;")
        .unwrap();
    let string = t.text("first");
    let other = t.make("Ljava/lang/Object;");
    t.app
        .vm
        .heap
        .array_elems_mut(objects)
        .unwrap()
        .clone_from_slice(&[string.clone(), Value::Obj(other)]);
    exception(
        t.app.vm.invoke_static(
            "Ljava/lang/System;",
            "arraycopy",
            sig,
            &[
                Value::Obj(objects),
                Value::Int(0),
                Value::Obj(strings),
                Value::Int(0),
                Value::Int(2),
            ],
        ),
        "Ljava/lang/ArrayStoreException;",
    );
    assert_eq!(
        t.app.vm.heap.as_array_elems(strings).unwrap().0,
        &[string, Value::Null]
    );
}
#[test]
fn array_backed_list_aliases_survive_gc_and_reject_size_changes() {
    let mut t = Test::new();
    let a = t
        .app
        .vm
        .heap
        .alloc_array_zeroed(ElemKind::Obj, 2, "Ljava/lang/String;")
        .unwrap();
    let first = t.text("first");
    let second = t.text("second");
    t.app
        .vm
        .heap
        .array_elems_mut(a)
        .unwrap()
        .clone_from_slice(&[first, second.clone()]);
    let list = t
        .static_call(
            "Ljava/util/Arrays;",
            "asList",
            "([Ljava/lang/Object;)Ljava/util/List;",
            &[Value::Obj(a)],
        )
        .as_ref()
        .unwrap()
        .unwrap();
    t.keep("list", list);
    t.app.vm.gc();
    assert_eq!(
        t.call(list, "get", "(I)Ljava/lang/Object;", &[Value::Int(1)]),
        second
    );
    let changed = t.text("changed");
    t.call(
        list,
        "set",
        "(ILjava/lang/Object;)Ljava/lang/Object;",
        &[Value::Int(0), changed.clone()],
    );
    assert_eq!(t.app.vm.heap.as_array_elems(a).unwrap().0[0], changed);
    t.app.vm.heap.array_elems_mut(a).unwrap()[1] = Value::Null;
    assert_eq!(
        t.call(list, "get", "(I)Ljava/lang/Object;", &[Value::Int(1)]),
        Value::Null
    );
    exception(
        t.app
            .vm
            .invoke_instance(list, "add", "(Ljava/lang/Object;)Z", &[Value::Null]),
        "Ljava/lang/UnsupportedOperationException;",
    );
    let other = t.make("Ljava/lang/Object;");
    exception(
        t.app.vm.invoke_instance(
            list,
            "set",
            "(ILjava/lang/Object;)Ljava/lang/Object;",
            &[Value::Int(0), Value::Obj(other)],
        ),
        "Ljava/lang/ArrayStoreException;",
    );
    let iterator = t
        .call(list, "iterator", "()Ljava/util/Iterator;", &[])
        .as_ref()
        .unwrap()
        .unwrap();
    assert_eq!(
        t.call(iterator, "next", "()Ljava/lang/Object;", &[]),
        changed
    );
    assert_eq!(
        t.call(iterator, "next", "()Ljava/lang/Object;", &[]),
        Value::Null
    );
    exception(
        t.app
            .vm
            .invoke_instance(iterator, "next", "()Ljava/lang/Object;", &[]),
        "Ljava/util/NoSuchElementException;",
    );
}
#[test]
fn random_access_has_big_endian_shared_bytes_eof_and_readonly_errors() {
    let mut t = Test::new();
    let r = t.make("Ljava/io/RandomAccessFile;");
    let path = t.text("random-access.bin");
    let mode = t.text("rw");
    t.call(
        r,
        "<init>",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[path.clone(), mode],
    );
    t.keep("raf", r);
    t.call(r, "writeInt", "(I)V", &[Value::Int(0x01020304)]);
    assert_eq!(t.call(r, "length", "()J", &[]), Value::Long(4));
    assert_eq!(
        std::fs::read(
            t.root
                .join("userdata/org.androswitch.conformance/files/random-access.bin")
        )
        .unwrap(),
        [1, 2, 3, 4]
    );
    t.call(r, "seek", "(J)V", &[Value::Long(0)]);
    assert_eq!(t.call(r, "readInt", "()I", &[]), Value::Int(0x01020304));
    assert_eq!(t.call(r, "read", "()I", &[]), Value::Int(-1));
    t.call(r, "seek", "(J)V", &[Value::Long(3)]);
    exception(
        t.app.vm.invoke_instance(r, "readInt", "()I", &[]),
        "Ljava/io/EOFException;",
    );
    assert_eq!(t.call(r, "getFilePointer", "()J", &[]), Value::Long(4));
    t.call(r, "close", "()V", &[]);
    exception(
        t.app.vm.invoke_instance(r, "read", "()I", &[]),
        "Ljava/io/IOException;",
    );
    let ro = t.make("Ljava/io/RandomAccessFile;");
    let mode = t.text("r");
    t.call(
        ro,
        "<init>",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[path, mode],
    );
    exception(
        t.app
            .vm
            .invoke_instance(ro, "writeInt", "(I)V", &[Value::Int(4)]),
        "Ljava/io/IOException;",
    );
}
#[test]
fn compiled_sqlite_statements_bind_without_interpolation_keep_database_live_and_report_actual_changes(
) {
    let mut t = Test::new();
    let activity = t.app.android.activity_object().unwrap();
    let filename = t.text("statements.db");
    let db=t.call(activity,"openOrCreateDatabase","(Ljava/lang/String;ILandroid/database/sqlite/SQLiteDatabase$CursorFactory;)Landroid/database/sqlite/SQLiteDatabase;",&[filename,Value::Int(0),Value::Null]).as_ref().unwrap().unwrap();
    let sql = t.text("CREATE TABLE notes(id INTEGER PRIMARY KEY, value TEXT UNIQUE)");
    t.call(db, "execSQL", "(Ljava/lang/String;)V", &[sql]);
    let insert = t.compile(db, "INSERT OR IGNORE INTO notes(value) VALUES (?1)");
    t.keep("insert", insert);
    t.app.vm.gc(); // The live statement must keep its otherwise unrooted database alive.
    let value = t.text("quote'value ? :name 雪");
    t.call(
        insert,
        "bindString",
        "(ILjava/lang/String;)V",
        &[Value::Int(1), value],
    );
    assert_eq!(t.call(insert, "executeInsert", "()J", &[]), Value::Long(1));
    assert_eq!(t.call(insert, "executeInsert", "()J", &[]), Value::Long(-1));
    exception(
        t.app.vm.invoke_instance(
            insert,
            "bindLong",
            "(IJ)V",
            &[Value::Int(2), Value::Long(1)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    let count = t.compile(db, "SELECT count(*) FROM notes");
    assert_eq!(
        t.call(count, "simpleQueryForLong", "()J", &[]),
        Value::Long(1)
    );
    let number = t.compile(db, "SELECT '  -12tail'");
    assert_eq!(
        t.call(number, "simpleQueryForLong", "()J", &[]),
        Value::Long(-12)
    );
    let empty = t.compile(db, "SELECT id FROM notes WHERE 0");
    exception(
        t.app
            .vm
            .invoke_instance(empty, "simpleQueryForLong", "()J", &[]),
        "Landroid/database/sqlite/SQLiteDoneException;",
    );
    t.call(insert, "clearBindings", "()V", &[]);
    assert_eq!(t.call(insert, "executeInsert", "()J", &[]), Value::Long(2));
    t.call(insert, "close", "()V", &[]);
    exception(
        t.app
            .vm
            .invoke_instance(insert, "executeInsert", "()J", &[]),
        "Ljava/lang/IllegalStateException;",
    );
    t.call(db, "close", "()V", &[]);
    exception(
        t.app
            .vm
            .invoke_instance(count, "simpleQueryForLong", "()J", &[]),
        "Ljava/lang/IllegalStateException;",
    );
}

fn caps(rotation: i32) -> as_framework::GraphicsCaps {
    as_framework::GraphicsCaps {
        width: if rotation % 2 == 0 { 1280 } else { 720 },
        height: if rotation % 2 == 0 { 720 } else { 1280 },
        gles_version: 0x20000,
        density: 2.,
        density_dpi: 320,
        rotation,
    }
}
struct Display;
impl as_framework::EglConfigBackend for Display {
    fn set_display_rotation(&self, r: i32) -> Result<as_framework::GraphicsCaps, VmExit> {
        Ok(caps(r))
    }
    fn choose_config(
        &self,
        _: u64,
        _: &[i32],
        _: usize,
    ) -> Result<as_framework::EglConfigs, VmExit> {
        Err(rd_vm::err::vm_error("TEST_NO_GPU", "display-only test backend").into())
    }
    fn get_config_attrib(&self, _: u64, _: u64, _: i32) -> Result<i32, VmExit> {
        Err(rd_vm::err::vm_error("TEST_NO_GPU", "display-only test backend").into())
    }
}
#[test]
fn software_orientation_recreates_activity_preserves_intent_and_emits_save_restore_lifecycle() {
    let mut t = Test::configured(Some(false));
    let before = t.app.android.activity_object().unwrap();
    let intent = t.call(before, "getIntent", "()Landroid/content/Intent;", &[]);
    t.call(before, "setRequestedOrientation", "(I)V", &[Value::Int(1)]);
    assert_eq!(t.app.android.graphics_caps().unwrap().rotation, 0); // Asynchronous request.
    assert_eq!(
        t.call(before, "getRequestedOrientation", "()I", &[]),
        Value::Int(1)
    );
    t.app.draw().unwrap();
    let after = t.app.android.activity_object().unwrap();
    assert_ne!(before, after);
    assert_eq!((t.app.frame.width, t.app.frame.height), (360, 640));
    assert_eq!(
        t.call(after, "getIntent", "()Landroid/content/Intent;", &[]),
        intent
    );
    let events: Vec<_> = t
        .app
        .android
        .lifecycle()
        .into_iter()
        .map(|e| e.event)
        .collect();
    assert!(
        events.windows(8).any(|e| e
            == [
                "onPause",
                "onSaveInstanceState",
                "onStop",
                "onDestroy",
                "onCreate",
                "onStart",
                "onRestoreInstanceState",
                "onResume"
            ]),
        "{events:?}"
    );
    t.app.vm.gc();
    assert_eq!(
        t.call(after, "getRequestedOrientation", "()I", &[]),
        Value::Int(1)
    );
}
#[test]
fn handled_configuration_keeps_activity_and_cancelled_requests_do_not_rotate_display() {
    let mut t = Test::configured(Some(true));
    let activity = t.app.android.activity_object().unwrap();
    t.call(
        activity,
        "setRequestedOrientation",
        "(I)V",
        &[Value::Int(1)],
    );
    t.call(
        activity,
        "setRequestedOrientation",
        "(I)V",
        &[Value::Int(-1)],
    );
    t.app.draw().unwrap();
    assert_eq!(t.app.android.graphics_caps().unwrap().rotation, 0);
    t.call(
        activity,
        "setRequestedOrientation",
        "(I)V",
        &[Value::Int(1)],
    );
    t.app.draw().unwrap();
    assert_eq!(t.app.android.activity_object(), Some(activity));
    assert_eq!(t.app.android.graphics_caps().unwrap().rotation, 1);
    assert!(t
        .app
        .android
        .lifecycle()
        .iter()
        .any(|e| e.event == "onConfigurationChanged"));
}
