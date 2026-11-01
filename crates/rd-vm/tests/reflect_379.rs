//! Issue #379 (reflection surface): `Class.getMethods`, `getDeclaredMethods`
//! and `getConstructors` allocate `Method`/`Constructor` instances and an
//! array from inside a Rust host call. The result must name exactly the
//! members that Java would return, with correct descriptors.

use rd_dex::Dex;
use rd_testsupport::vm::*;
use rd_vm::{Engine, ObjRef, Value, VmConfig};

const CLS: &str = "LReflTarget;";
const ACC_PRIVATE: u32 = 0x2;
const ACC_CONSTRUCTOR: u32 = 0x10000;

fn engine() -> Engine {
    let mut b = DexBuilder::new();
    let cls = b.class(CLS, "Ljava/lang/Object;");
    let ctor_code = b.code(1, 1, 0, op10x(0x0e));
    b.direct(
        cls,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(ctor_code),
    );
    for (name, access) in [("a", ACC_PUBLIC), ("b", ACC_PUBLIC), ("c", ACC_PRIVATE)] {
        let code = b.code(1, 1, 0, op10x(0x0e));
        b.r#virtual(cls, name, "V", vec![], access, Some(code));
    }
    let dex = Dex::parse(b.finish()).expect("DEX");
    Engine::new(vec![dex], VmConfig::default())
}

fn class_obj(vm: &mut Engine, desc: &str) -> ObjRef {
    let s = vm.heap.alloc_string(desc.to_string()).expect("alloc name");
    vm.heap
        .alloc_instance(
            "Ljava/lang/Class;".to_string(),
            vec![("class_name".to_string(), Value::Obj(s))],
        )
        .expect("alloc Class")
}

/// Invokes `recv.name()` and returns `(name, descriptor)` for each element.
fn members(vm: &mut Engine, recv: ObjRef, name: &str, sig: &str) -> Vec<(String, String)> {
    let arr = match vm
        .invoke_instance(recv, name, sig, &[])
        .expect("reflect call")
    {
        Value::Obj(a) => a,
        other => panic!("{name} returned {other:?}, expected array"),
    };
    let elems: Vec<ObjRef> = vm
        .heap
        .as_array_elems(arr)
        .expect("array")
        .0
        .iter()
        .map(|v| match v {
            Value::Obj(o) => *o,
            other => panic!("element is not an object: {other:?}"),
        })
        .collect();
    elems
        .into_iter()
        .map(|o| {
            let field = |f: &str| match vm.heap.get_field(o, f).expect(f) {
                Value::Obj(s) => vm.heap.as_str(s).expect("as_str").to_string(),
                other => panic!("{f} is not a string: {other:?}"),
            };
            (field("rd_name"), field("rd_sig"))
        })
        .collect()
}

fn sorted_names(mut v: Vec<(String, String)>) -> Vec<String> {
    v.sort();
    v.into_iter().map(|(n, _)| n).collect()
}

#[test]
fn get_declared_methods_includes_private_members() {
    let mut vm = engine();
    let c = class_obj(&mut vm, CLS);
    let got = members(
        &mut vm,
        c,
        "getDeclaredMethods",
        "()[Ljava/lang/reflect/Method;",
    );
    assert_eq!(sorted_names(got.clone()), vec!["a", "b", "c"]);
    assert!(got.iter().all(|(_, sig)| sig == "()V"), "{got:?}");
}

#[test]
fn get_methods_returns_only_public_members() {
    let mut vm = engine();
    let c = class_obj(&mut vm, CLS);
    let got = members(&mut vm, c, "getMethods", "()[Ljava/lang/reflect/Method;");
    assert_eq!(sorted_names(got), vec!["a", "b"]);
}

#[test]
fn get_constructors_returns_public_init_only() {
    let mut vm = engine();
    let c = class_obj(&mut vm, CLS);
    let got = members(
        &mut vm,
        c,
        "getConstructors",
        "()[Ljava/lang/reflect/Constructor;",
    );
    assert_eq!(got, vec![("<init>".to_string(), "()V".to_string())]);
}
