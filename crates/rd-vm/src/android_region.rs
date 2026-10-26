//! Rectilinear android.graphics.Region, used by accessibility occlusion.
//! Regions are disjoint rectangles; DIFFERENCE splits rather than reducing to
//! a bounding box, so covered icons do not become visible semantic targets.
use crate::err::{put_field_result, VmExit};
use crate::heap::{ElemKind, ObjRef};
#[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
#[allow(unused_imports)] // A single explicit alloc prelude for inherited modules.
use crate::port_prelude::*;
use crate::{Engine, Value};
type Rect = [i32; 4];
fn int(v: &Value) -> i32 {
    if let Value::Int(v) = v {
        *v
    } else {
        0
    }
}
fn rect(vm: &Engine, r: ObjRef) -> Rect {
    ["left", "top", "right", "bottom"]
        .map(|k| vm.heap.get_field(r, k).map(|v| int(&v)).unwrap_or(0))
}
fn intersection(a: Rect, b: Rect) -> Option<Rect> {
    let r = [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ];
    (r[2] > r[0] && r[3] > r[1]).then_some(r)
}
fn difference(a: Rect, b: Rect) -> Vec<Rect> {
    let Some(i) = intersection(a, b) else {
        return vec![a];
    };
    [
        [a[0], a[1], a[2], i[1]],
        [a[0], i[3], a[2], a[3]],
        [a[0], i[1], i[0], i[3]],
        [i[2], i[1], a[2], i[3]],
    ]
    .into_iter()
    .filter(|r| r[2] > r[0] && r[3] > r[1])
    .collect()
}
fn load(vm: &Engine, r: ObjRef) -> Vec<Rect> {
    if let Ok(Value::Obj(a)) = vm.heap.get_field(r, "rd$rects") {
        if let Ok((v, _)) = vm.heap.as_array_elems(a) {
            return v
                .chunks_exact(4)
                .map(|v| [int(&v[0]), int(&v[1]), int(&v[2]), int(&v[3])])
                .collect();
        }
    }
    vec![]
}
fn store(vm: &mut Engine, r: ObjRef, rects: Vec<Rect>) -> Result<(), VmExit> {
    if rects.len() > 4096 {
        return Err(crate::err::vm_error("REGION_LIMIT", "region exceeds 4096 rectangles").into());
    }
    let vals: Vec<_> = rects.into_iter().flatten().map(Value::Int).collect();
    let a = vm.heap.alloc_array(ElemKind::Int, vals.len(), "[I")?;
    *vm.heap.array_elems_mut(a)? = vals;
    put_field_result(vm.heap.put_field(r, "rd$rects", Value::Obj(a)))
}
fn operand(vm: &Engine, args: &[Value]) -> Vec<Rect> {
    if args.len() >= 4 {
        let r = [int(&args[0]), int(&args[1]), int(&args[2]), int(&args[3])];
        return if r[2] > r[0] && r[3] > r[1] {
            vec![r]
        } else {
            vec![]
        };
    }
    if let Some(Value::Obj(r)) = args.first() {
        if vm.heap.class_of(*r).unwrap_or("") == "Landroid/graphics/Region;" {
            load(vm, *r)
        } else {
            let r = rect(vm, *r);
            if r[2] > r[0] && r[3] > r[1] {
                vec![r]
            } else {
                vec![]
            }
        }
    } else {
        vec![]
    }
}
fn limit() -> VmExit {
    crate::err::vm_error(
        "REGION_LIMIT",
        "region complexity exceeds 4096 rectangles or 1000000 rectangle tests",
    )
    .into()
}
fn subtract(mut a: Vec<Rect>, b: &[Rect]) -> Result<Vec<Rect>, VmExit> {
    let mut work = 0usize;
    for r in b {
        work = work.saturating_add(a.len());
        if work > 1_000_000 {
            return Err(limit());
        }
        a = a
            .into_iter()
            .flat_map(|x| difference(x, *r))
            .take(4097)
            .collect();
        if a.len() > 4096 {
            return Err(limit());
        }
    }
    Ok(a)
}
fn union(a: Vec<Rect>, b: Vec<Rect>) -> Result<Vec<Rect>, VmExit> {
    let mut out = a.clone();
    out.extend(subtract(b, &a)?);
    if out.len() > 4096 {
        return Err(limit());
    }
    Ok(out)
}
fn is_rect(v: &[Rect]) -> bool {
    let area =
        |r: Rect| (i128::from(r[2]) - i128::from(r[0])) * (i128::from(r[3]) - i128::from(r[1]));
    !v.is_empty() && v.iter().copied().map(area).sum::<i128>() == area(bounds(v))
}
fn bounds(v: &[Rect]) -> Rect {
    if v.is_empty() {
        [0; 4]
    } else {
        v.iter()
            .fold([i32::MAX, i32::MAX, i32::MIN, i32::MIN], |a, b| {
                [
                    a[0].min(b[0]),
                    a[1].min(b[1]),
                    a[2].max(b[2]),
                    a[3].max(b[3]),
                ]
            })
    }
}
pub(crate) fn call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != "Landroid/graphics/Region;" {
        return Ok(None);
    }
    let a = load(vm, recv);
    let out = match name {
        "<init>" | "set" | "setEmpty" => {
            let b = if name == "setEmpty" {
                vec![]
            } else {
                operand(vm, args)
            };
            let b: Vec<_> = b
                .into_iter()
                .filter(|r| r[2] > r[0] && r[3] > r[1])
                .collect();
            let nonempty = !b.is_empty();
            store(vm, recv, b)?;
            if name == "set" {
                Value::Int(nonempty as i32)
            } else {
                Value::Null
            }
        }
        "isEmpty" => Value::Int(a.is_empty() as i32),
        "isRect" => Value::Int(is_rect(&a) as i32),
        "isComplex" => Value::Int((!a.is_empty() && !is_rect(&a)) as i32),
        "contains" => {
            let (x, y) = (int(&args[0]), int(&args[1]));
            Value::Int(
                a.iter()
                    .any(|r| x >= r[0] && x < r[2] && y >= r[1] && y < r[3]) as i32,
            )
        }
        "quickReject" => {
            let b = operand(vm, args);
            Value::Int(
                !a.iter()
                    .any(|x| b.iter().any(|y| intersection(*x, *y).is_some()))
                    as i32,
            )
        }
        "getBounds" => {
            let b = bounds(&a);
            let (r, created) = if let Some(Value::Obj(r)) = args.first() {
                (*r, false)
            } else {
                (
                    vm.heap
                        .alloc_instance("Landroid/graphics/Rect;".into(), vec![])?,
                    true,
                )
            };
            for (k, v) in ["left", "top", "right", "bottom"].into_iter().zip(b) {
                put_field_result(vm.heap.put_field(r, k, Value::Int(v)))?;
            }
            if created {
                Value::Obj(r)
            } else {
                Value::Int(!a.is_empty() as i32)
            }
        }
        "op" => {
            let Some(Value::Obj(op)) = args.last() else {
                return Ok(None);
            };
            let ordinal = match vm.heap.get_field(*op, "rd_ord") {
                Ok(Value::Int(i)) => i,
                _ => {
                    return Err(
                        crate::err::vm_error("INVALID_ARG", "Region.Op missing ordinal").into(),
                    )
                }
            };
            let (a, b) = if args.len() == 3 {
                if let Value::Obj(first) = args[0] {
                    (operand(vm, &[Value::Obj(first)]), operand(vm, &args[1..2]))
                } else {
                    (a, operand(vm, &args[..args.len() - 1]))
                }
            } else {
                (a, operand(vm, &args[..args.len() - 1]))
            };
            if a.len().saturating_mul(b.len()) > 1_000_000 {
                return Err(limit());
            }
            let result = match ordinal {
                0 => subtract(a, &b)?,
                1 => a
                    .iter()
                    .flat_map(|x| b.iter().filter_map(|y| intersection(*x, *y)))
                    .take(4097)
                    .collect(),
                2 => union(a, b)?,
                3 => {
                    let left = subtract(a.clone(), &b)?;
                    union(left, subtract(b, &a)?)?
                }
                4 => subtract(b, &a)?,
                5 => b,
                _ => return Err(crate::err::vm_error("INVALID_ARG", "Region.Op ordinal").into()),
            };
            let nonempty = !result.is_empty();
            store(vm, recv, result)?;
            Value::Int(nonempty as i32)
        }
        _ => return Ok(None),
    };
    Ok(Some(out))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
    #[allow(unused_imports)]
    use crate::port_prelude::*;
    #[test]
    fn subtraction_preserves_hole() {
        let r = subtract(vec![[0, 0, 100, 100]], &[[25, 25, 75, 75]]).unwrap();
        assert_eq!(r.len(), 4);
        assert_eq!(
            r.iter().map(|r| (r[2] - r[0]) * (r[3] - r[1])).sum::<i32>(),
            7500
        );
    }
    const REGION: &str = "Landroid/graphics/Region;";
    fn region(vm: &mut Engine, r: Rect) -> ObjRef {
        let out = vm.heap.alloc_instance(REGION.into(), vec![]).unwrap();
        call(vm, REGION, "<init>", out, &r.map(Value::Int)).unwrap();
        out
    }
    #[test]
    fn all_boolean_ops_follow_platform_ordinals_and_half_open_bounds() {
        let mut vm = Engine::new(vec![], Default::default());
        let b = region(&mut vm, [5, 0, 15, 10]);
        for (ordinal, expected) in [
            (0, [1, 0, 0]),
            (1, [0, 1, 0]),
            (2, [1, 1, 1]),
            (3, [1, 0, 1]),
            (4, [0, 0, 1]),
            (5, [0, 1, 1]),
        ] {
            let a = region(&mut vm, [0, 0, 10, 10]);
            let op = vm
                .heap
                .alloc_instance(
                    "Landroid/graphics/Region$Op;".into(),
                    vec![("rd_ord".into(), Value::Int(ordinal))],
                )
                .unwrap();
            call(&mut vm, REGION, "op", a, &[Value::Obj(b), Value::Obj(op)]).unwrap();
            for (x, expected) in [1, 6, 11].into_iter().zip(expected) {
                assert_eq!(
                    call(
                        &mut vm,
                        REGION,
                        "contains",
                        a,
                        &[Value::Int(x), Value::Int(1)]
                    )
                    .unwrap(),
                    Some(Value::Int(expected)),
                    "op {ordinal} x{x}"
                );
            }
            assert_eq!(
                call(
                    &mut vm,
                    REGION,
                    "contains",
                    a,
                    &[Value::Int(15), Value::Int(1)]
                )
                .unwrap(),
                Some(Value::Int(0))
            );
        }
    }
    #[test]
    fn adjacent_union_is_rect_and_invalid_rect_replacement_is_empty() {
        let mut vm = Engine::new(vec![], Default::default());
        let a = region(&mut vm, [0, 0, 10, 10]);
        let b = region(&mut vm, [10, 0, 20, 10]);
        let op = vm
            .heap
            .alloc_instance(
                "Landroid/graphics/Region$Op;".into(),
                vec![("rd_ord".into(), Value::Int(2))],
            )
            .unwrap();
        call(&mut vm, REGION, "op", a, &[Value::Obj(b), Value::Obj(op)]).unwrap();
        assert_eq!(
            call(&mut vm, REGION, "isRect", a, &[]).unwrap(),
            Some(Value::Int(1))
        );
        assert_eq!(
            call(&mut vm, REGION, "isComplex", a, &[]).unwrap(),
            Some(Value::Int(0))
        );
        vm.heap.put_field(op, "rd_ord", Value::Int(5)).unwrap();
        call(
            &mut vm,
            REGION,
            "op",
            a,
            &[
                Value::Int(5),
                Value::Int(5),
                Value::Int(1),
                Value::Int(1),
                Value::Obj(op),
            ],
        )
        .unwrap();
        assert_eq!(
            call(&mut vm, REGION, "isEmpty", a, &[]).unwrap(),
            Some(Value::Int(1))
        );
    }
    #[test]
    fn work_limit_reports_error_before_mutating_region() {
        let mut vm = Engine::new(vec![], Default::default());
        let a = region(&mut vm, [0, 0, 1, 1]);
        let b = region(&mut vm, [2, 0, 3, 1]);
        let v: Vec<_> = (0..4096).map(|i| [i * 2, 0, i * 2 + 1, 1]).collect();
        store(&mut vm, a, v.clone()).unwrap();
        store(&mut vm, b, v.clone()).unwrap();
        let op = vm
            .heap
            .alloc_instance(
                "Landroid/graphics/Region$Op;".into(),
                vec![("rd_ord".into(), Value::Int(1))],
            )
            .unwrap();
        let err = call(&mut vm, REGION, "op", a, &[Value::Obj(b), Value::Obj(op)]).unwrap_err();
        assert!(matches!(err,VmExit::Error(e) if e.code=="REGION_LIMIT"));
        assert_eq!(load(&vm, a), v);
    }
}
