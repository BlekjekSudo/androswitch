//! `android.graphics.ColorSpace`: the named colour spaces
//! (`ColorSpace.get(ColorSpace.Named)`), as Compose and image code query them.
//!
//! - **Identity.** Each named space is a cached singleton, so identity
//!   equality holds as on Android. `getId` equals the `Named` ordinal (the
//!   AOSP numbering); `getName`, `isSrgb`, `isWideGamut`, `getModel`,
//!   `getComponentCount` and `getMinValue`/`getMaxValue` follow AOSP.
//! - **RGB spaces** (`ColorSpace$Rgb`) expose `getWhitePoint`,
//!   `getPrimaries`, `getTransform` (RGB→XYZ, column-major 3×3, computed
//!   from primaries and white point as AOSP does) and
//!   `getTransferParameters` (ICC parametric curve a..g).
//! - **Not modelled.** `getOetf`/`getEotf` return null: apps get the curve
//!   from the parameters. Building a custom `ColorSpace$Rgb(...)` is not
//!   modelled either.

use crate::engine::Engine;
use crate::err::VmExit;
use crate::heap::{ElemKind, ObjRef};
#[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
#[allow(unused_imports)] // A single explicit alloc prelude for inherited modules.
use crate::port_prelude::*;
use crate::value::Value;

pub(crate) const CS: &str = "Landroid/graphics/ColorSpace;";
pub(crate) const RGB: &str = "Landroid/graphics/ColorSpace$Rgb;";
pub(crate) const NAMED: &str = "Landroid/graphics/ColorSpace$Named;";
const TP: &str = "Landroid/graphics/ColorSpace$Rgb$TransferParameters;";

/// AOSP `ColorSpace.Named` order (ids).
pub(crate) const NAMES: &[&str] = &[
    "SRGB",
    "LINEAR_SRGB",
    "EXTENDED_SRGB",
    "LINEAR_EXTENDED_SRGB",
    "BT709",
    "BT2020",
    "DCI_P3",
    "DISPLAY_P3",
    "NTSC_1953",
    "SMPTE_C",
    "ADOBE_RGB",
    "PRO_PHOTO_RGB",
    "ACES",
    "ACESCG",
    "CIE_XYZ",
    "CIE_LAB",
    "BT2020_HLG",
    "BT2020_PQ",
];
pub(crate) const MODELS: &[&str] = &["RGB", "XYZ", "LAB", "CMYK"];

const D65: [f32; 2] = [0.31271, 0.32902];
const D50: [f32; 2] = [0.34567, 0.35850];
const ACES_WP: [f32; 2] = [0.32168, 0.33767];
const DCI_WP: [f32; 2] = [0.314, 0.351];
const C_WP: [f32; 2] = [0.310, 0.316];

const SRGB_P: [f32; 6] = [0.640, 0.330, 0.300, 0.600, 0.150, 0.060];
const BT2020_P: [f32; 6] = [0.708, 0.292, 0.170, 0.797, 0.131, 0.046];
const P3_P: [f32; 6] = [0.680, 0.320, 0.265, 0.690, 0.150, 0.060];

/// sRGB transfer function (also used by Display P3), as ICC parametric
/// `(a, b, c, d, e, f, g)`.
const SRGB_TF: [f64; 7] = [
    1.0 / 1.055,
    0.055 / 1.055,
    1.0 / 12.92,
    0.04045,
    0.0,
    0.0,
    2.4,
];
const BT709_TF: [f64; 7] = [
    1.0 / 1.099,
    0.099 / 1.099,
    1.0 / 4.5,
    0.081,
    0.0,
    0.0,
    1.0 / 0.45,
];
const LINEAR_TF: [f64; 7] = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];

struct Spec {
    primaries: [f32; 6],
    white: [f32; 2],
    tf: [f64; 7],
    min: f32,
    max: f32,
}

