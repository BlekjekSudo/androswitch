//! Compare actual shipping entry points with the frozen independent JVM.
use rd_vm::{Engine, ObjRef, Value, VmConfig, VmExit};
fn units(s: &str) -> String {
    s.encode_utf16()
        .map(|u| format!("{u:x}"))
        .collect::<Vec<_>>()
        .join(".")
}
fn decode(vm: &mut Engine, token: &str) -> Value {
    if token == "n" {
        return Value::Null;
    }
    let (kind, text) = token.split_once(':').unwrap();
    match kind {
        "i" => Value::Int(text.parse().unwrap()),
        "j" => Value::Long(text.parse().unwrap()),
        "f" => Value::Float(f32::from_bits(u32::from_str_radix(text, 16).unwrap())),
        "d" => Value::Double(f64::from_bits(u64::from_str_radix(text, 16).unwrap())),
        "s" | "a" => {
            let chars: Vec<u16> = if text.is_empty() {
                vec![]
            } else {
                text.split('.')
                    .map(|u| u16::from_str_radix(u, 16).unwrap())
                    .collect()
            };
            if kind == "s" {
                return Value::Obj(
                    vm.heap
                        .alloc_string(String::from_utf16(&chars).unwrap())
                        .unwrap(),
                );
            }
            let r = vm
                .heap
                .alloc_array_zeroed(rd_vm::heap::ElemKind::Int, chars.len(), "[C")
                .unwrap();
            for (slot, c) in vm.heap.array_elems_mut(r).unwrap().iter_mut().zip(chars) {
                *slot = Value::Int(i32::from(c));
            }
            Value::Obj(r)
        }
        _ => panic!("unknown frozen argument {token}"),
    }
}
fn array(vm: &Engine, r: ObjRef) -> String {
    "a:".to_string()
        + &vm
            .heap
            .as_array_elems(r)
            .unwrap()
            .0
            .iter()
            .map(|v| format!("{:x}", v.as_int().unwrap() as u16))
            .collect::<Vec<_>>()
            .join(".")
}
fn encode(vm: &Engine, v: Value) -> String {
    match v {
        Value::Null => "n".into(),
        Value::Int(x) => format!("i:{x}"),
        Value::Long(x) => format!("j:{x}"),
        Value::Float(x) => format!("f:{:x}", x.to_bits()),
        Value::Double(x) => format!("d:{:x}", x.to_bits()),
        Value::Obj(r) => {
            let class = vm.heap.class_of(r).unwrap();
            if class == "Ljava/lang/String;" {
                return "s:".to_string() + &units(vm.heap.as_str(r).unwrap());
            }
            if class == "[C" {
                return array(vm, r);
            }
            format!(
                "o:{class}:{}",
                encode(vm, vm.heap.get_field(r, "value").unwrap())
            )
        }
        other => panic!("unencoded result {other:?}"),
    }
}
#[test]
fn every_inherited_numeric_declaration_with_a_jvm_recipe_executes_the_same_contract() {
    let mut compared = 0;
    let mut declarations = std::collections::BTreeSet::new();
    for line in include_str!("../../../fixtures/numeric-oracle/catalog.tsv").lines() {
        let f: Vec<_> = line.split('\t').collect();
        assert_eq!(f.len(), 5);
        let (class, name, sig) = (f[0], f[1], f[2]);
        if rd_vm::numerics::method_metadata(class, name, sig).is_none() {
            continue;
        }
        let mut vm = Engine::new(vec![], VmConfig::default());
        let args = if f[3].is_empty() {
            vec![]
        } else {
            f[3].split(',')
                .map(|t| decode(&mut vm, t))
                .collect::<Vec<_>>()
        };
        let mut actual = match vm.invoke_static(class, name, sig, &args) {
            Ok(v) => encode(&vm, v),
            Err(VmExit::Exception(e)) => format!("e:{}", e.class),
            Err(e) => panic!("{class}->{name}{sig} ({}) : {e:?}", f[3]),
        };
        for arg in &args {
            if let Value::Obj(r) = arg {
                if vm.heap.class_of(*r).unwrap() == "[C" {
                    actual.push('|');
                    actual.push_str(&array(&vm, *r));
                }
            }
        }
        assert_eq!(actual, f[4], "{class}->{name}{sig} ({})", f[3]);
        compared += 1;
        declarations.insert((class, name, sig));
    }
    assert_eq!(
        compared, 233,
        "review the frozen JVM/metadata inventory before changing coverage"
    );
    assert_eq!(
        declarations.len(),
        218,
        "an inherited declaration was lost or added without inventory review"
    );
    println!(
        "{compared} JVM rows / {} exact inherited numeric declarations",
        declarations.len()
    );
}
