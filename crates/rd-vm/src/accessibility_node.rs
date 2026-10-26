//! Mutable platform AccessibilityNodeInfo objects for virtual-node providers.
//! No Compose internals or obfuscated names are inspected: providers run DEX
//! and populate the ordinary Android node API.
use crate::err::{put_field_result, VmExit};
use crate::heap::ObjRef;
#[cfg(not(feature = "std"))]
#[allow(unused_imports)] // Explicit alloc prelude for the inherited module.
use crate::port_prelude::*;
use crate::{Engine, Value};
const NODE: &str = "Landroid/view/accessibility/AccessibilityNodeInfo;";
fn put(vm: &mut Engine, r: ObjRef, k: &str, v: Value) -> Result<(), VmExit> {
    put_field_result(vm.heap.put_field(r, k, v))
}
fn get(vm: &Engine, r: ObjRef, k: &str) -> Value {
    vm.heap.get_field(r, k).unwrap_or(Value::Null)
}
fn integer(v: Value) -> i32 {
    if let Value::Int(i) = v {
        i
    } else {
        0
    }
}

pub(crate) fn obtain(vm: &mut Engine, args: &[Value]) -> Result<Value, VmExit> {
    let r = vm.heap.alloc_instance(NODE.into(), vec![])?;
    if let Some(Value::Obj(src)) = args.first() {
        if vm.heap.class_of(*src)? == NODE {
            if let crate::heap::HeapObj::Instance { fields, .. } = vm.heap.get(*src)?.clone() {
                for (k, v) in fields {
                    put(vm, r, &k, v)?;
                }
            }
        } else {
            initialize_view(vm, *src, r)?;
        }
    }
    Ok(Value::Obj(r))
}

pub(crate) fn initialize_view(vm: &mut Engine, view: ObjRef, r: ObjRef) -> Result<(), VmExit> {
    put(vm, r, "Source", Value::Obj(view))?;
    put(vm, r, "VirtualId", Value::Int(-1))?;
    let cls = vm
        .heap
        .class_of(view)?
        .trim_start_matches('L')
        .trim_end_matches(';')
        .replace('/', ".");
    let cls = vm.heap.alloc_string(cls)?;
    put(vm, r, "ClassName", Value::Obj(cls))?;
    if let Some(crate::framework::HostObj::View {
        text,
        enabled,
        click_listener,
        x,
        y,
        w,
        h,
        ..
    }) = vm.fw.objects.get(&view).cloned()
    {
        let text = vm.heap.alloc_string(text)?;
        put(vm, r, "Text", Value::Obj(text))?;
        put(vm, r, "Enabled", Value::Int(enabled as i32))?;
        put(
            vm,
            r,
            "Clickable",
            Value::Int(click_listener.is_some() as i32),
        )?;
        put(vm, r, "VisibleToUser", Value::Int(1))?;
        for (k, v) in [("left", x), ("top", y), ("right", x + w), ("bottom", y + h)] {
            put(vm, r, &format!("BoundsInScreen${k}"), Value::Int(v))?;
        }
    }
    Ok(())
}

