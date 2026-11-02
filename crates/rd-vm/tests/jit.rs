#![cfg(feature = "jit")]
//! Cranelift/interpreter differential tests; no unsafe outside rd-jni.
mod common;
use common::*;
use rd_vm::heap::ElemKind;
use rd_vm::{Engine, Value};

fn engines(op: u8) -> (Engine, Engine) {
    let mut b = DexBuilder::new();
    let c = b.class("LCaso;", "Ljava/lang/Object;");
    let mut code = op23x(op, 0, 1, 2);
    code.extend(op11x(0x0f, 0));
    b.direct(
        c,
        "run",
        "I",
        vec!["I", "I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(3, 2, 0, code)),
    );
    let i = engine_of(&b);
    let mut j = engine_of(&b);
    j.enable_jit().unwrap();
    (i, j)
}

fn op21t(op: u8, a: u8, off: i16) -> Vec<u16> {
    vec![(op as u16) | ((a as u16) << 8), off as u16]
}

fn op31i(op: u8, a: u8, lit: i32) -> Vec<u16> {
    vec![
        (op as u16) | ((a as u16) << 8),
        lit as u16,
        (lit >> 16) as u16,
    ]
}

fn sparse_payload(cases: &[(i32, i32)]) -> Vec<u16> {
    let mut v = vec![0x0200u16, cases.len() as u16];
    for (k, _) in cases {
        v.push(*k as u16);
        v.push((*k >> 16) as u16);
    }
    for (_, t) in cases {
        v.push(*t as u16);
        v.push((*t >> 16) as u16);
    }
    v
}

#[test]
fn native_integer_leaves_match_interpreter_at_edges_and_random_inputs() {
    for op in [0x90, 0x91, 0x92, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a] {
        let (mut i, mut j) = engines(op);
        let mut seed = 0x9282ab34u32;
        for n in 0..1200 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let x = seed as i32;
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let y = match n % 8 {
                0 => -1,
                1 => i32::MIN,
                2 => i32::MAX,
                3 => 0,
                4 => 32,
                5 => -32,
                _ => seed as i32,
            };
            let args = [Value::Int(x), Value::Int(y)];
            let expected = i.invoke_static("LCaso;", "run", "(II)I", &args).unwrap();
            assert_eq!(
                j.invoke_static("LCaso;", "run", "(II)I", &args).unwrap(),
                expected,
                "op={op:x} x={x} y={y}"
            );
        }
        assert_eq!(j.jit_stats().compiled, 1);
        assert!(
            j.jit_stats().native_calls > 1100,
            "op={op:x} {:?}",
            j.jit_stats()
        );
        assert_eq!(
            i.fuel_used(),
            j.fuel_used(),
            "native fuel must match interpreted leaf"
        );
    }
}

#[test]
fn throwing_division_falls_back_and_fuel_is_still_enforced() {
    let (_, mut j) = engines(0x93);
    for _ in 0..40 {
        assert_eq!(
            j.invoke_static("LCaso;", "run", "(II)I", &[Value::Int(6), Value::Int(2)])
                .unwrap(),
            Value::Int(3)
        );
    }
    assert_eq!(j.jit_stats().compiled, 0);
    assert_eq!(j.jit_stats().unsupported, 1);
    assert!(matches!(
        j.invoke_static("LCaso;", "run", "(II)I", &[Value::Int(6), Value::Int(0)]),
        Err(rd_vm::err::VmExit::Exception(_))
    ));
    let (_, mut j) = engines(0x90);
    for _ in 0..40 {
        j.invoke_static("LCaso;", "run", "(II)I", &[Value::Int(1), Value::Int(2)])
            .unwrap();
    }
    assert!(j.jit_stats().native_calls > 0);
    j.config.fuel = j.fuel_used();
    assert!(
        matches!(j.invoke_static("LCaso;","run","(II)I",&[Value::Int(1),Value::Int(2)]),Err(rd_vm::err::VmExit::Error(ref e)) if e.code=="VM_FUEL")
    );
}

#[test]
fn literal_reverse_subtract_and_narrowing_match() {
    for opcode in [0x7b, 0x7c, 0x8d, 0x8e, 0x8f, 0xd1, 0xd9] {
        let mut b = DexBuilder::new();
        let c = b.class("LCaso;", "Ljava/lang/Object;");
        let mut code = if opcode == 0xd9 {
            op22b(opcode, 0, 1, -7)
        } else if opcode == 0xd1 {
            op22s(opcode, 0, 1, -7)
        } else {
            op12x(opcode, 0, 1)
        };
        code.extend(op11x(0x0f, 0));
        b.direct(
            c,
            "run",
            "I",
            vec!["I"],
            ACC_PUBLIC | ACC_STATIC,
            Some(b.code(2, 1, 0, code)),
        );
        let mut i = engine_of(&b);
        let mut j = engine_of(&b);
        j.enable_jit().unwrap();
        for n in 0i32..300 {
            let x = n.wrapping_mul(0x12345);
            let args = [Value::Int(x)];
            assert_eq!(
                j.invoke_static("LCaso;", "run", "(I)I", &args).unwrap(),
                i.invoke_static("LCaso;", "run", "(I)I", &args).unwrap()
            );
        }
        assert_eq!(
            j.jit_stats().compiled,
            1,
            "op={opcode:x} {:?}",
            j.jit_stats()
        );
    }
}

