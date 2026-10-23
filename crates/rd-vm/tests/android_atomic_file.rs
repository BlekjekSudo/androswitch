//! AOSP `AtomicFile` commit/rollback protocol over the VM filesystem.

mod common;

use common::*;
use rd_vm::heap::ElemKind;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef};

const FILE: &str = "Ljava/io/File;";
const ATOMIC_FILE: &str = "Landroid/util/AtomicFile;";
const PATH: &str = "/data/user/0/app/files/atomic-round9/state.txt";

fn instance(engine: &mut Engine, class: &str) -> ObjRef {
    engine
        .heap
        .alloc_instance(class.to_string(), Vec::new())
        .unwrap()
}

fn object(value: Value) -> ObjRef {
    let Value::Obj(reference) = value else {
        panic!("expected object, got {value:?}");
    };
    reference
}

fn byte_array(engine: &mut Engine, bytes: &[u8]) -> Value {
    let array = engine
        .heap
        .alloc_array_zeroed(ElemKind::Int, bytes.len(), "[B")
        .unwrap();
    let elems = engine.heap.array_elems_mut(array).unwrap();
    for (slot, byte) in elems.iter_mut().zip(bytes) {
        *slot = Value::Int(i32::from(*byte as i8));
    }
    Value::Obj(array)
}

fn bytes_of(engine: &Engine, value: &Value) -> Vec<u8> {
    let Value::Obj(array) = value else {
        panic!("expected byte array, got {value:?}");
    };
    engine
        .heap
        .as_array_elems(*array)
        .unwrap()
        .0
        .iter()
        .map(|value| value.as_int().unwrap_or(0) as u8)
        .collect()
}

fn write(engine: &mut Engine, atomic: ObjRef, bytes: &[u8]) -> ObjRef {
    let stream = object(
        engine
            .invoke_instance(atomic, "startWrite", "()Ljava/io/FileOutputStream;", &[])
            .unwrap(),
    );
    let data = byte_array(engine, bytes);
    engine
        .invoke_instance(stream, "write", "([B)V", &[data])
        .unwrap();
    stream
}

fn finish(engine: &mut Engine, atomic: ObjRef, stream: ObjRef) {
    engine
        .invoke_instance(
            atomic,
            "finishWrite",
            "(Ljava/io/FileOutputStream;)V",
            &[Value::Obj(stream)],
        )
        .unwrap();
}

fn fail(engine: &mut Engine, atomic: ObjRef, stream: ObjRef) {
    engine
        .invoke_instance(
            atomic,
            "failWrite",
            "(Ljava/io/FileOutputStream;)V",
            &[Value::Obj(stream)],
        )
        .unwrap();
}

fn read_fully(engine: &mut Engine, atomic: ObjRef) -> Vec<u8> {
    let bytes = engine
        .invoke_instance(atomic, "readFully", "()[B", &[])
        .unwrap();
    bytes_of(engine, &bytes)
}

#[test]
fn atomic_file_finish_fail_and_interrupted_writes_follow_aosp_backup_protocol() {
    let mut engine = engine_of(&DexBuilder::new());
    let file = instance(&mut engine, FILE);
    let path = Value::Obj(engine.heap.alloc_string(PATH.to_string()).unwrap());
    engine
        .invoke_instance(file, "<init>", "(Ljava/lang/String;)V", &[path])
        .unwrap();

    let atomic = instance(&mut engine, ATOMIC_FILE);
    engine
        .invoke_instance(atomic, "<init>", "(Ljava/io/File;)V", &[Value::Obj(file)])
        .unwrap();
    assert_eq!(
        engine
            .invoke_instance(atomic, "getBaseFile", "()Ljava/io/File;", &[])
            .unwrap(),
        Value::Obj(file)
    );

    // startWrite creates the missing parent directory on retry, as AOSP does.
    let stream = write(&mut engine, atomic, b"old");
    finish(&mut engine, atomic, stream);
    assert_eq!(read_fully(&mut engine, atomic), b"old");

    // failWrite discards the replacement and restores the committed backup.
    let stream = write(&mut engine, atomic, b"partial replacement");
    fail(&mut engine, atomic, stream);
    assert_eq!(read_fully(&mut engine, atomic), b"old");

    // If a second startWrite sees the first write's backup, it retains that
    // older committed backup and discards the first partial base file.
    let _interrupted = write(&mut engine, atomic, b"first partial");
    let stream = write(&mut engine, atomic, b"second partial");
    fail(&mut engine, atomic, stream);
    assert_eq!(read_fully(&mut engine, atomic), b"old");

    // An interrupted write is also rolled back by openRead/readFully.
    let _interrupted = write(&mut engine, atomic, b"never committed");
    assert_eq!(read_fully(&mut engine, atomic), b"old");

    let stream = write(&mut engine, atomic, b"new value");
    finish(&mut engine, atomic, stream);
    assert_eq!(read_fully(&mut engine, atomic), b"new value");

    engine
        .invoke_instance(atomic, "delete", "()V", &[])
        .unwrap();
    let error = engine
        .invoke_instance(atomic, "openRead", "()Ljava/io/FileInputStream;", &[])
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("FileNotFoundException"),
        "unexpected error: {error:?}"
    );
}
