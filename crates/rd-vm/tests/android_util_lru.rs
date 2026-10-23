//! Guest-level proof that a DEX `LruCache` subclass keeps its AOSP
//! `create`, `sizeOf` and `entryRemoved` callbacks while the host owns the
//! final access-order cache operations.

mod common;

use common::*;
use rd_vm::value::Value;

const ACC_CONSTRUCTOR: u32 = 0x1_0000;
const LRU: &str = "Landroid/util/LruCache;";
const TEST_CACHE: &str = "LTestLruCache;";

fn build() -> DexBuilder {
    let mut b = DexBuilder::new();
    let super_init = {
        let proto = b.proto_idx("V", vec!["I".into()]);
        b.method_idx(LRU, proto, "<init>")
    };

    let cache = b.class(TEST_CACHE, LRU);
    b.instance_field(cache, "createCalls", "I");
    b.instance_field(cache, "removedCalls", "I");
    b.instance_field(cache, "lastEvicted", "I");
    b.instance_field(cache, "lastKey", "Ljava/lang/Object;");
    let create_calls = b.field_idx(TEST_CACHE, "I", "createCalls");
    let removed_calls = b.field_idx(TEST_CACHE, "I", "removedCalls");
    let last_evicted = b.field_idx(TEST_CACHE, "I", "lastEvicted");
    let last_key = b.field_idx(TEST_CACHE, "Ljava/lang/Object;", "lastKey");

    // <init>()V { super(4); }  this=v2, capacity temp=v0.
    b.direct(
        cache,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(3, 1, 2, {
            let mut code = op11n(0x12, 0, 4);
            code.extend(op35c(0x70, 2, super_init, [2, 0, 0, 0, 0]));
            code.extend(op10x(0x0e));
            code
        })),
    );

    // create(key) { createCalls++; return key; }
    // this=v2, key=v3.
    b.r#virtual(
        cache,
        "create",
        "Ljava/lang/Object;",
        vec!["Ljava/lang/Object;"],
        ACC_PUBLIC,
        Some(b.code(4, 2, 0, {
            let mut code = op22c(0x52, 0, 2, create_calls);
            code.extend(op11n(0x12, 1, 1));
            code.extend(op12x(0xb0, 0, 1));
            code.extend(op22c(0x59, 0, 2, create_calls));
            code.extend(op11x(0x11, 3));
            code
        })),
    );

    // entryRemoved(evicted, key, oldValue, newValue) {
    //   removedCalls++; lastEvicted = evicted; lastKey = key;
    // }
    // this=v2, evicted=v3, key=v4.
    b.r#virtual(
        cache,
        "entryRemoved",
        "V",
        vec![
            "Z",
            "Ljava/lang/Object;",
            "Ljava/lang/Object;",
            "Ljava/lang/Object;",
        ],
        ACC_PUBLIC,
        Some(b.code(7, 5, 0, {
            let mut code = op22c(0x52, 0, 2, removed_calls);
            code.extend(op11n(0x12, 1, 1));
            code.extend(op12x(0xb0, 0, 1));
            code.extend(op22c(0x59, 0, 2, removed_calls));
            code.extend(op22c(0x59, 3, 2, last_evicted));
            code.extend(op22c(0x5b, 4, 2, last_key));
            code.extend(op10x(0x0e));
            code
        })),
    );

    // sizeOf(key, value) { return 2; }
    b.r#virtual(
        cache,
        "sizeOf",
        "I",
        vec!["Ljava/lang/Object;", "Ljava/lang/Object;"],
        ACC_PUBLIC,
        Some(b.code(4, 3, 0, {
            let mut code = op11n(0x12, 0, 2);
            code.extend(op11x(0x0f, 0));
            code
        })),
    );

    b
}

fn string(engine: &mut rd_vm::Engine, value: &str) -> Value {
    Value::Obj(engine.heap.alloc_string(value.to_string()).unwrap())
}

#[test]
fn dex_lru_cache_callbacks_drive_creation_sizing_and_eviction() {
    let builder = build();
    let mut engine = engine_of(&builder);
    let cache = engine.new_dex_instance_pub(TEST_CACHE).unwrap();
    let get_sig = "(Ljava/lang/Object;)Ljava/lang/Object;";

    let first = string(&mut engine, "first");
    let created = engine
        .invoke_instance(cache, "get", get_sig, std::slice::from_ref(&first))
        .unwrap();
    assert_eq!(created, first);
    assert_eq!(
        engine.heap.get_field(cache, "createCalls").unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        engine.invoke_instance(cache, "size", "()I", &[]).unwrap(),
        Value::Int(2)
    );

    // Equal-content String keys hit the same entry and do not create again.
    let first_again = string(&mut engine, "first");
    assert_eq!(
        engine
            .invoke_instance(cache, "get", get_sig, &[first_again])
            .unwrap(),
        first
    );
    assert_eq!(
        engine.heap.get_field(cache, "createCalls").unwrap(),
        Value::Int(1)
    );

    let second = string(&mut engine, "second");
    engine
        .invoke_instance(cache, "get", get_sig, &[second])
        .unwrap();
    let third = string(&mut engine, "third");
    engine
        .invoke_instance(cache, "get", get_sig, &[third])
        .unwrap();

    // Capacity is 4 and every entry costs 2, so creating the third entry
    // evicts `first`, the least recently used key.
    assert_eq!(
        engine.invoke_instance(cache, "size", "()I", &[]).unwrap(),
        Value::Int(4)
    );
    assert_eq!(
        engine
            .invoke_instance(cache, "createCount", "()I", &[])
            .unwrap(),
        Value::Int(3)
    );
    assert_eq!(
        engine
            .invoke_instance(cache, "evictionCount", "()I", &[])
            .unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        engine.heap.get_field(cache, "removedCalls").unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        engine.heap.get_field(cache, "lastEvicted").unwrap(),
        Value::Int(1)
    );
    let Value::Obj(last_key) = engine.heap.get_field(cache, "lastKey").unwrap() else {
        panic!("entryRemoved did not receive the evicted key");
    };
    assert_eq!(engine.heap.as_str(last_key).unwrap(), "first");
}