#[test]
fn cfg_loops_collatz_steps_match_interpreter_and_exact_fuel() {
    // Bounded Collatz-like multi-branch loop:
    // v2 = n (arg0), v3 = max_steps (arg1), v0 = steps = 0, v1 = scratch
    // pc 0:  const/4 v0, 0                   (1)
    // pc 1:  if-ge v0, v3, +14 -> pc 15      (2)
    // pc 3:  if-lez v2, +12 -> pc 15         (2)
    // pc 5:  const/4 v1, 1                   (1)
    // pc 6:  if-eq v2, v1, +9 -> pc 15       (2)
    // pc 8:  rem-int/lit8 v1, v2, 2          (2)
    // pc 10: if-nez v1, +4 -> pc 14          (2)
    // pc 12: div-int/lit8 v2, v2, 2          (2)
    // pc 14 (wait: jump over odd):
    let mut b = DexBuilder::new();
    let c = b.class("LLoop;", "Ljava/lang/Object;");
    let mut code = Vec::new();
    code.extend(op11n(0x12, 0, 0)); // pc 0 (len 1): v0 = 0
    code.extend(op22t(0x35, 0, 3, 17)); // pc 1 (len 2): if-ge v0, v3 -> pc 18
    code.extend(op21t(0x3d, 2, 15)); // pc 3 (len 2): if-lez v2 -> pc 18
    code.extend(op11n(0x12, 1, 1)); // pc 5 (len 1): v1 = 1
    code.extend(op22t(0x32, 2, 1, 12)); // pc 6 (len 2): if-eq v2, v1 -> pc 18
    code.extend(op22b(0xdc, 1, 2, 2)); // pc 8 (len 2): rem-int/lit8 v1, v2, 2
    code.extend(op21t(0x39, 1, 4)); // pc 10 (len 2): if-nez v1 -> pc 14 (odd)
    code.extend(op22b(0xdb, 2, 2, 2)); // pc 12 (len 2): div-int/lit8 v2, v2, 2
    code.extend(op10t(0x28, 3)); // pc 14 (len 1) -> WAIT: let's keep pc exact:
                                 // Re-layout cleanly with verified unit offsets:
    code.clear();
    code.extend(op11n(0x12, 0, 0)); // pc 0  (1): v0 = 0
    code.extend(op22t(0x35, 0, 3, 18)); // pc 1  (2): if-ge v0, v3 -> pc 19
    code.extend(op21t(0x3d, 2, 16)); // pc 3  (2): if-lez v2 -> pc 19
    code.extend(op11n(0x12, 1, 1)); // pc 5  (1): v1 = 1
    code.extend(op22t(0x32, 2, 1, 13)); // pc 6  (2): if-eq v2, v1 -> pc 19
    code.extend(op22b(0xdc, 1, 2, 2)); // pc 8  (2): v1 = v2 % 2
    code.extend(op21t(0x39, 1, 4)); // pc 10 (2): if-nez v1 -> pc 14
    code.extend(op22b(0xdb, 2, 2, 2)); // pc 12 (2): v2 = v2 / 2
    code.extend(op10t(0x28, 3)); // pc 14 (1) -> wait: 10 + 4 == 14! Make pc 10 jump +5 -> pc 15!
    code.clear();
    code.extend(op11n(0x12, 0, 0)); // pc 0  (1): v0 = 0
    code.extend(op22t(0x35, 0, 3, 19)); // pc 1  (2): if-ge v0, v3 -> pc 20
    code.extend(op21t(0x3d, 2, 17)); // pc 3  (2): if-lez v2 -> pc 20
    code.extend(op11n(0x12, 1, 1)); // pc 5  (1): v1 = 1
    code.extend(op22t(0x32, 2, 1, 14)); // pc 6  (2): if-eq v2, v1 -> pc 20
    code.extend(op22b(0xdc, 1, 2, 2)); // pc 8  (2): v1 = v2 % 2
    code.extend(op21t(0x39, 1, 5)); // pc 10 (2): if-nez v1 -> pc 15 (odd)
    code.extend(op22b(0xdb, 2, 2, 2)); // pc 12 (2): v2 = v2 / 2
    code.extend(op10t(0x28, 4)); // pc 14 (1): goto +4 -> pc 18
    code.extend(op22b(0xda, 2, 2, 3)); // pc 15 (2): v2 = v2 * 3
    code.extend(op22b(0xd8, 2, 2, 1)); // pc 17 (2) -> wait: 15 + 2 = 17, 17 + 2 = 19!
                                       // Adjust pc 14 goto +5 -> pc 19, and early exits -> pc 22:
    code.clear();
    code.extend(op11n(0x12, 0, 0)); // pc 0  (1): v0 = 0
    code.extend(op22t(0x35, 0, 3, 21)); // pc 1  (2): if-ge v0, v3 -> pc 22
    code.extend(op21t(0x3d, 2, 19)); // pc 3  (2): if-lez v2 -> pc 22
    code.extend(op11n(0x12, 1, 1)); // pc 5  (1): v1 = 1
    code.extend(op22t(0x32, 2, 1, 16)); // pc 6  (2): if-eq v2, v1 -> pc 22
    code.extend(op22b(0xdc, 1, 2, 2)); // pc 8  (2): v1 = v2 % 2
    code.extend(op21t(0x39, 1, 5)); // pc 10 (2): if-nez v1 -> pc 15
    code.extend(op22b(0xdb, 2, 2, 2)); // pc 12 (2): v2 = v2 / 2
    code.extend(op10t(0x28, 5)); // pc 14 (1): goto +5 -> pc 19
    code.extend(op22b(0xda, 2, 2, 3)); // pc 15 (2): v2 = v2 * 3
    code.extend(op22b(0xd8, 2, 2, 1)); // pc 17 (2): v2 = v2 + 1
    code.extend(op22b(0xd8, 0, 0, 1)); // pc 19 (2): v0 = v0 + 1
    code.extend(op10t(0x28, -20)); // pc 21 (1): goto -20 -> pc 1
    code.extend(op11x(0x0f, 0)); // pc 22 (1): return v0
    assert_eq!(code.len(), 23);

    b.direct(
        c,
        "collatz",
        "I",
        vec!["I", "I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(4, 2, 0, code)),
    );
    let mut interp = engine_of(&b);
    let mut jit = engine_of(&b);
    jit.enable_jit().unwrap();

    for n in [-5, 0, 1, 2, 3, 7, 27, 97, 871, 6171] {
        let args = [Value::Int(n), Value::Int(500)];
        let expected = interp
            .invoke_static("LLoop;", "collatz", "(II)I", &args)
            .unwrap();
        let actual = jit
            .invoke_static("LLoop;", "collatz", "(II)I", &args)
            .unwrap();
        assert_eq!(actual, expected, "n={n}");
        assert_eq!(
            jit.fuel_used(),
            interp.fuel_used(),
            "exact loop fuel mismatch at n={n}"
        );
    }
    // Loop-containing method compiles immediately on call #1!
    assert_eq!(jit.jit_stats().compiled, 1);
    assert_eq!(jit.jit_stats().native_calls, 10);
}

#[test]
fn infinite_loop_in_jit_is_deterministically_fuel_bounded() {
    let mut b = DexBuilder::new();
    let c = b.class("LSpin;", "Ljava/lang/Object;");
    let mut code = Vec::new();
    code.extend(op11n(0x12, 0, 0)); // pc 0 (1): v0 = 0
    code.extend(op22b(0xd8, 0, 0, 1)); // pc 1 (2): v0 += 1
    code.extend(op10t(0x28, -2)); // pc 3 (1): goto -2 -> pc 1
    b.direct(
        c,
        "spin",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(1, 0, 0, code)),
    );
    let mut interp = engine_of(&b);
    let mut jit = engine_of(&b);
    jit.enable_jit().unwrap();
    interp.config.fuel = 5_000;
    jit.config.fuel = 5_000;

    let ie = interp.invoke_static("LSpin;", "spin", "()I", &[]);
    let je = jit.invoke_static("LSpin;", "spin", "()I", &[]);
    assert!(matches!(ie, Err(rd_vm::err::VmExit::Error(ref e)) if e.code == "VM_FUEL"));
    assert!(matches!(je, Err(rd_vm::err::VmExit::Error(ref e)) if e.code == "VM_FUEL"));
    assert_eq!(jit.jit_stats().compiled, 1);
    assert_eq!(jit.fuel_used(), interp.fuel_used());
}

