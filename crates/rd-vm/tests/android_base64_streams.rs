//! Host-level proof for Android's Base64 filter streams over real
//! `ByteArrayOutputStream` / `ByteArrayInputStream` instances.

mod common;

use common::*;
use rd_vm::heap::ElemKind;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef};

const BAOS: &str = "Ljava/io/ByteArrayOutputStream;";
const BAIS: &str = "Ljava/io/ByteArrayInputStream;";
const B64_OUT: &str = "Landroid/util/Base64OutputStream;";
const B64_IN: &str = "Landroid/util/Base64InputStream;";

fn instance(engine: &mut Engine, class: &str) -> ObjRef {
    engine
        .heap
        .alloc_instance(class.to_string(), Vec::new())
        .unwrap()
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

fn byte_stream_contents(engine: &mut Engine, stream: ObjRef) -> Vec<u8> {
    let bytes = engine
        .invoke_instance(stream, "toByteArray", "()[B", &[])
        .unwrap();
    bytes_of(engine, &bytes)
}

#[test]
fn base64_output_stream_incrementally_encodes_and_finishes_on_close() {
    let mut engine = engine_of(&DexBuilder::new());
    let sink = instance(&mut engine, BAOS);
    engine.invoke_instance(sink, "<init>", "()V", &[]).unwrap();
    let encoder = instance(&mut engine, B64_OUT);
    engine
        .invoke_instance(
            encoder,
            "<init>",
            "(Ljava/io/OutputStream;I)V",
            &[Value::Obj(sink), Value::Int(0)],
        )
        .unwrap();

    for byte in b"hello" {
        engine
            .invoke_instance(encoder, "write", "(I)V", &[Value::Int(i32::from(*byte))])
            .unwrap();
    }
    engine
        .invoke_instance(encoder, "flush", "()V", &[])
        .unwrap();
    // Only the complete "hel" triplet is emitted before close.
    assert_eq!(byte_stream_contents(&mut engine, sink), b"aGVs");

    engine
        .invoke_instance(encoder, "close", "()V", &[])
        .unwrap();
    assert_eq!(byte_stream_contents(&mut engine, sink), b"aGVsbG8=\n");
}

#[test]
fn base64_input_stream_decodes_wrapped_bytes_with_offsets() {
    let mut engine = engine_of(&DexBuilder::new());
    let encoded = byte_array(&mut engine, b"aGVs\nbG8=");
    let source = instance(&mut engine, BAIS);
    engine
        .invoke_instance(source, "<init>", "([B)V", &[encoded])
        .unwrap();
    let decoder = instance(&mut engine, B64_IN);
    engine
        .invoke_instance(
            decoder,
            "<init>",
            "(Ljava/io/InputStream;I)V",
            &[Value::Obj(source), Value::Int(0)],
        )
        .unwrap();

    assert_eq!(
        engine.invoke_instance(decoder, "read", "()I", &[]).unwrap(),
        Value::Int(i32::from(b'h'))
    );
    let destination = byte_array(&mut engine, &[0; 5]);
    assert_eq!(
        engine
            .invoke_instance(
                decoder,
                "read",
                "([BII)I",
                &[destination.clone(), Value::Int(1), Value::Int(3)],
            )
            .unwrap(),
        Value::Int(3)
    );
    assert_eq!(
        bytes_of(&engine, &destination),
        vec![0, b'e', b'l', b'l', 0]
    );
    assert_eq!(
        engine
            .invoke_instance(decoder, "available", "()I", &[])
            .unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        engine.invoke_instance(decoder, "read", "()I", &[]).unwrap(),
        Value::Int(i32::from(b'o'))
    );
    assert_eq!(
        engine.invoke_instance(decoder, "read", "()I", &[]).unwrap(),
        Value::Int(-1)
    );
    engine
        .invoke_instance(decoder, "close", "()V", &[])
        .unwrap();
}