fn spec(id: usize) -> Option<Spec> {
    let s = |primaries, white, tf| Spec {
        primaries,
        white,
        tf,
        min: 0.0,
        max: 1.0,
    };
    Some(match NAMES.get(id).copied()? {
        "SRGB" => s(SRGB_P, D65, SRGB_TF),
        "LINEAR_SRGB" => s(SRGB_P, D65, LINEAR_TF),
        "EXTENDED_SRGB" => Spec {
            min: -0.799,
            max: 2.399,
            ..s(SRGB_P, D65, SRGB_TF)
        },
        "LINEAR_EXTENDED_SRGB" => Spec {
            min: -0.5,
            max: 7.499,
            ..s(SRGB_P, D65, LINEAR_TF)
        },
        "BT709" => s(SRGB_P, D65, BT709_TF),
        "BT2020" | "BT2020_HLG" | "BT2020_PQ" => s(
            BT2020_P,
            D65,
            [
                1.0 / 1.0993,
                0.0993 / 1.0993,
                1.0 / 4.5,
                0.08145,
                0.0,
                0.0,
                1.0 / 0.45,
            ],
        ),
        "DCI_P3" => s(P3_P, DCI_WP, [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.6]),
        "DISPLAY_P3" => s(P3_P, D65, SRGB_TF),
        "NTSC_1953" => s([0.67, 0.33, 0.21, 0.71, 0.14, 0.08], C_WP, BT709_TF),
        "SMPTE_C" => s([0.630, 0.340, 0.310, 0.595, 0.155, 0.070], D65, BT709_TF),
        "ADOBE_RGB" => s(
            [0.64, 0.33, 0.21, 0.71, 0.15, 0.06],
            D65,
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.2],
        ),
        "PRO_PHOTO_RGB" => s(
            [0.7347, 0.2653, 0.1596, 0.8404, 0.0366, 0.0001],
            D50,
            [1.0, 0.0, 1.0 / 16.0, 0.031248, 0.0, 0.0, 1.8],
        ),
        "ACES" => Spec {
            min: -65504.0,
            max: 65504.0,
            ..s(
                [0.73470, 0.26530, 0.0, 1.0, 0.00010, -0.0770],
                ACES_WP,
                LINEAR_TF,
            )
        },
        "ACESCG" => Spec {
            min: -65504.0,
            max: 65504.0,
            ..s(
                [0.713, 0.293, 0.165, 0.830, 0.128, 0.044],
                ACES_WP,
                LINEAR_TF,
            )
        },
        _ => return None, // CIE_XYZ / CIE_LAB: not RGB
    })
}

/// RGB→XYZ matrix (column-major), from xy primaries and white point.
pub(crate) fn rgb_to_xyz(p: &[f32; 6], w: &[f32; 2]) -> [f32; 9] {
    let xyz = |x: f32, y: f32| [x / y, 1.0, (1.0 - x - y) / y];
    let (r, g, b, wt) = (
        xyz(p[0], p[1]),
        xyz(p[2], p[3]),
        xyz(p[4], p[5]),
        xyz(w[0], w[1]),
    );
    // solve [r g b] * S = wt
    let m = [[r[0], g[0], b[0]], [r[1], g[1], b[1]], [r[2], g[2], b[2]]];
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let inv = |i: usize, j: usize| {
        let (a, b) = ((i + 1) % 3, (i + 2) % 3);
        let (c, d) = ((j + 1) % 3, (j + 2) % 3);
        (m[c][a] * m[d][b] - m[c][b] * m[d][a]) / det
    };
    let mut s = [0f32; 3];
    for (i, si) in s.iter_mut().enumerate() {
        *si = (0..3).map(|j| inv(i, j) * wt[j]).sum();
    }
    [
        r[0] * s[0],
        r[1] * s[0],
        r[2] * s[0],
        g[0] * s[1],
        g[1] * s[1],
        g[2] * s[1],
        b[0] * s[2],
        b[1] * s[2],
        b[2] * s[2],
    ]
}

fn float_array(vm: &mut Engine, vals: &[f32]) -> Result<Value, VmExit> {
    let a = vm.heap.alloc_array(ElemKind::Float, vals.len(), "F")?;
    *vm.heap.array_elems_mut(a)? = vals.iter().map(|v| Value::Float(*v)).collect();
    Ok(Value::Obj(a))
}

fn enum_obj(vm: &mut Engine, class: &str, names: &[&str], i: usize) -> Result<Value, VmExit> {
    let k = (class.to_string(), names[i].to_string());
    if let Some(v) = vm.statics.get(&k) {
        return Ok(v.clone());
    }
    let r = vm.heap.alloc_instance(class.to_string(), Vec::new())?;
    let name = Value::Obj(crate::intrinsics::alloc_string(vm, names[i].to_string())?);
    crate::err::put_field_result(vm.heap.put_field(r, "rd_ord", Value::Int(i as i32)))?;
    crate::err::put_field_result(vm.heap.put_field(r, "rd_name", name))?;
    vm.statics.insert(k, Value::Obj(r));
    Ok(Value::Obj(r))
}

/// `sget ColorSpace$Named.X` / `ColorSpace$Model.X`.
pub(crate) fn host_static(
    vm: &mut Engine,
    owner: &str,
    field: &str,
) -> Result<Option<Value>, VmExit> {
    let names = match owner {
        NAMED => NAMES,
        "Landroid/graphics/ColorSpace$Model;" => MODELS,
        _ => return Ok(None),
    };
    match names.iter().position(|n| *n == field) {
        Some(i) => Ok(Some(enum_obj(vm, owner, names, i)?)),
        None => Ok(None),
    }
}