#[test]
fn wide_64bit_long_alu_shifts_conversions_and_cmp_long_match_interpreter() {
    // Mixes two `long` inputs + an `int` shift amount + `int` loop count:
    // sig `(JJII)J` -> ins_size = 2 + 2 + 1 + 1 = 6; regs = 10:
    //   v4..v5 = a (J), v6..v7 = b (J), v8 = shift (I), v9 = rounds (I)
    //   v0..v1 = acc (J), v2..v3 = tmp (J)
    let mut builder = DexBuilder::new();
    let c = builder.class("LLong;", "Ljava/lang/Object;");
    let mut code = Vec::new();
    code.extend(op51l(0x18, 0, 0x9e37_79b9_7f4a_7c15u64 as i64)); // pc 0  (5): const-wide v0, golden
    code.extend(op12x(0xbb, 0, 4)); // pc 5  (1): add-long/2addr v0, v4
    code.extend(op21t(0x3d, 9, 16)); // pc 6  (2): if-lez v9 -> pc 22
    code.extend(op23x(0x9c, 2, 0, 6)); // pc 8  (2): sub-long v2, v0, v6
    code.extend(op23x(0xa3, 2, 2, 8)); // pc 10 (2): shl-long v2, v2, v8
    code.extend(op12x(0xc2, 0, 2)); // pc 12 (1): xor-long/2addr v0, v2
    code.extend(op23x(0xa5, 2, 0, 8)); // pc 13 (2): ushr-long v2, v0, v8
    code.extend(op12x(0xbd, 0, 6)); // pc 15 (1): mul-long/2addr v0, v6
    code.extend(op12x(0xbb, 0, 2)); // pc 16 (1): add-long/2addr v0, v2
    code.extend(op12x(0x7e, 0, 0)); // pc 17 (1): not-long v0, v0
    code.extend(op12x(0x7d, 0, 0)); // pc 18 (1): neg-long v0, v0
    code.extend(op22b(0xd8, 9, 9, -1)); // pc 19 (2): v9 -= 1
    code.extend(op10t(0x28, -15)); // pc 21 (1): goto -15 -> pc 6
                                   // Post-loop: exercise cmp-long (0x31), int-to-long (0x81), long-to-int (0x84), shr-long/2addr (0xc4)
    code.extend(op23x(0x31, 8, 0, 4)); // pc 22 (2): cmp-long v8, v0, v4 (-1/0/+1)
    code.extend(op12x(0x81, 2, 8)); // pc 24 (1): int-to-long v2, v8
    code.extend(op12x(0xbb, 0, 2)); // pc 25 (1): add-long/2addr v0, v2
    code.extend(op12x(0x84, 8, 0)); // pc 26 (1): long-to-int v8, v0
    code.extend(op12x(0xc4, 0, 8)); // pc 27 (1): shr-long/2addr v0, v8
    code.extend(op11x(0x10, 0)); // pc 28 (1): return-wide v0
    assert_eq!(code.len(), 29);

    builder.direct(
        c,
        "mix",
        "J",
        vec!["J", "J", "I", "I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(builder.code(10, 6, 0, code)),
    );
    let mut interp = engine_of(&builder);
    let mut jit = engine_of(&builder);
    jit.enable_jit().unwrap();

    let edge_longs = [0i64, 1, -1, i64::MIN, i64::MAX, 0x1234_5678_9abc_def0];
    let shifts = [0i32, 1, 31, 32, 63, 64, -1, -65, 13];
    for &a in &edge_longs {
        for &s in &shifts {
            let b_val = a.wrapping_mul(6364136223846793005).wrapping_add(1);
            let args = [
                Value::Long(a),
                Value::Long(b_val),
                Value::Int(s),
                Value::Int(12),
            ];
            let exp = interp
                .invoke_static("LLong;", "mix", "(JJII)J", &args)
                .unwrap();
            let got = jit
                .invoke_static("LLong;", "mix", "(JJII)J", &args)
                .unwrap();
            assert_eq!(got, exp, "a={a} b={b_val} s={s}");
        }
    }
    assert_eq!(jit.jit_stats().compiled, 1);
    assert_eq!(jit.fuel_used(), interp.fuel_used());
}

#[test]
fn packed_and_sparse_switch_with_negative_keys_match_interpreter() {
    let mut b = DexBuilder::new();
    let c = b.class("LSwitch;", "Ljava/lang/Object;");

    // Method 1: packed-switch with `first_key = -1` and 3 targets + default
    // pc 0 (3): packed-switch v1, +11 -> payload at pc 11
    // pc 3 (2): const/16 v0, 999   (default)
    // pc 5 (1): return v0
    // pc 6 (2): const/16 v0, 10    (key -1)
    // pc 8 (1): return v0
    // pc 9 (1): const/4 v0, 7      (key 0 & key 1)
    // pc 10 (1): return v0
    // pc 11 (10): packed-switch-payload first=-1, targets=[6, 9, 9]
    let mut pcode = Vec::new();
    pcode.extend(op31t(0x2b, 1, 11)); // pc 0 (3)
    pcode.extend(op21s(0x13, 0, 999)); // pc 3 (2)
    pcode.extend(op11x(0x0f, 0)); // pc 5 (1)
    pcode.extend(op21s(0x13, 0, 10)); // pc 6 (2)
    pcode.extend(op11x(0x0f, 0)); // pc 8 (1)
    pcode.extend(op11n(0x12, 0, 7)); // pc 9 (1)
    pcode.extend(op11x(0x0f, 0)); // pc 10 (1)
    pcode.extend(packed_payload(-1, &[6, 9, 9])); // pc 11

    // Method 2: sparse-switch with negative, zero, and large positive keys
    // pc 0 (3): sparse-switch v1, +12 -> payload at pc 12
    // pc 3 (2): const/16 v0, -55
    // pc 5 (1): return v0
    // pc 6 (2): const/16 v0, 111
    // pc 8 (1): return v0
    // pc 9 (2): const/16 v0, 222
    // pc 11 (1): return v0
    // pc 12: sparse-switch-payload [(-1000, 6), (0, 9), (50000, 6)]
    let mut scode = Vec::new();
    scode.extend(op31t(0x2c, 1, 12)); // pc 0 (3)
    scode.extend(op21s(0x13, 0, -55)); // pc 3 (2)
    scode.extend(op11x(0x0f, 0)); // pc 5 (1)
    scode.extend(op21s(0x13, 0, 111)); // pc 6 (2)
    scode.extend(op11x(0x0f, 0)); // pc 8 (1)
    scode.extend(op21s(0x13, 0, 222)); // pc 9 (2)
    scode.extend(op11x(0x0f, 0)); // pc 11 (1)
    scode.extend(sparse_payload(&[(-1000, 6), (0, 9), (50000, 6)])); // pc 12

    // DexBuilder requires methods in ascending name order within directs: "packed" < "sparse".
    b.direct(
        c,
        "packed",
        "I",
        vec!["I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(2, 1, 0, pcode)),
    );
    b.direct(
        c,
        "sparse",
        "I",
        vec!["I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(2, 1, 0, scode)),
    );

    let mut interp = engine_of(&b);
    let mut jit = engine_of(&b);
    jit.enable_jit().unwrap();

    let keys = [i32::MIN, -1000, -2, -1, 0, 1, 2, 50000, 50001, i32::MAX];
    for _round in 0..8 {
        for &key in &keys {
            let args = [Value::Int(key)];
            assert_eq!(
                jit.invoke_static("LSwitch;", "packed", "(I)I", &args)
                    .unwrap(),
                interp
                    .invoke_static("LSwitch;", "packed", "(I)I", &args)
                    .unwrap(),
                "packed k={key}"
            );
            assert_eq!(
                jit.invoke_static("LSwitch;", "sparse", "(I)I", &args)
                    .unwrap(),
                interp
                    .invoke_static("LSwitch;", "sparse", "(I)I", &args)
                    .unwrap(),
                "sparse k={key}"
            );
        }
    }
    assert_eq!(jit.jit_stats().compiled, 2);
    assert_eq!(jit.fuel_used(), interp.fuel_used());
}

#[test]
fn literal_nonzero_div_and_rem_including_min_div_minus_one_match_interpreter() {
    for &lit in &[-1i16, 1, -7, 7, -32768, 32767] {
        for &(opcode, use_lit8) in &[(0xd3u8, false), (0xd4, false), (0xdb, true), (0xdc, true)] {
            if use_lit8 && i8::try_from(lit).is_err() {
                continue;
            }
            let mut b = DexBuilder::new();
            let c = b.class("LDiv;", "Ljava/lang/Object;");
            let mut code = if use_lit8 {
                op22b(opcode, 0, 1, lit as i8)
            } else {
                op22s(opcode, 0, 1, lit)
            };
            code.extend(op11x(0x0f, 0));
            b.direct(
                c,
                "calc",
                "I",
                vec!["I"],
                ACC_PUBLIC | ACC_STATIC,
                Some(b.code(2, 1, 0, code)),
            );
            let mut interp = engine_of(&b);
            let mut jit = engine_of(&b);
            jit.enable_jit().unwrap();
            let specials = [i32::MIN, i32::MIN + 1, -1, 0, 1, i32::MAX - 1, i32::MAX];
            for step in 0..60 {
                let x = if step < specials.len() {
                    specials[step]
                } else {
                    (step as i32).wrapping_mul(-1_048_583)
                };
                let args = [Value::Int(x)];
                assert_eq!(
                    jit.invoke_static("LDiv;", "calc", "(I)I", &args).unwrap(),
                    interp
                        .invoke_static("LDiv;", "calc", "(I)I", &args)
                        .unwrap(),
                    "op=0x{opcode:02x} lit={lit} x={x}"
                );
            }
            assert_eq!(jit.jit_stats().compiled, 1);
            assert_eq!(jit.fuel_used(), interp.fuel_used());
        }
    }
}

#[test]
fn primitive_array_crc_aliasing_and_transactional_exception_rollback_match_interpreter() {
    // `mutateAndSum([I[II)I`:
    // Loops `i = 0 .. limit-1`:
    //   v = a[i] ^ 0x5a5a
    //   b[i] = (short)v   (`aput-short` 0x51 to exercise narrowing writeback + aliasing!)
    //   sum += b[i]
    // Returns `sum + a.length`.
    // Registers: v0=sum, v1=i, v2=tmp, v3=a([I), v4=b([I), v5=limit(I)
    let mut b = DexBuilder::new();
    let c = b.class("LArr;", "Ljava/lang/Object;");
    let mut code = Vec::new();
    code.extend(op21s(0x13, 0, 0)); // pc 0  (2): v0 = 0
    code.extend(op21s(0x13, 1, 0)); // pc 2  (2): v1 = 0
    code.extend(op22t(0x35, 1, 5, 11)); // pc 4  (2): if-ge v1, v5 -> pc 15
    code.extend(op23x(0x44, 2, 3, 1)); // pc 6  (2): aget v2, v3, v1
    code.extend(op22s(0xd7, 2, 2, 0x5a5a)); // pc 8  (2): xor-int/lit16 v2, v2, 0x5a5a
    code.extend(op23x(0x51, 2, 4, 1)); // pc 10 (2): aput-short v2, v4, v1
    code.extend(op23x(0x4a, 2, 4, 1)); // pc 12 (2) -> wait: 10+2=12, 12+2=14, +inc(2)=16, +goto(1)=17
                                       // Re-compute exact branch offsets:
    code.clear();
    code.extend(op21s(0x13, 0, 0)); // pc 0  (2): sum = 0
    code.extend(op21s(0x13, 1, 0)); // pc 2  (2): i = 0
    code.extend(op22t(0x35, 1, 5, 14)); // pc 4  (2): if-ge i, limit -> pc 18
    code.extend(op23x(0x44, 2, 3, 1)); // pc 6  (2): aget v2, v3, v1
    code.extend(op22s(0xd7, 2, 2, 0x5a5a)); // pc 8  (2): xor-int/lit16 v2, v2, 0x5a5a
    code.extend(op23x(0x51, 2, 4, 1)); // pc 10 (2): aput-short v2, v4, v1
    code.extend(op23x(0x4a, 2, 4, 1)); // pc 12 (2): aget-short v2, v4, v1
    code.extend(op12x(0xb0, 0, 2)); // pc 14 (1): add-int/2addr v0, v2
    code.extend(op22b(0xd8, 1, 1, 1)); // pc 15 (2): i += 1
    code.extend(op10t(0x28, -13)); // pc 17 (1): goto -13 -> pc 4
    code.extend(op12x(0x21, 2, 3)); // pc 18 (1): array-length v2, v3
    code.extend(op12x(0xb0, 0, 2)); // pc 19 (1): add-int/2addr v0, v2
    code.extend(op11x(0x0f, 0)); // pc 20 (1): return v0
    assert_eq!(code.len(), 21);

    b.direct(
        c,
        "mutateAndSum",
        "I",
        vec!["[I", "[I", "I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(6, 3, 0, code)),
    );

    let mut interp = engine_of(&b);
    let mut jit = engine_of(&b);
    jit.enable_jit().unwrap();

    let alloc_int_arr = |vm: &mut Engine, vals: &[i32]| -> u32 {
        let r = vm
            .heap
            .alloc_array(ElemKind::Int, vals.len(), "[I")
            .unwrap();
        let elems = vm.heap.array_elems_mut(r).unwrap();
        elems.extend(vals.iter().copied().map(Value::Int));
        r
    };

    // 1. Aliased array call: `mutateAndSum(arr, arr, 5)` — reads & writes the SAME array!
    let seed = [100_000i32, -77_777, 0, 32_767, -32_768];
    let ia = alloc_int_arr(&mut interp, &seed);
    let ja = alloc_int_arr(&mut jit, &seed);
    let iret = interp
        .invoke_static(
            "LArr;",
            "mutateAndSum",
            "([I[II)I",
            &[Value::Obj(ia), Value::Obj(ia), Value::Int(5)],
        )
        .unwrap();
    let jret = jit
        .invoke_static(
            "LArr;",
            "mutateAndSum",
            "([I[II)I",
            &[Value::Obj(ja), Value::Obj(ja), Value::Int(5)],
        )
        .unwrap();
    assert_eq!(jret, iret);
    assert_eq!(jit.jit_stats().compiled, 1);
    assert_eq!(jit.jit_stats().native_calls, 1);
    assert_eq!(
        jit.heap.as_array_elems(ja).unwrap().0,
        interp.heap.as_array_elems(ia).unwrap().0
    );
    assert_eq!(jit.fuel_used(), interp.fuel_used());

    // 2. Out-of-bounds `limit = 6` on length-4 array: JIT rolls back scratch and
    //    interpreter applies partial writes `0..3` then throws exact `AIOOBE`!
    let ib = alloc_int_arr(&mut interp, &[1, 2, 3, 4]);
    let jb = alloc_int_arr(&mut jit, &[1, 2, 3, 4]);
    let ierr = interp.invoke_static(
        "LArr;",
        "mutateAndSum",
        "([I[II)I",
        &[Value::Obj(ib), Value::Obj(ib), Value::Int(6)],
    );
    let jerr = jit.invoke_static(
        "LArr;",
        "mutateAndSum",
        "([I[II)I",
        &[Value::Obj(jb), Value::Obj(jb), Value::Int(6)],
    );
    assert!(
        matches!(ierr, Err(rd_vm::err::VmExit::Exception(ref t)) if t.class.contains("ArrayIndexOutOfBoundsException"))
    );
    assert!(
        matches!(jerr, Err(rd_vm::err::VmExit::Exception(ref t)) if t.class.contains("ArrayIndexOutOfBoundsException"))
    );
    assert_eq!(
        jit.heap.as_array_elems(jb).unwrap().0,
        interp.heap.as_array_elems(ib).unwrap().0,
        "partial array writes before AIOOBE must match interpreter identically"
    );
    assert_eq!(jit.fuel_used(), interp.fuel_used());

    // 3. Null array throws exact `NullPointerException` with matching fuel.
    let inpe = interp.invoke_static(
        "LArr;",
        "mutateAndSum",
        "([I[II)I",
        &[Value::Null, Value::Null, Value::Int(1)],
    );
    let jnpe = jit.invoke_static(
        "LArr;",
        "mutateAndSum",
        "([I[II)I",
        &[Value::Null, Value::Null, Value::Int(1)],
    );
    assert!(
        matches!(inpe, Err(rd_vm::err::VmExit::Exception(ref t)) if t.class.contains("NullPointerException"))
    );
    assert!(
        matches!(jnpe, Err(rd_vm::err::VmExit::Exception(ref t)) if t.class.contains("NullPointerException"))
    );
    // 4. IEEE CRC32 inner polynomial step (`const v3, 0xEDB88320` via `op31i` + `[B` array):
    let mut cb = DexBuilder::new();
    let cc = cb.class("LCrc;", "Ljava/lang/Object;");
    let mut crc_code = Vec::new();
    // crc32([B)I: v0=crc(-1), v1=i(0), v2=len, v3=poly(0xEDB88320), v4=bit, v5=tmp, v6=buf([B)
    crc_code.extend(op11n(0x12, 0, -1)); // pc 0  (1): crc = -1
    crc_code.extend(op11n(0x12, 1, 0)); // pc 1  (1): i = 0
    crc_code.extend(op12x(0x21, 2, 6)); // pc 2  (1): len = buf.length
    crc_code.extend(op31i(0x14, 3, 0xEDB8_8320u32 as i32)); // pc 3  (3): poly = 0xEDB88320
    crc_code.extend(op22t(0x35, 1, 2, 19)); // pc 6  (2): if-ge i, len -> pc 25
    crc_code.extend(op23x(0x48, 5, 6, 1)); // pc 8  (2): aget-byte v5, buf, i
    crc_code.extend(op22s(0xd5, 5, 5, 0xff)); // pc 10 (2): v5 &= 0xff
    crc_code.extend(op12x(0xb7, 0, 5)); // pc 12 (1): crc ^= v5
    crc_code.extend(op21s(0x13, 4, 8)); // pc 13 (2): bit = 8
    crc_code.extend(op21t(0x3d, 4, 7)); // pc 15 (2): if-lez bit -> pc 22
    crc_code.extend(op22b(0xdd, 5, 0, 1)); // pc 17 (2): v5 = crc & 1
    crc_code.extend(op22b(0xe2, 0, 0, 1)); // pc 19 (2): crc >>>= 1
    crc_code.extend(op12x(0x7b, 5, 5)); // pc 21 (1): v5 = -v5 (0 or -1 mask) -> wait: recompute offsets cleanly!
    crc_code.clear();
    crc_code.extend(op11n(0x12, 0, -1)); // pc 0  (1)
    crc_code.extend(op11n(0x12, 1, 0)); // pc 1  (1)
    crc_code.extend(op12x(0x21, 2, 6)); // pc 2  (1)
    crc_code.extend(op31i(0x14, 3, 0xEDB8_8320u32 as i32)); // pc 3  (3)
    crc_code.extend(op22t(0x35, 1, 2, 22)); // pc 6  (2): if-ge i, len -> pc 28
    crc_code.extend(op23x(0x48, 5, 6, 1)); // pc 8  (2): aget-byte v5, buf, i
    crc_code.extend(op22s(0xd5, 5, 5, 0xff)); // pc 10 (2): v5 &= 0xff
    crc_code.extend(op12x(0xb7, 0, 5)); // pc 12 (1): crc ^= v5
    crc_code.extend(op21s(0x13, 4, 8)); // pc 13 (2): bit = 8
    crc_code.extend(op21t(0x3d, 4, 10)); // pc 15 (2): if-lez bit -> pc 25
    crc_code.extend(op22b(0xdd, 5, 0, 1)); // pc 17 (2): v5 = crc & 1
    crc_code.extend(op22b(0xe2, 0, 0, 1)); // pc 19 (2): crc >>>= 1
    crc_code.extend(op12x(0x7b, 5, 5)); // pc 21 (1): v5 = -v5
    crc_code.extend(op12x(0xb5, 5, 3)); // pc 22 (1): v5 &= poly
    crc_code.extend(op12x(0xb7, 0, 5)); // pc 23 (1): crc ^= v5 -> wait: need bit -= 1 + goto!
    crc_code.clear();
    crc_code.extend(op11n(0x12, 0, -1)); // pc 0  (1)
    crc_code.extend(op11n(0x12, 1, 0)); // pc 1  (1)
    crc_code.extend(op12x(0x21, 2, 6)); // pc 2  (1)
    crc_code.extend(op31i(0x14, 3, 0xEDB8_8320u32 as i32)); // pc 3  (3)
    crc_code.extend(op22t(0x35, 1, 2, 24)); // pc 6  (2): if-ge i, len -> pc 30
    crc_code.extend(op23x(0x48, 5, 6, 1)); // pc 8  (2): aget-byte v5, buf, i
    crc_code.extend(op22s(0xd5, 5, 5, 0xff)); // pc 10 (2): v5 &= 0xff
    crc_code.extend(op12x(0xb7, 0, 5)); // pc 12 (1): crc ^= v5
    crc_code.extend(op21s(0x13, 4, 8)); // pc 13 (2): bit = 8
    crc_code.extend(op21t(0x3d, 4, 12)); // pc 15 (2): if-lez bit -> pc 27
    crc_code.extend(op22b(0xdd, 5, 0, 1)); // pc 17 (2): v5 = crc & 1
    crc_code.extend(op22b(0xe2, 0, 0, 1)); // pc 19 (2): crc >>>= 1
    crc_code.extend(op12x(0x7b, 5, 5)); // pc 21 (1): v5 = -v5
    crc_code.extend(op12x(0xb5, 5, 3)); // pc 22 (1): v5 &= poly
    crc_code.extend(op12x(0xb7, 0, 5)); // pc 23 (1): crc ^= v5
    crc_code.extend(op22b(0xd8, 4, 4, -1)); // pc 24 (2): bit -= 1
    crc_code.extend(op10t(0x28, -11)); // pc 26 (1): goto -11 -> pc 15
    crc_code.extend(op22b(0xd8, 1, 1, 1)); // pc 27 (2): i += 1
    crc_code.extend(op10t(0x28, -23)); // pc 29 (1): goto -23 -> pc 6
    crc_code.extend(op12x(0x7c, 0, 0)); // pc 30 (1): crc = ~crc
    crc_code.extend(op11x(0x0f, 0)); // pc 31 (1): return crc
    assert_eq!(crc_code.len(), 32);
    cb.direct(
        cc,
        "crc32",
        "I",
        vec!["[B"],
        ACC_PUBLIC | ACC_STATIC,
        Some(cb.code(7, 1, 0, crc_code)),
    );
    let mut ci = engine_of(&cb);
    let mut cj = engine_of(&cb);
    cj.enable_jit().unwrap();
    let payload: Vec<i32> = b"123456789".iter().map(|&ch| i32::from(ch as i8)).collect();
    let ir_buf = ci
        .heap
        .alloc_array(ElemKind::Int, payload.len(), "[B")
        .unwrap();
    ci.heap
        .array_elems_mut(ir_buf)
        .unwrap()
        .extend(payload.iter().copied().map(Value::Int));
    let jr_buf = cj
        .heap
        .alloc_array(ElemKind::Int, payload.len(), "[B")
        .unwrap();
    cj.heap
        .array_elems_mut(jr_buf)
        .unwrap()
        .extend(payload.iter().copied().map(Value::Int));
    let expected_crc = ci
        .invoke_static("LCrc;", "crc32", "([B)I", &[Value::Obj(ir_buf)])
        .unwrap();
    let actual_crc = cj
        .invoke_static("LCrc;", "crc32", "([B)I", &[Value::Obj(jr_buf)])
        .unwrap();
    // Standard IEEE 802.3 CRC-32 of "123456789" == 0xCBF43926!
    assert_eq!(expected_crc, Value::Int(0xCBF4_3926u32 as i32));
    assert_eq!(actual_crc, expected_crc);
    assert_eq!(cj.jit_stats().compiled, 1);
    assert_eq!(cj.fuel_used(), ci.fuel_used());
}

#[test]
fn virtual_instance_methods_and_void_return_compile_and_match_interpreter() {
    let mut b = DexBuilder::new();
    let c = b.class("LInst;", "Ljava/lang/Object;");
    // `<init>()V`: return-void (`0x0e`)
    b.direct(
        c,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 0, op10x(0x0e))),
    );
    // Virtual method `sameAndHash(Ljava/lang/Object;I)I`:
    //   v1 = this, v2 = other, v3 = x
    //   if (this == other) return x ^ 0x1357; else return x + 1;
    let mut vcode = Vec::new();
    vcode.extend(op22t(0x32, 1, 2, 4)); // pc 0 (2): if-eq v1, v2 -> pc 4
    vcode.extend(op22b(0xd8, 0, 3, 1)); // pc 2 (2): v0 = v3 + 1
    vcode.extend(op11x(0x0f, 0)); // pc 4? wait: 2+2=4 -> make pc 0 jump +5 -> pc 5!
    vcode.clear();
    vcode.extend(op22t(0x32, 1, 2, 5)); // pc 0 (2): if-eq v1, v2 -> pc 5
    vcode.extend(op22b(0xd8, 0, 3, 1)); // pc 2 (2): v0 = v3 + 1
    vcode.extend(op11x(0x0f, 0)); // pc 4 (1): return v0
    vcode.extend(op22s(0xd7, 0, 3, 0x1357)); // pc 5 (2): v0 = v3 ^ 0x1357
    vcode.extend(op11x(0x0f, 0)); // pc 7 (1): return v0
    b.r#virtual(
        c,
        "sameAndHash",
        "I",
        vec!["Ljava/lang/Object;", "I"],
        ACC_PUBLIC,
        Some(b.code(4, 3, 0, vcode)),
    );

    let mut interp = engine_of(&b);
    let mut jit = engine_of(&b);
    jit.enable_jit().unwrap();

    let io1 = interp.new_instance("LInst;").unwrap();
    let io2 = interp.new_instance("LInst;").unwrap();
    let jo1 = jit.new_instance("LInst;").unwrap();
    let jo2 = jit.new_instance("LInst;").unwrap();

    for n in 0..45 {
        let iother = match n % 3 {
            0 => Value::Obj(io1),
            1 => Value::Obj(io2),
            _ => Value::Null,
        };
        let jother = match n % 3 {
            0 => Value::Obj(jo1),
            1 => Value::Obj(jo2),
            _ => Value::Null,
        };
        let ir = interp
            .invoke_instance(
                io1,
                "sameAndHash",
                "(Ljava/lang/Object;I)I",
                &[iother, Value::Int(n)],
            )
            .unwrap();
        let jr = jit
            .invoke_instance(
                jo1,
                "sameAndHash",
                "(Ljava/lang/Object;I)I",
                &[jother, Value::Int(n)],
            )
            .unwrap();
        assert_eq!(jr, ir, "n={n}");
    }
    assert!(jit.jit_stats().compiled >= 1);
    assert!(jit.jit_stats().native_calls > 10);
    assert_eq!(jit.fuel_used(), interp.fuel_used());
}
