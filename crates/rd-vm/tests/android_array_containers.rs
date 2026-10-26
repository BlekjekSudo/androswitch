//! Guest-visible proof that `ArrayMap`/`ArraySet` use Java `hashCode`
//! and guest `equals`, while keeping equal-hash entries in insertion order.

mod common;

use common::*;
use rd_vm::value::Value;

const ARRAY_MAP: &str = "Landroid/util/ArrayMap;";
const ARRAY_SET: &str = "Landroid/util/ArraySet;";
const KEY: &str = "LHashKey;";
const TEST_MAP: &str = "LTestArrayMap;";
const TEST_SET: &str = "LTestArraySet;";
const ACC_CONSTRUCTOR: u32 = 0x1_0000;

fn build() -> DexBuilder {
    let mut b = DexBuilder::new();
    let key = b.class(KEY, "Ljava/lang/Object;");
    b.instance_field(key, "id", "I");
    let id = b.field_idx(KEY, "I", "id");

    // equals(Object) { return this.id == ((HashKey) other).id; }
    // regs=4, ins=2: v2=this, v3=other, v0/v1 are the two ids.
    b.r#virtual(
        key,
        "equals",
        "Z",
        vec!["Ljava/lang/Object;"],
        ACC_PUBLIC,
        Some(b.code(4, 2, 0, {
            let mut code = op22c(0x52, 0, 2, id); // iget v0, v2, id
            code.extend(op22c(0x52, 1, 3, id)); // iget v1, v3, id
            code.extend(op22t(0x32, 0, 1, 4)); // if-eq v0, v1, +4
            code.extend(op11n(0x12, 0, 0));
            code.extend(op11x(0x0f, 0));
            code.extend(op11n(0x12, 0, 1));
            code.extend(op11x(0x0f, 0));
            code
        })),
    );

    // hashCode() { return 7; } — deliberately collide every key so the
    // test proves AOSP's stable insertion order within one hash bucket.
    b.r#virtual(
        key,
        "hashCode",
        "I",
        vec![],
        ACC_PUBLIC,
        Some(b.code(2, 1, 0, {
            let mut code = op11n(0x12, 0, 7);
            code.extend(op11x(0x0f, 0));
            code
        })),
    );

    // DEX subclasses with no overrides: their constructors only chain to
    // the host ArrayMap/ArraySet no-arg constructors.
    let map_init = {
        let proto = b.proto_idx("V", vec![]);
        b.method_idx(ARRAY_MAP, proto, "<init>")
    };
    let test_map = b.class(TEST_MAP, ARRAY_MAP);
    b.direct(
        test_map,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut code = op35c(0x70, 1, map_init, [0, 0, 0, 0, 0]);
            code.extend(op10x(0x0e));
            code
        })),
    );

    let set_init = {
        let proto = b.proto_idx("V", vec![]);
        b.method_idx(ARRAY_SET, proto, "<init>")
    };
    let test_set = b.class(TEST_SET, ARRAY_SET);
    b.direct(
        test_set,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut code = op35c(0x70, 1, set_init, [0, 0, 0, 0, 0]);
            code.extend(op10x(0x0e));
            code
        })),
    );

    b
}

fn text(engine: &mut rd_vm::Engine, value: &str) -> Value {
    Value::Obj(engine.heap.alloc_string(value.to_string()).unwrap())
}

fn key(engine: &mut rd_vm::Engine, id: i32) -> Value {
    Value::Obj(
        engine
            .heap
            .alloc_instance(KEY.to_string(), vec![("id".to_string(), Value::Int(id))])
            .unwrap(),
    )
}

fn key_id(engine: &rd_vm::Engine, value: &Value) -> i32 {
    let Value::Obj(obj) = value else {
        panic!("expected HashKey object, got {value:?}");
    };
    engine.heap.get_field(*obj, "id").unwrap().as_int().unwrap()
}

