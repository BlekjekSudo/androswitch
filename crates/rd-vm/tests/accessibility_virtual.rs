#![cfg(feature = "advanced-components")]
//! Standard mutable node APIs used by Compose's virtual node provider.
use rd_vm::m9;
use rd_vm::{Engine, Value, VmConfig};
const NODE: &str = "Landroid/view/accessibility/AccessibilityNodeInfo;";
fn reference(v: Option<Value>) -> u32 {
    if let Some(Value::Obj(r)) = v {
        r
    } else {
        panic!("{v:?}")
    }
}
fn call(e: &mut Engine, r: u32, n: &str, sig: &str, args: &[Value]) -> Value {
    m9::call_instance(e, NODE, n, sig, r, args)
        .unwrap()
        .unwrap()
}
#[test]
fn virtual_description_bounds_children_and_copy_are_stateful() {
    let mut e = Engine::new(vec![], VmConfig::default());
    let node = reference(
        m9::call_static(
            &mut e,
            NODE,
            "obtain",
            "()Landroid/view/accessibility/AccessibilityNodeInfo;",
            &[],
        )
        .unwrap(),
    );
    let desc = e.heap.alloc_string("Open shopping lists".into()).unwrap();
    call(
        &mut e,
        node,
        "setContentDescription",
        "(Ljava/lang/CharSequence;)V",
        &[Value::Obj(desc)],
    );
    assert_eq!(
        call(
            &mut e,
            node,
            "getContentDescription",
            "()Ljava/lang/CharSequence;",
            &[]
        ),
        Value::Obj(desc)
    );
    let rect = e
        .heap
        .alloc_instance(
            "Landroid/graphics/Rect;".into(),
            vec![
                ("left".into(), Value::Int(10)),
                ("top".into(), Value::Int(20)),
                ("right".into(), Value::Int(60)),
                ("bottom".into(), Value::Int(70)),
            ],
        )
        .unwrap();
    call(
        &mut e,
        node,
        "setBoundsInScreen",
        "(Landroid/graphics/Rect;)V",
        &[Value::Obj(rect)],
    );
    e.heap.put_field(rect, "left", Value::Int(999)).unwrap();
    call(
        &mut e,
        node,
        "getBoundsInScreen",
        "(Landroid/graphics/Rect;)V",
        &[Value::Obj(rect)],
    );
    assert_eq!(
        e.heap.get_field(rect, "left").unwrap(),
        Value::Int(10),
        "bounds copied, not aliased"
    );
    let host = e
        .heap
        .alloc_instance("Landroid/view/View;".into(), vec![])
        .unwrap();
    call(
        &mut e,
        node,
        "addChild",
        "(Landroid/view/View;I)V",
        &[Value::Obj(host), Value::Int(12)],
    );
    assert_eq!(
        call(&mut e, node, "getChildCount", "()I", &[]),
        Value::Int(1)
    );
    let copied=reference(m9::call_static(&mut e,NODE,"obtain","(Landroid/view/accessibility/AccessibilityNodeInfo;)Landroid/view/accessibility/AccessibilityNodeInfo;",&[Value::Obj(node)]).unwrap());
    call(
        &mut e,
        node,
        "setContentDescription",
        "(Ljava/lang/CharSequence;)V",
        &[Value::Null],
    );
    assert_eq!(
        call(
            &mut e,
            copied,
            "getContentDescription",
            "()Ljava/lang/CharSequence;",
            &[]
        ),
        Value::Obj(desc)
    );
    let extras = call(&mut e, copied, "getExtras", "()Landroid/os/Bundle;", &[]);
    assert!(matches!(extras, Value::Obj(_)));
    assert_eq!(
        call(&mut e, copied, "getExtras", "()Landroid/os/Bundle;", &[]),
        extras
    );
}
#[test]
fn accessibility_action_carries_platform_id_and_label() {
    let mut e = Engine::new(vec![], VmConfig::default());
    let class = "Landroid/view/accessibility/AccessibilityNodeInfo$AccessibilityAction;";
    let action = e.heap.alloc_instance(class.into(), vec![]).unwrap();
    let label = e.heap.alloc_string("Activate".into()).unwrap();
    m9::call_instance(
        &mut e,
        class,
        "<init>",
        "(ILjava/lang/CharSequence;)V",
        action,
        &[Value::Int(16), Value::Obj(label)],
    )
    .unwrap();
    assert_eq!(
        m9::call_instance(&mut e, class, "getId", "()I", action, &[]).unwrap(),
        Some(Value::Int(16))
    );
    assert_eq!(
        m9::call_instance(
            &mut e,
            class,
            "getLabel",
            "()Ljava/lang/CharSequence;",
            action,
            &[]
        )
        .unwrap(),
        Some(Value::Obj(label))
    );
}
