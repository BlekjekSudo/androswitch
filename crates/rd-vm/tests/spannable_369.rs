//! Regression test for issue #369 (`crates/rd-vm/src/android_spannable.rs`):
//! `SpannableStringBuilder.append(this)` doubles the host text on each call.
//! The grown length must be charged to `heap_budget`, so repeated self-append
//! throws a guest-catchable `OutOfMemoryError` instead of exhausting the host.

use rd_vm::err::VmExit;
use rd_vm::value::Value;
use rd_vm::{Engine, VmConfig};

const SSB: &str = "Landroid/text/SpannableStringBuilder;";
const APPEND_CS: &str = "(Ljava/lang/CharSequence;)Landroid/text/SpannableStringBuilder;";

fn engine(heap_budget: usize) -> Engine {
    Engine::new(
        Vec::new(),
        VmConfig {
            heap_budget,
            ..VmConfig::default()
        },
    )
}

fn obj(v: Value) -> u32 {
    match v {
        Value::Obj(r) => r,
        other => panic!("expected Obj, got {other:?}"),
    }
}

#[test]
fn issue_369_plain_append_still_works() {
    let mut e = engine(64 * 1024 * 1024);
    let sb = e.new_instance(SSB).unwrap();
    let a = Value::Obj(e.heap.alloc_string("ab".to_string()).unwrap());
    e.invoke_instance(sb, "append", APPEND_CS, &[a]).unwrap();
    let b = Value::Obj(e.heap.alloc_string("cd".to_string()).unwrap());
    e.invoke_instance(sb, "append", APPEND_CS, &[b]).unwrap();
    let out = e
        .invoke_instance(sb, "toString", "()Ljava/lang/String;", &[])
        .unwrap();
    assert_eq!(e.heap.as_str(obj(out)).unwrap(), "abcd");
}

#[test]
fn issue_369_self_append_doubling_hits_heap_budget_as_oome() {
    // 2 MiB budget: the builder text doubles each call and must hit the budget
    // (around 2^20 u16 units) well before the 25-step cap used here.
    let mut e = engine(2 * 1024 * 1024);
    let sb = e.new_instance(SSB).unwrap();
    let seed = Value::Obj(e.heap.alloc_string("x".to_string()).unwrap());
    e.invoke_instance(sb, "append", APPEND_CS, &[seed]).unwrap();
    let mut oome = None;
    for _ in 0..25 {
        match e.invoke_instance(sb, "append", APPEND_CS, &[Value::Obj(sb)]) {
            Ok(_) => continue,
            Err(VmExit::Exception(t)) => {
                oome = Some(t.class);
                break;
            }
            Err(other) => panic!("unexpected exit: {other:?}"),
        }
    }
    assert_eq!(oome.as_deref(), Some("Ljava/lang/OutOfMemoryError;"));
}