#[test]
fn array_containers_consult_guest_hash_code_and_equals() {
    let builder = build();
    let mut engine = engine_of(&builder);

    let map = engine
        .heap
        .alloc_instance(ARRAY_MAP.to_string(), Vec::new())
        .unwrap();
    engine.invoke_instance(map, "<init>", "()V", &[]).unwrap();
    let put_sig = "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
    let get_sig = "(Ljava/lang/Object;)Ljava/lang/Object;";

    let key2 = key(&mut engine, 2);
    let two = text(&mut engine, "two");
    assert_eq!(
        engine
            .invoke_instance(map, "put", put_sig, &[key2, two])
            .unwrap(),
        Value::Null
    );
    let key1 = key(&mut engine, 1);
    let one = text(&mut engine, "one");
    assert_eq!(
        engine
            .invoke_instance(map, "put", put_sig, &[key1, one])
            .unwrap(),
        Value::Null
    );

    // A distinct guest object with id=2 is the same map key: its constant
    // hashCode selects the bucket and its guest equals replaces the value.
    let duplicate2 = key(&mut engine, 2);
    let two_again = text(&mut engine, "two-again");
    let old = engine
        .invoke_instance(map, "put", put_sig, &[duplicate2.clone(), two_again])
        .unwrap();
    let Value::Obj(old) = old else {
        panic!("duplicate put did not return the old String value");
    };
    assert_eq!(engine.heap.as_str(old).unwrap(), "two");
    assert_eq!(
        engine.invoke_instance(map, "size", "()I", &[]).unwrap(),
        Value::Int(2)
    );
    let fetched = engine
        .invoke_instance(map, "get", get_sig, std::slice::from_ref(&duplicate2))
        .unwrap();
    let Value::Obj(fetched) = fetched else {
        panic!("duplicate get did not return a String");
    };
    assert_eq!(engine.heap.as_str(fetched).unwrap(), "two-again");

    // All guest keys collide at hash 7, so keyAt keeps insertion order.
    let first_key = engine
        .invoke_instance(map, "keyAt", "(I)Ljava/lang/Object;", &[Value::Int(0)])
        .unwrap();
    let second_key = engine
        .invoke_instance(map, "keyAt", "(I)Ljava/lang/Object;", &[Value::Int(1)])
        .unwrap();
    assert_eq!(
        (key_id(&engine, &first_key), key_id(&engine, &second_key)),
        (2, 1)
    );

    let set = engine
        .heap
        .alloc_instance(ARRAY_SET.to_string(), Vec::new())
        .unwrap();
    engine.invoke_instance(set, "<init>", "()V", &[]).unwrap();
    let add_sig = "(Ljava/lang/Object;)Z";
    let set_key2 = key(&mut engine, 2);
    let set_key1 = key(&mut engine, 1);
    let set_duplicate2 = key(&mut engine, 2);
    assert_eq!(
        engine
            .invoke_instance(set, "add", add_sig, &[set_key2])
            .unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        engine
            .invoke_instance(set, "add", add_sig, &[set_key1])
            .unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        engine
            .invoke_instance(set, "add", add_sig, std::slice::from_ref(&set_duplicate2))
            .unwrap(),
        Value::Int(0)
    );
    let first_value = engine
        .invoke_instance(set, "valueAt", "(I)Ljava/lang/Object;", &[Value::Int(0)])
        .unwrap();
    let second_value = engine
        .invoke_instance(set, "valueAt", "(I)Ljava/lang/Object;", &[Value::Int(1)])
        .unwrap();
    assert_eq!(
        (
            key_id(&engine, &first_value),
            key_id(&engine, &second_value)
        ),
        (2, 1)
    );
}

#[test]
fn dex_subclasses_inherit_array_container_operations() {
    let builder = build();
    let mut engine = engine_of(&builder);

    let map = engine.new_dex_instance_pub(TEST_MAP).unwrap();
    let key = text(&mut engine, "answer");
    let value = engine
        .invoke_static(
            "Ljava/lang/Integer;",
            "valueOf",
            "(I)Ljava/lang/Integer;",
            &[Value::Int(42)],
        )
        .unwrap();
    assert_eq!(
        engine
            .invoke_instance(
                map,
                "put",
                "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
                &[key.clone(), value],
            )
            .unwrap(),
        Value::Null
    );
    let fetched = engine
        .invoke_instance(
            map,
            "get",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            std::slice::from_ref(&key),
        )
        .unwrap();
    let Value::Obj(fetched) = fetched else {
        panic!("subclass ArrayMap.get did not return the boxed value");
    };
    assert_eq!(
        engine
            .invoke_instance(fetched, "intValue", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );

    let set = engine.new_dex_instance_pub(TEST_SET).unwrap();
    let element = text(&mut engine, "only");
    assert_eq!(
        engine
            .invoke_instance(
                set,
                "add",
                "(Ljava/lang/Object;)Z",
                std::slice::from_ref(&element)
            )
            .unwrap(),
        Value::Int(1)
    );
    let duplicate = text(&mut engine, "only");
    assert_eq!(
        engine
            .invoke_instance(set, "add", "(Ljava/lang/Object;)Z", &[duplicate])
            .unwrap(),
        Value::Int(0)
    );
    let at0 = engine
        .invoke_instance(set, "valueAt", "(I)Ljava/lang/Object;", &[Value::Int(0)])
        .unwrap();
    let Value::Obj(at0) = at0 else {
        panic!("subclass ArraySet.valueAt did not return the String element");
    };
    assert_eq!(engine.heap.as_str(at0).unwrap(), "only");
}
