//! Regression test for issue #374 item 2b (`crates/rd-vm/src/jregex.rs`):
//! each `Matcher` keeps a copy of its input, and that copy is charged to the
//! guest heap budget, so creating many matchers over one large input cannot
//! grow host memory without limit.

use rd_vm::err::VmExit;
use rd_vm::value::Value;
use rd_vm::{Engine, VmConfig};

const PATTERN: &str = "Ljava/util/regex/Pattern;";
const BUDGET: usize = 8 * 1024 * 1024;
const CHUNK: usize = 256 * 1024;
const MAX_MATCHERS: usize = 64;

#[test]
fn issue_374_matcher_input_copies_are_charged_to_heap_budget() {
    let mut e = Engine::new(
        Vec::new(),
        VmConfig {
            heap_budget: BUDGET,
            ..VmConfig::default()
        },
    );
    let src = Value::Obj(e.heap.alloc_string("a".to_string()).unwrap());
    let pattern = e
        .invoke_static(
            PATTERN,
            "compile",
            "(Ljava/lang/String;)Ljava/util/regex/Pattern;",
            &[src],
        )
        .expect("compile");
    let Value::Obj(pattern) = pattern else {
        panic!("expected Pattern")
    };
    let input = Value::Obj(e.heap.alloc_string("a".repeat(CHUNK)).unwrap());
    let mut hit_oome = false;
    for _ in 0..MAX_MATCHERS {
        match e.invoke_instance(
            pattern,
            "matcher",
            "(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;",
            std::slice::from_ref(&input),
        ) {
            Ok(_) => {}
            Err(VmExit::Exception(t)) => {
                assert_eq!(t.class, "Ljava/lang/OutOfMemoryError;");
                hit_oome = true;
                break;
            }
            Err(other) => panic!("unexpected error {other:?}"),
        }
    }
    assert!(
        hit_oome,
        "{MAX_MATCHERS} matchers over a {CHUNK}-byte input never hit the {BUDGET}-byte budget"
    );
}