fn get(vm: &mut Engine, id: usize) -> Result<Value, VmExit> {
    let k = (CS.to_string(), format!("rd_named_{id}"));
    if let Some(v) = vm.statics.get(&k) {
        return Ok(v.clone());
    }
    let class = if spec(id).is_some() { RGB } else { CS };
    let r = vm.heap.alloc_instance(class.to_string(), Vec::new())?;
    crate::err::put_field_result(vm.heap.put_field(r, "rd_id", Value::Int(id as i32)))?;
    vm.statics.insert(k, Value::Obj(r));
    Ok(Value::Obj(r))
}

pub fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != CS && class != RGB {
        return Ok(None);
    }
    Ok(Some(match name {
        "get" => {
            let id = match args.first() {
                Some(Value::Obj(n)) => match vm.heap.get_field(*n, "rd_ord") {
                    Ok(Value::Int(i)) => i.max(0) as usize,
                    _ => 0,
                },
                _ => {
                    return Err(VmExit::Exception(crate::err::Throwable::new(
                        "Ljava/lang/NullPointerException;",
                        "name == null",
                    )))
                }
            };
            get(vm, id.min(NAMES.len() - 1))?
        }
        _ => return Ok(None),
    }))
}

pub fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if recv_class != CS && recv_class != RGB {
        return Ok(None);
    }
    let id = match vm.heap.get_field(recv, "rd_id") {
        Ok(Value::Int(i)) => i.max(0) as usize,
        _ => return Ok(None),
    };
    let sp = spec(id);
    let lab = NAMES.get(id) == Some(&"CIE_LAB");
    Ok(Some(match name {
        "getId" => Value::Int(id as i32),
        "getName" => {
            let n = NAMES
                .get(id)
                .copied()
                .unwrap_or("Unknown")
                .replace('_', " ");
            Value::Obj(crate::intrinsics::alloc_string(vm, n)?)
        }
        "isSrgb" => Value::Int((id == 0) as i32),
        "isWideGamut" => Value::Int(!matches!(id, 0 | 1 | 4 | 8 | 9) as i32),
        "getComponentCount" => Value::Int(3),
        "getModel" => {
            let m = if sp.is_some() {
                0
            } else if lab {
                2
            } else {
                1
            };
            enum_obj(vm, "Landroid/graphics/ColorSpace$Model;", MODELS, m)?
        }
        "getMinValue" | "getMaxValue" => {
            let comp = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0);
            let max = name == "getMaxValue";
            Value::Float(match &sp {
                Some(s) => {
                    if max {
                        s.max
                    } else {
                        s.min
                    }
                }
                None if lab => match (comp, max) {
                    (0, false) => 0.0,
                    (0, true) => 100.0,
                    (_, false) => -128.0,
                    (_, true) => 128.0,
                },
                None => {
                    if max {
                        2.0
                    } else {
                        -2.0
                    }
                }
            })
        }
        "getWhitePoint" => {
            let w = sp.as_ref().map(|s| s.white).unwrap_or(D50);
            float_array(vm, &w)?
        }
        "getPrimaries" => match &sp {
            Some(s) => float_array(vm, &s.primaries)?,
            None => return Ok(None),
        },
        "getTransform" => match &sp {
            Some(s) => {
                let m = rgb_to_xyz(&s.primaries, &s.white);
                float_array(vm, &m)?
            }
            None => return Ok(None),
        },
        "getTransferParameters" => match &sp {
            Some(s) => {
                let names = ["a", "b", "c", "d", "e", "f", "g"];
                let fields = names
                    .iter()
                    .zip(s.tf.iter())
                    .map(|(n, v)| (n.to_string(), Value::Double(*v)))
                    .collect();
                Value::Obj(vm.heap.alloc_instance(TP.to_string(), fields)?)
            }
            None => Value::Null,
        },
        "getOetf" | "getEotf" => Value::Null,
        "equals" => Value::Int(matches!(args.first(), Some(Value::Obj(o)) if *o == recv) as i32),
        "hashCode" => Value::Int(id as i32 * 31 + 7),
        "toString" => {
            let n = NAMES
                .get(id)
                .copied()
                .unwrap_or("Unknown")
                .replace('_', " ");
            Value::Obj(crate::intrinsics::alloc_string(
                vm,
                format!("{n} (id={id})"),
            )?)
        }
        _ => return Ok(None),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
    #[allow(unused_imports)]
    use crate::port_prelude::*;
    #[test]
    fn srgb_matrix_matches_reference() {
        let m = rgb_to_xyz(&SRGB_P, &D65);
        // reference sRGB → XYZ (D65), column-major
        let r = [
            0.4124, 0.2126, 0.0193, 0.3576, 0.7152, 0.1192, 0.1805, 0.0722, 0.9505,
        ];
        for (a, b) in m.iter().zip(r.iter()) {
            assert!((a - b).abs() < 2e-3, "{m:?}");
        }
        assert_eq!(NAMES[7], "DISPLAY_P3");
    }
}