pub(crate) fn call(
    vm: &mut Engine,
    r: ObjRef,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let out = match name {
        "<init>" => Value::Null,
        "recycle" => Value::Null, // immutable snapshot remains inspectable; GC reclaims it
        "getExtras" => match get(vm, r, "Extras") {
            Value::Obj(b) => Value::Obj(b),
            _ => {
                let b = vm
                    .heap
                    .alloc_instance("Landroid/os/Bundle;".into(), vec![])?;
                vm.fw
                    .objects
                    .insert(b, crate::framework::HostObj::Bundle { map: vec![] });
                put(vm, r, "Extras", Value::Obj(b))?;
                Value::Obj(b)
            }
        },
        "setBoundsInScreen" | "setBoundsInParent" => {
            if let Some(Value::Obj(rect)) = args.first() {
                for k in ["left", "top", "right", "bottom"] {
                    put(
                        vm,
                        r,
                        &format!("{}${k}", &name[3..]),
                        vm.heap.get_field(*rect, k).unwrap_or(Value::Int(0)),
                    )?;
                }
            }
            Value::Null
        }
        "getBoundsInScreen" | "getBoundsInParent" => {
            if let Some(Value::Obj(rect)) = args.first() {
                for k in ["left", "top", "right", "bottom"] {
                    put(
                        vm,
                        *rect,
                        k,
                        Value::Int(integer(get(vm, r, &format!("{}${k}", &name[3..])))),
                    )?;
                }
            }
            Value::Null
        }
        "addChild" => {
            let n = integer(get(vm, r, "ChildCount"));
            put(
                vm,
                r,
                &format!("ChildView{n}"),
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            put(
                vm,
                r,
                &format!("ChildId{n}"),
                args.get(1).cloned().unwrap_or(Value::Int(-1)),
            )?;
            put(vm, r, "ChildCount", Value::Int(n + 1))?;
            Value::Null
        }
        "setSource" | "setParent" => {
            put(
                vm,
                r,
                &name[3..],
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            put(
                vm,
                r,
                &format!("{}Id", &name[3..]),
                args.get(1).cloned().unwrap_or(Value::Int(-1)),
            )?;
            Value::Null
        }
        "setTextSelection" => {
            put(
                vm,
                r,
                "TextSelectionStart",
                args.first().cloned().unwrap_or(Value::Int(-1)),
            )?;
            put(
                vm,
                r,
                "TextSelectionEnd",
                args.get(1).cloned().unwrap_or(Value::Int(-1)),
            )?;
            Value::Null
        }
        "addAction" => {
            let n = integer(get(vm, r, "ActionCount"));
            put(
                vm,
                r,
                &format!("Action{n}"),
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            put(vm, r, "ActionCount", Value::Int(n + 1))?;
            Value::Null
        }
        "getActionList" => {
            let n = integer(get(vm, r, "ActionCount"));
            let actions = (0..n)
                .filter_map(|i| match get(vm, r, &format!("Action{i}")) {
                    Value::Obj(a) => Some(Value::Obj(a)),
                    _ => None,
                })
                .collect();
            crate::collections::new_list(vm, "Ljava/util/ArrayList;", actions)?
        }
        "hashCode" => Value::Int(r as i32),
        "equals" => Value::Int((args.first() == Some(&Value::Obj(r))) as i32),
        n if n.starts_with("set") && sig.ends_with(")V") && args.len() == 1 => {
            put(vm, r, &n[3..], args[0].clone())?;
            Value::Null
        }
        n if args.is_empty() && (n.starts_with("get") || n.starts_with("is")) => {
            let key = n
                .strip_prefix("get")
                .or_else(|| n.strip_prefix("is"))
                .unwrap_or(n);
            let v = get(vm, r, key);
            if matches!(v, Value::Null) && (sig.ends_with(")I") || sig.ends_with(")Z")) {
                Value::Int(
                    if key == "MaxTextLength" || key.starts_with("TextSelection") {
                        -1
                    } else {
                        0
                    },
                )
            } else {
                v
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(out))
}

/// Toolkit-independent virtual node description returned by an app provider.
#[derive(Debug, Clone)]
pub struct VirtualSemanticNode {
    pub host: ObjRef,
    pub virtual_id: i32,
    pub text: String,
    pub description: String,
    pub bounds: [i32; 4],
    pub clickable: bool,
}
fn text(vm: &mut Engine, r: ObjRef, k: &str) -> Result<String, VmExit> {
    match get(vm, r, k) {
        Value::Obj(s) => crate::intrinsics::object_to_string(vm, s),
        _ => Ok(String::new()),
    }
}
impl Engine {
    /// Query registered platform delegates/providers, not toolkit internals.
    /// Limits bound broken/cyclic providers. Errors are returned to the observer.
    pub fn virtual_semantics(&mut self) -> Result<Vec<VirtualSemanticNode>, VmExit> {
        self.with_host_roots(|vm| {
            let delegates: Vec<_> = vm
                .view_props
                .iter()
                .filter_map(|((view, key), v)| {
                    if key == "AccessibilityDelegate" {
                        if let Value::Obj(d) = v {
                            Some((*view, *d))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect();
            let mut out = Vec::new();
            let mut visited = 0usize;
            for (host, delegate) in delegates {
                // Ignore detached views and ordinary delegates returning null.
                if !vm.attached_views.contains(&host) {
                    continue;
                }
                let provider = vm.invoke_instance(
                    delegate,
                    "getAccessibilityNodeProvider",
                    "(Landroid/view/View;)Landroid/view/accessibility/AccessibilityNodeProvider;",
                    &[Value::Obj(host)],
                )?;
                let Value::Obj(provider) = provider else {
                    continue;
                };
                let mut queue = alloc::collections::VecDeque::from([-1]);
                // Deduplicate on enqueue, not just after processing, to bound
                // cyclic/shared child edges as well as provider callbacks.
                let mut seen = crate::maps::HashSet::from([-1]);
                while let Some(id) = queue.pop_front() {
                    if visited >= 2048 {
                        return Err(crate::err::vm_error(
                            "ACCESSIBILITY_LIMIT",
                            "virtual provider exceeds 2048 nodes",
                        )
                        .into());
                    }
                    visited += 1;
                    let node = vm.invoke_instance(
                        provider,
                        "createAccessibilityNodeInfo",
                        "(I)Landroid/view/accessibility/AccessibilityNodeInfo;",
                        &[Value::Int(id)],
                    )?;
                    let Value::Obj(node) = node else {
                        continue;
                    };
                    let n = integer(get(vm, node, "ChildCount"));
                    if !(0..=2048).contains(&n) {
                        return Err(crate::err::vm_error(
                            "ACCESSIBILITY_LIMIT",
                            "virtual node exceeds 2048 children",
                        )
                        .into());
                    }
                    for i in 0..n {
                        let v = get(vm, node, &format!("ChildView{i}"));
                        if v == Value::Obj(host) {
                            let child = integer(get(vm, node, &format!("ChildId{i}")));
                            if seen.insert(child) {
                                queue.push_back(child);
                            }
                            if seen.len() > 2048 {
                                return Err(crate::err::vm_error(
                                    "ACCESSIBILITY_LIMIT",
                                    "virtual provider exceeds 2048 nodes",
                                )
                                .into());
                            }
                        }
                    }
                    let bounds = ["left", "top", "right", "bottom"]
                        .map(|k| integer(get(vm, node, &format!("BoundsInScreen${k}"))));
                    if integer(get(vm, node, "VisibleToUser")) != 0
                        && bounds[2] > bounds[0]
                        && bounds[3] > bounds[1]
                    {
                        out.push(VirtualSemanticNode {
                            host,
                            virtual_id: id,
                            text: text(vm, node, "Text")?,
                            description: text(vm, node, "ContentDescription")?,
                            bounds,
                            clickable: integer(get(vm, node, "Clickable")) != 0,
                        });
                    }
                }
            }
            Ok(out)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
    #[allow(unused_imports)]
    use crate::port_prelude::*;
    use rd_testsupport::vm::*;
    fn fixture() -> (Engine, ObjRef, ObjRef) {
        let mut b = DexBuilder::new();
        let p = b.class(
            "LTestProvider;",
            "Landroid/view/accessibility/AccessibilityNodeProvider;",
        );
        let d = b.class(
            "LTestDelegate;",
            "Landroid/view/View$AccessibilityDelegate;",
        );
        let root = b.field_idx("LTestProvider;", NODE, "root");
        let child = b.field_idx("LTestProvider;", NODE, "child");
        let provider = b.field_idx("LTestDelegate;", "LTestProvider;", "provider");
        let mut body = op11n(0x12, 0, -1);
        body.extend(op22t(0x33, 2, 0, 5));
        body.extend(op22c(0x54, 0, 1, root));
        body.extend(op11x(0x11, 0));
        body.extend(op22c(0x54, 0, 1, child));
        body.extend(op11x(0x11, 0));
        b.r#virtual(
            p,
            "createAccessibilityNodeInfo",
            NODE,
            vec!["I"],
            ACC_PUBLIC,
            Some(b.code(3, 2, 0, body)),
        );
        let mut body = op22c(0x54, 0, 1, provider);
        body.extend(op11x(0x11, 0));
        b.r#virtual(
            d,
            "getAccessibilityNodeProvider",
            "Landroid/view/accessibility/AccessibilityNodeProvider;",
            vec!["Landroid/view/View;"],
            ACC_PUBLIC,
            Some(b.code(3, 2, 0, body)),
        );
        let dex = rd_dex::Dex::parse(b.finish()).unwrap();
        let mut vm = Engine::new(vec![dex], Default::default());
        let host = vm
            .heap
            .alloc_instance("Landroid/view/View;".into(), vec![])
            .unwrap();
        let Value::Obj(root) = obtain(&mut vm, &[]).unwrap() else {
            panic!()
        };
        let Value::Obj(child) = obtain(&mut vm, &[]).unwrap() else {
            panic!()
        };
        call(
            &mut vm,
            root,
            "addChild",
            "(Landroid/view/View;I)V",
            &[Value::Obj(host), Value::Int(7)],
        )
        .unwrap();
        call(
            &mut vm,
            child,
            "addChild",
            "(Landroid/view/View;I)V",
            &[Value::Obj(host), Value::Int(-1)],
        )
        .unwrap();
        for (k, v) in [
            ("VisibleToUser", Value::Int(1)),
            ("BoundsInScreen$left", Value::Int(0)),
            ("BoundsInScreen$top", Value::Int(0)),
            ("BoundsInScreen$right", Value::Int(20)),
            ("BoundsInScreen$bottom", Value::Int(30)),
        ] {
            put(&mut vm, child, k, v).unwrap();
        }
        let label = vm.heap.alloc_string("Named icon".into()).unwrap();
        put(&mut vm, child, "ContentDescription", Value::Obj(label)).unwrap();
        let provider = vm
            .heap
            .alloc_instance(
                "LTestProvider;".into(),
                vec![
                    ("root".into(), Value::Obj(root)),
                    ("child".into(), Value::Obj(child)),
                ],
            )
            .unwrap();
        let delegate = vm
            .heap
            .alloc_instance(
                "LTestDelegate;".into(),
                vec![("provider".into(), Value::Obj(provider))],
            )
            .unwrap();
        vm.view_props
            .insert((host, "AccessibilityDelegate".into()), Value::Obj(delegate));
        vm.attached_views.insert(host);
        (vm, root, child)
    }
    #[test]
    fn real_dex_provider_traversal_names_nodes_and_deduplicates_cycles() {
        let (mut vm, _, _) = fixture();
        let nodes = vm.virtual_semantics().unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].virtual_id, 7);
        assert_eq!(nodes[0].description, "Named icon");
        assert_eq!(nodes[0].bounds, [0, 0, 20, 30]);
        assert!(vm.fuel_used() < 50);
    }
    #[test]
    fn oversized_child_list_reports_a_structured_limit() {
        let (mut vm, root, _) = fixture();
        put(&mut vm, root, "ChildCount", Value::Int(2049)).unwrap();
        let err = vm.virtual_semantics().unwrap_err();
        assert!(matches!(err,VmExit::Error(e) if e.code=="ACCESSIBILITY_LIMIT"));
    }
}
