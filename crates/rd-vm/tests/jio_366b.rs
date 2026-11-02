//! Regression tests for issue #366 item 1 (`crates/rd-vm/src/jio_chars.rs`):
//! writer buffers live on the host, so their growth must be charged against
//! `heap_budget`. Before, `StringWriter.write` grew the host buffer without any
//! limit, and every write cloned the whole buffer (O(N^2)).

use rd_vm::err::VmExit;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef, VmConfig};

const SW: &str = "Ljava/io/StringWriter;";

fn engine_with_budget(heap_budget: usize) -> Engine {
    Engine::new(
        Vec::new(),
        VmConfig {
            heap_budget,
            ..VmConfig::default()
        },
    )
}

fn alloc_obj(vm: &mut Engine, class: &str) -> ObjRef {
    vm.heap
        .alloc_instance(class.to_string(), Vec::new())
        .expect("alloc_instance")
}

fn alloc_jstr(vm: &mut Engine, s: &str) -> Value {
    Value::Obj(vm.heap.alloc_string(s.to_string()).expect("alloc_string"))
}

#[test]
fn issue_366_string_writer_growth_is_bounded_by_heap_budget() {
    let mut e = engine_with_budget(64 * 1024);
    let sw = alloc_obj(&mut e, SW);
    e.invoke_instance(sw, "<init>", "()V", &[])
        .expect("StringWriter()");
    let chunk = "x".repeat(1000);
    let mut failure = None;
    for _ in 0..500 {
        let s = alloc_jstr(&mut e, &chunk);
        match e.invoke_instance(sw, "write", "(Ljava/lang/String;)V", &[s]) {
            Ok(_) => {}
            Err(err) => {
                failure = Some(err);
                break;
            }
        }
    }
    match failure {
        Some(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/OutOfMemoryError;", "wrong exception")
        }
        other => panic!("writes past the 64 KiB heap budget must throw OOM, got {other:?}"),
    }
}

#[test]
fn issue_366_string_writer_small_writes_keep_their_content() {
    let mut e = engine_with_budget(1024 * 1024);
    let sw = alloc_obj(&mut e, SW);
    e.invoke_instance(sw, "<init>", "()V", &[])
        .expect("StringWriter()");
    for part in ["ola", " ", "mundo"] {
        let s = alloc_jstr(&mut e, part);
        e.invoke_instance(sw, "write", "(Ljava/lang/String;)V", &[s])
            .expect("write");
    }
    let out = e
        .invoke_instance(sw, "toString", "()Ljava/lang/String;", &[])
        .expect("toString");
    let Value::Obj(r) = out else {
        panic!("expected String, got {out:?}")
    };
    assert_eq!(e.heap.as_str(r).expect("as_str"), "ola mundo");
}

const BAOS: &str = "Ljava/io/ByteArrayOutputStream;";

fn alloc_bytes(vm: &mut Engine, data: &[u8]) -> ObjRef {
    let arr = vm
        .heap
        .alloc_array_zeroed(rd_vm::heap::ElemKind::Int, data.len(), "[B")
        .expect("alloc [B");
    let elems = vm.heap.array_elems_mut(arr).expect("elems");
    for (slot, b) in elems.iter_mut().zip(data) {
        *slot = Value::Int(i32::from(*b as i8));
    }
    arr
}

#[test]
fn issue_366_byte_array_output_growth_is_bounded_by_heap_budget() {
    let mut e = engine_with_budget(64 * 1024);
    let baos = alloc_obj(&mut e, BAOS);
    e.invoke_instance(baos, "<init>", "()V", &[])
        .expect("BAOS()");
    let chunk = alloc_bytes(&mut e, &[7u8; 1000]);
    let mut failure = None;
    for _ in 0..500 {
        match e.invoke_instance(baos, "write", "([B)V", &[Value::Obj(chunk)]) {
            Ok(_) => {}
            Err(err) => {
                failure = Some(err);
                break;
            }
        }
    }
    match failure {
        Some(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/OutOfMemoryError;", "wrong exception")
        }
        other => panic!("writes past the 64 KiB heap budget must throw OOM, got {other:?}"),
    }
}

#[test]
fn issue_366_byte_array_output_keeps_content_and_size() {
    let mut e = engine_with_budget(1024 * 1024);
    let baos = alloc_obj(&mut e, BAOS);
    e.invoke_instance(baos, "<init>", "()V", &[])
        .expect("BAOS()");
    let a = alloc_bytes(&mut e, b"abc");
    e.invoke_instance(baos, "write", "([B)V", &[Value::Obj(a)])
        .expect("write([B)");
    e.invoke_instance(baos, "write", "(I)V", &[Value::Int(b'!' as i32)])
        .expect("write(I)");
    let size = e.invoke_instance(baos, "size", "()I", &[]).expect("size");
    assert_eq!(size, Value::Int(4));
    let out = e
        .invoke_instance(baos, "toString", "()Ljava/lang/String;", &[])
        .expect("toString");
    let Value::Obj(r) = out else {
        panic!("expected String, got {out:?}")
    };
    assert_eq!(e.heap.as_str(r).expect("as_str"), "abc!");
}
