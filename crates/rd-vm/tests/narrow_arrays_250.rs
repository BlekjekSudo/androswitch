//! Issue #250: arrays primitivos estreitos (`[Z`/`[B`/`[C`/`[S`) não podem
//! guardar valores fora da faixa do tipo.
//! - `filled-new-array` com componente primitivo diferente de `int` lança a
//!   mesma exceção do ART (DoFilledNewArray). Antes copiava os registradores
//!   crus para o array.
//! - `fill-array-data` em `boolean[]` guarda 0|1, a mesma invariante do
//!   aput-boolean. Antes, com extensão de sinal, o payload 0xFF virava −1.
mod common;
use common::*;
use rd_vm::err::VmExit;
use rd_vm::value::Value;

fn array_payload(width: u16, count: u32, data_units: &[u16]) -> Vec<u16> {
    let mut v = vec![0x0300u16, width, count as u16, (count >> 16) as u16];
    v.extend_from_slice(data_units);
    v
}

/// `static int f()`: v0 = 300, v1 = −1; filled-new-array {v0, v1}, `desc`;
/// move-result-object v2; aget-<kind> v3, v2[índice 0]; return v3.
fn filled_case(desc: &str, aget: u8) -> rd_vm::engine::Engine {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let t = b.type_idx(desc);
    b.direct(
        cls,
        "f",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(5, 0, 0, {
            let mut u = op21s(0x13, 0, 300); // const/16 v0, 300
            u.extend(op11n(0x12, 1, -1)); // const/4 v1, -1
            u.extend(op35c(0x24, 2, t, [0, 1, 0, 0, 0])); // filled-new-array {v0, v1}
            u.extend(op11x(0x0C, 2)); // move-result-object v2
            u.extend(op11n(0x12, 4, 0)); // v4 = 0
            u.extend(op23x(aget, 3, 2, 4)); // aget v3, v2, v4
            u.extend(op11x(0x0F, 3)); // return v3
            u
        })),
    );
    engine_of(&b)
}

#[test]
fn filled_new_array_int_still_works() {
    let mut e = filled_case("[I", 0x44);
    assert_eq!(
        invoke(&mut e, "f", "()I", &[]).unwrap().as_int().unwrap(),
        300
    );
}

#[test]
fn filled_new_array_narrow_primitives_throw_internal_error_like_art() {
    for (desc, aget, pretty) in [
        ("[Z", 0x47, "boolean"),
        ("[B", 0x48, "byte"),
        ("[C", 0x49, "char"),
        ("[S", 0x4A, "short"),
        ("[F", 0x44, "float"),
    ] {
        let mut e = filled_case(desc, aget);
        match invoke(&mut e, "f", "()I", &[]) {
            Err(VmExit::Exception(t)) => {
                assert_eq!(t.class, "Ljava/lang/InternalError;", "{desc}");
                assert_eq!(
                    t.message.as_deref(),
                    Some(
                        format!(
                            "Found type {pretty}; filled-new-array not implemented for anything but 'int'"
                        )
                        .as_str()
                    ),
                    "{desc}"
                );
            }
            other => panic!("{desc}: esperava InternalError, veio {other:?}"),
        }
    }
}

#[test]
fn filled_new_array_wide_primitives_throw_runtime_exception_like_art() {
    for (desc, pretty) in [("[J", "long"), ("[D", "double")] {
        let mut e = filled_case(desc, 0x44);
        match invoke(&mut e, "f", "()I", &[]) {
            Err(VmExit::Exception(t)) => {
                assert_eq!(t.class, "Ljava/lang/RuntimeException;", "{desc}");
                assert_eq!(
                    t.message.as_deref(),
                    Some(format!("Bad filled array request for type {pretty}").as_str()),
                    "{desc}"
                );
            }
            other => panic!("{desc}: esperava RuntimeException, veio {other:?}"),
        }
    }
}

/// `static int g(int i)`: new-array v0 = new <desc>[4]; fill-array-data com
/// largura 1 e bytes FF 00 02 80; aget-<kind> v1 = v0[i]; return v1.
fn fill_case(desc: &str, aget: u8) -> rd_vm::engine::Engine {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let t = b.type_idx(desc);
    b.direct(
        cls,
        "g",
        "I",
        vec!["I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(5, 1, 0, {
            let mut u = op11n(0x12, 0, 4); // v0 = 4
            u.extend(op22c(0x23, 0, 0, t)); // new-array v0, v0, desc
            u.extend(op31t(0x26, 0, 3)); // fill-array-data v0, +3
                                         // bytes em ordem little-endian: FF 00 | 02 80
            u.extend(array_payload(1, 4, &[0x00FF, 0x8002]));
            u.extend(op23x(aget, 1, 0, 4)); // aget v1, v0, v4 (arg)
            u.extend(op11x(0x0F, 1)); // return v1
            u
        })),
    );
    engine_of(&b)
}

fn g(e: &mut rd_vm::engine::Engine, i: i32) -> i32 {
    invoke(e, "g", "(I)I", &[Value::Int(i)])
        .unwrap()
        .as_int()
        .unwrap()
}

#[test]
fn fill_array_data_boolean_normalizes_to_zero_or_one() {
    let mut e = fill_case("[Z", 0x47); // aget-boolean
    assert_eq!(
        [g(&mut e, 0), g(&mut e, 1), g(&mut e, 2), g(&mut e, 3)],
        [1, 0, 1, 1],
        "boolean[] só guarda 0|1 (0xFF não pode virar −1)"
    );
}

#[test]
fn fill_array_data_byte_keeps_sign_extension() {
    let mut e = fill_case("[B", 0x48); // aget-byte
    assert_eq!(
        [g(&mut e, 0), g(&mut e, 1), g(&mut e, 2), g(&mut e, 3)],
        [-1, 0, 2, -128],
        "byte[] continua com sinal"
    );
}
