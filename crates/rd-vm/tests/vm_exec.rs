//! Testes de integração do interpretador rd-vm sobre DEXs sintéticos
//! construídos byte a byte (mesma política dos fixtures do rd-dex).
//! Cada teste valida um grupo de opcodes do contrato M2 ("métodos puros").
//!
//! Builder/helpers compartilhados: tests/common/mod.rs (usado também por app.rs).

mod common;

use common::*;
use rd_vm::engine::{Engine, VmConfig};
use rd_vm::err::VmExit;
use rd_vm::value::Value;
pub const ACC_CONSTRUCTOR: u32 = 0x1_0000;

#[test]
fn arith_int_2addr_and_lits() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    // soma(II)I: v0 = v3 + v4; return v0
    b.direct(
        cls,
        "soma",
        "I",
        vec!["I", "I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(5, 2, 0, {
            let mut u = op23x(0x90, 0, 3, 4);
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    // calc(II)I: v0 = v3 * v4; v0 += 7 (lit8); return v0
    b.direct(
        cls,
        "calc",
        "I",
        vec!["I", "I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(5, 2, 0, {
            let mut u = op23x(0x92, 0, 3, 4);
            u.extend(op22b(0xD8, 0, 0, 7));
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    // rsub10(I)I: v0 = 10 - v3 (rsub-int/lit16)
    b.direct(
        cls,
        "rsub10",
        "I",
        vec!["I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(4, 1, 0, {
            let mut u = op22s(0xD1, 0, 3, 10);
            u.extend(op11x(0x0F, 0));
            u
        })),
    );

    let mut e = engine_of(&b);
    assert_eq!(
        invoke(&mut e, "soma", "(II)I", &[Value::Int(5), Value::Int(3)])
            .unwrap()
            .as_int()
            .unwrap(),
        8
    );
    assert_eq!(
        invoke(&mut e, "calc", "(II)I", &[Value::Int(6), Value::Int(7)])
            .unwrap()
            .as_int()
            .unwrap(),
        49
    );
    assert_eq!(
        invoke(&mut e, "rsub10", "(I)I", &[Value::Int(4)])
            .unwrap()
            .as_int()
            .unwrap(),
        6
    );
}

#[test]
fn div_rem_wrapping_and_exceptions() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    b.direct(
        cls,
        "div",
        "I",
        vec!["I", "I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(5, 2, 0, {
            let mut u = op23x(0x93, 0, 3, 4);
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    let mut e = engine_of(&b);
    // MIN / -1 = MIN (wrapping, sem pânico)
    assert_eq!(
        invoke(
            &mut e,
            "div",
            "(II)I",
            &[Value::Int(i32::MIN), Value::Int(-1)]
        )
        .unwrap()
        .as_int()
        .unwrap(),
        i32::MIN
    );
    // / 0 → ArithmeticException
    match invoke(&mut e, "div", "(II)I", &[Value::Int(1), Value::Int(0)]) {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/ArithmeticException;");
            assert_eq!(t.message.as_deref(), Some("divide by zero"));
        }
        other => panic!("esperava ArithmeticException, got {other:?}"),
    }
}

#[test]
fn long_and_double_wide_flow() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    // wide(JJ)J: regs=7, ins=4 → args v3/v4, v5/v6; v0 = v3 + v5; return-wide v0
    b.direct(
        cls,
        "wide",
        "J",
        vec!["J", "J"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(7, 4, 0, {
            let mut u = op23x(0x9B, 0, 3, 5);
            u.extend(op11x(0x10, 0));
            u
        })),
    );
    // const64()J: const-wide v0; return-wide v0
    b.direct(
        cls,
        "const64",
        "J",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(2, 0, 0, {
            let mut u = op51l(0x18, 0, 0x0123_4567_89AB_CDEF);
            u.extend(op11x(0x10, 0));
            u
        })),
    );
    let mut e = engine_of(&b);
    assert_eq!(
        invoke(
            &mut e,
            "wide",
            "(JJ)J",
            &[Value::Long(1 << 40), Value::Long(1)]
        )
        .unwrap()
        .as_long()
        .unwrap(),
        (1 << 40) + 1
    );
    assert_eq!(
        invoke(&mut e, "const64", "()J", &[])
            .unwrap()
            .as_long()
            .unwrap(),
        0x0123_4567_89AB_CDEF
    );
}

#[test]
fn float_double_java_semantics() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    // fdiv(FF)F: v0 = v2 / v3
    b.direct(
        cls,
        "fdiv",
        "F",
        vec!["F", "F"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(4, 2, 0, {
            let mut u = op23x(0xA9, 0, 2, 3);
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    // dcmpl(DD)I: v0 = cmpl-double(v2/v3, v4/v5)
    b.direct(
        cls,
        "dcmpl",
        "I",
        vec!["D", "D"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(6, 4, 0, {
            let mut u = op23x(0x2F, 0, 2, 4);
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    let mut e = engine_of(&b);
    // 1.0f / 0.0f = +Infinity (não exceção)
    assert_eq!(
        invoke(
            &mut e,
            "fdiv",
            "(FF)F",
            &[Value::Float(1.0), Value::Float(0.0)]
        )
        .unwrap(),
        Value::Float(f32::INFINITY)
    );
    // NaN compara com cmpl → -1
    assert_eq!(
        invoke(
            &mut e,
            "dcmpl",
            "(DD)I",
            &[Value::Double(f64::NAN), Value::Double(1.0)]
        )
        .unwrap()
        .as_int()
        .unwrap(),
        -1
    );
}

#[test]
fn branches_and_loop() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    // fat(I)I iterativo — arg em v3, acumulador v1
    // 0:      const/4 v1, 1            (1 unit)
    // 1..2:   if-le v3, v1, +6 → 7     (2 units)
    // 3:      mul-int/2addr v1, v3     (1 unit)
    // 4..5:   add-int/lit8 v3, v3, -1  (2 units)
    // 6:      goto -5 → 1              (1 unit)
    // 7:      return v1
    b.direct(
        cls,
        "fat",
        "I",
        vec!["I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(4, 1, 0, {
            let mut u = Vec::new();
            u.extend(op11n(0x12, 1, 1)); // 0: v1 = 1 (acc)
            u.extend(op11n(0x12, 2, 1)); // 1: v2 = 1 (constante)
            u.extend(op22t(0x37, 3, 2, 6)); // 2..3: if-le v3, v2 → 8
            u.extend(op12x(0xB2, 1, 3)); // 4: mul-int/2addr v1, v3
            u.extend(op22b(0xD8, 3, 3, -1)); // 5..6: v3 -= 1
            u.extend(op10t(0x28, -6)); // 7: goto 1
            u.extend(op11x(0x0F, 1)); // 8: return v1
            u
        })),
    );
    let mut e = engine_of(&b);
    assert_eq!(
        invoke(&mut e, "fat", "(I)I", &[Value::Int(5)])
            .unwrap()
            .as_int()
            .unwrap(),
        120
    );
    assert_eq!(
        invoke(&mut e, "fat", "(I)I", &[Value::Int(1)])
            .unwrap()
            .as_int()
            .unwrap(),
        1
    );
}

#[test]
fn packed_switch() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    // sw(I)I — arg em v3, resultado v1. const/4 é SINALIZADO de 4 bits
    // (10 vira -6!) — valores ≥ 8 usam const/16 (2 units).
    // 0..2:   packed-switch v3, +6 → payload @6 (unit par, alinhado)
    // 3..4:   const/16 v1, 99   ← default (fall-through)
    // 5:      return v1
    // 6..15:  payload (10 units; alvos relativos ao switch @0)
    // 16..17: const/16 v1, 10; 18: return      (caso 1)
    // 19..20: const/16 v1, 20; 21: return      (caso 2)
    // 22..23: const/16 v1, 30; 24: return      (caso 3)
    let mut u = op31t(0x2B, 3, 6);
    u.extend(op21s(0x13, 1, 99)); // 3..4: default
    u.extend(op11x(0x0F, 1)); // 5
    u.extend(packed_payload(1, &[16, 19, 22])); // 6..15
    u.extend(op21s(0x13, 1, 10)); // 16..17
    u.extend(op11x(0x0F, 1)); // 18
    u.extend(op21s(0x13, 1, 20)); // 19..20
    u.extend(op11x(0x0F, 1)); // 21
    u.extend(op21s(0x13, 1, 30)); // 22..23
    u.extend(op11x(0x0F, 1)); // 24
    b.direct(
        cls,
        "sw",
        "I",
        vec!["I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(4, 1, 0, u)),
    );

    let mut e = engine_of(&b);
    assert_eq!(
        invoke(&mut e, "sw", "(I)I", &[Value::Int(1)])
            .unwrap()
            .as_int()
            .unwrap(),
        10
    );
    assert_eq!(
        invoke(&mut e, "sw", "(I)I", &[Value::Int(2)])
            .unwrap()
            .as_int()
            .unwrap(),
        20
    );
    assert_eq!(
        invoke(&mut e, "sw", "(I)I", &[Value::Int(3)])
            .unwrap()
            .as_int()
            .unwrap(),
        30
    );
    assert_eq!(
        invoke(&mut e, "sw", "(I)I", &[Value::Int(7)])
            .unwrap()
            .as_int()
            .unwrap(),
        99
    );
}

#[test]
fn strings_length_and_hashcode() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let sidx_hello = b.intern("hello");
    let sidx_abc = b.intern("abc");
    // method_ids determinísticos: 0=length, 1=hashCode
    let p_i0 = b.proto_idx("I", vec![]);
    let length_mid = b.method_idx("Ljava/lang/String;", p_i0, "length");
    let hashcode_mid = b.method_idx("Ljava/lang/String;", p_i0, "hashCode");
    assert_eq!(length_mid, 0);
    assert_eq!(hashcode_mid, 1);

    // strlen()I: const-string "hello"; length; return
    b.direct(
        cls,
        "strlen",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(2, 0, 1, {
            let mut u = op21c(0x1A, 0, sidx_hello as u16);
            u.extend(op35c(0x6E, 1, length_mid, [0, 0, 0, 0, 0]));
            u.extend(op11x(0x0A, 0));
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    // str_hash()I: const-string "abc"; hashCode; return
    b.direct(
        cls,
        "str_hash",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(2, 0, 1, {
            let mut u = op21c(0x1A, 0, sidx_abc as u16);
            u.extend(op35c(0x6E, 1, hashcode_mid, [0, 0, 0, 0, 0]));
            u.extend(op11x(0x0A, 0));
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    let mut e = engine_of(&b);
    assert_eq!(
        invoke(&mut e, "strlen", "()I", &[])
            .unwrap()
            .as_int()
            .unwrap(),
        5
    );
    // "abc".hashCode() = 96354 (algoritmo especificado do Java)
    assert_eq!(
        invoke(&mut e, "str_hash", "()I", &[])
            .unwrap()
            .as_int()
            .unwrap(),
        96354
    );
}

#[test]
fn statics_clinit_and_static_values() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    // issue #66: o fixture AGORA tem <clinit> REAL (antes: só static_values —
    // o comentário mentia “clinit roda uma única vez” sem clinit nenhum).
    b.static_field(cls, "BASE", "I", SVal::Int(0));
    let fidx_base = b.field_idx("LCaso;", "I", "BASE");
    b.direct(
        cls,
        "<clinit>",
        "V",
        vec![],
        ACC_STATIC | ACC_CONSTRUCTOR,
        Some(b.code(2, 0, 0, {
            let mut u = op21s(0x13, 0, 100); // const/16 v0, 100 (const/4 não cabe)
            u.extend(op21c(0x67, 0, fidx_base)); // sput v0, BASE (clinit real)
            u.extend(op10x(0x0E));
            u
        })),
    );
    // le_base()I: sget v0, BASE; +1; return
    b.direct(
        cls,
        "le_base",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(2, 0, 0, {
            let mut u = op21c(0x60, 0, fidx_base);
            u.extend(op22b(0xD8, 0, 0, 1));
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    let mut e = engine_of(&b);
    // 101 prova que o <clinit> RODOU (default era 0)
    assert_eq!(
        invoke(&mut e, "le_base", "()I", &[])
            .unwrap()
            .as_int()
            .unwrap(),
        101
    );
    // clinit roda uma única vez (resultado estável — sem re-execução)
    assert_eq!(
        invoke(&mut e, "le_base", "()I", &[])
            .unwrap()
            .as_int()
            .unwrap(),
        101
    );
}

// ── issue #57: campo estático sobe a cadeia de superclasses (JVMS 5.4.3.2) ─

/// sget/sput de campo herdado via ref da SUBCLASSE usam o MESMO slot da
/// declarante. Antes: statics keyados pela classe do ref — leitura via
/// subclasse dava default silencioso e sput criava slot divergente.
#[test]
fn static_field_resolution_walks_superclasses() {
    let mut b = DexBuilder::new();
    let base = b.class("LBase;", "Ljava/lang/Object;");
    b.static_field(base, "bcnt", "I", SVal::Int(1));
    b.class("LSub;", "LBase;"); // NÃO declara bcnt
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let f_via_sub = b.field_idx("LSub;", "I", "bcnt"); // ref pela subclasse
    let f_via_base = b.field_idx("LBase;", "I", "bcnt"); // ref pela declarante
    b.direct(
        cls,
        "le_via_sub",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(1, 0, 0, {
            let mut u = op21c(0x60, 0, f_via_sub);
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    b.direct(
        cls,
        "grava_via_sub",
        "V",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(2, 0, 0, {
            let mut u = op21s(0x13, 0, 42); // const/16 v0, 42 (const/4 não cabe)
            u.extend(op21c(0x67, 0, f_via_sub)); // sput via LSub;
            u.extend(op10x(0x0E));
            u
        })),
    );
    b.direct(
        cls,
        "le_via_base",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(1, 0, 0, {
            let mut u = op21c(0x60, 0, f_via_base);
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    let mut e = engine_of(&b);
    assert_eq!(
        invoke(&mut e, "le_via_sub", "()I", &[])
            .unwrap()
            .as_int()
            .unwrap(),
        1,
        "sget via subclasse lê o slot da declarante (não default 0)"
    );
    invoke(&mut e, "grava_via_sub", "()V", &[]).unwrap();
    assert_eq!(
        invoke(&mut e, "le_via_base", "()I", &[])
            .unwrap()
            .as_int()
            .unwrap(),
        42,
        "sput via LSub; e leitura via LBase; veem o MESMO slot (antes: divergente)"
    );
    assert_eq!(
        invoke(&mut e, "le_via_sub", "()I", &[])
            .unwrap()
            .as_int()
            .unwrap(),
        42,
        "ref pela subclasse continua vendo o mesmo valor"
    );
}

/// Campo estático inexistente em toda a cadeia → NoSuchFieldError CAPTURÁVEL
/// (JVMS 5.4.3.2; antes: default silencioso).
#[test]
fn static_field_missing_is_catchable_nosuchfielderror() {
    let mut b = DexBuilder::new();
    b.type_idx("Ljava/lang/NoSuchFieldError;");
    b.type_idx("Ljava/lang/Error;");
    b.class("LBase;", "Ljava/lang/Object;");
    b.class("LSub;", "LBase;");
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let f_nope = b.field_idx("LSub;", "I", "nao_existe"); // nem em LSub; nem em LBase;
                                                          // direto()I: sget SEM try → NoSuchFieldError escapa
    b.direct(
        cls,
        "direto",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(1, 0, 0, {
            let mut u = op21c(0x60, 0, f_nope);
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    // tenta()I: try { sget } catch (Error) → 1
    let mut blob = b.code(2, 0, 0, {
        let mut u = op11n(0x12, 0, 0); // 0: flag=0
        u.extend(op21c(0x60, 1, f_nope)); // 1..3: sget v1 (TRY)
        u.extend(op11x(0x0F, 0)); // 3: return flag
        u.extend(op11x(0x0D, 1)); // 4: handler: move-exception v1
        u.extend(op11n(0x12, 0, 1)); // 5: flag=1
        u.extend(op11x(0x0F, 0)); // 6: return flag
        u
    });
    blob.tries.push(TryBlob {
        start: 1,
        count: 1,
        typed: vec![("Ljava/lang/Error;".to_string(), 4)],
        catch_all: None,
    });
    b.direct(
        cls,
        "tenta",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(blob),
    );

    let mut e = engine_of(&b);
    match invoke(&mut e, "direto", "()I", &[]) {
        Err(VmExit::Exception(t)) => {
            assert_eq!(
                t.class, "Ljava/lang/NoSuchFieldError;",
                "erro certo: {:?}",
                t
            )
        }
        other => panic!("esperava NoSuchFieldError, got {other:?}"),
    }
    let v = invoke(&mut e, "tenta", "()I", &[])
        .unwrap()
        .as_int()
        .unwrap();
    assert_eq!(v, 1, "NoSuchFieldError é capturável por catch (Error)");
}

/// issue #66 (probe promovido): clinit de A roda ANTES do clinit de B quando
/// B é acessado primeiro — o clinit de B lê o resultado do de A.
#[test]
fn clinit_runs_super_before_sub() {
    let mut b = DexBuilder::new();
    let a = b.class("LA;", "Ljava/lang/Object;");
    b.static_field(a, "sa", "I", SVal::Int(0));
    let f_sa = b.field_idx("LA;", "I", "sa");
    b.direct(
        a,
        "<clinit>",
        "V",
        vec![],
        ACC_STATIC | ACC_CONSTRUCTOR,
        Some(b.code(2, 0, 0, {
            let mut u = op21c(0x60, 0, f_sa);
            u.extend(op22b(0xD8, 0, 0, 1)); // sa += 1 → 1
            u.extend(op21c(0x67, 0, f_sa));
            u.extend(op10x(0x0E));
            u
        })),
    );
    let sub = b.class("LB;", "LA;");
    b.static_field(sub, "sb", "I", SVal::Int(0));
    let f_sb = b.field_idx("LB;", "I", "sb");
    let f_sa2 = b.field_idx("LA;", "I", "sa");
    // B.<clinit>: sb = sa + 1 — SÓ dá 2 se o clinit de A rodou antes
    b.direct(
        sub,
        "<clinit>",
        "V",
        vec![],
        ACC_STATIC | ACC_CONSTRUCTOR,
        Some(b.code(2, 0, 0, {
            let mut u = op21c(0x60, 0, f_sa2);
            u.extend(op22b(0xD8, 0, 0, 1));
            u.extend(op21c(0x67, 0, f_sb));
            u.extend(op10x(0x0E));
            u
        })),
    );
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let f_sb2 = b.field_idx("LB;", "I", "sb");
    b.direct(
        cls,
        "le",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(1, 0, 0, {
            let mut u = op21c(0x60, 0, f_sb2);
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    let mut e = engine_of(&b);
    assert_eq!(
        invoke(&mut e, "le", "()I", &[]).unwrap().as_int().unwrap(),
        2,
        "ordem A→B: sb = sa(1) + 1 = 2"
    );
}

/// issue #66 (probe promovido): o PRIMEIRO acesso a classe com clinit que
/// falha, dentro de try/catch e com engine FRESCO, captura EIIE (o teste
/// oficial reusa engine → exercita NCDFE; este cobre o caminho fresco).
#[test]
fn eiie_first_access_catchable_on_fresh_engine() {
    let build = || {
        let mut b = DexBuilder::new();
        b.type_idx("Ljava/lang/Error;");
        let fail = b.class("LFail;", "Ljava/lang/Object;");
        b.direct(
            fail,
            "<clinit>",
            "V",
            vec![],
            ACC_STATIC | ACC_CONSTRUCTOR,
            Some(b.code(2, 0, 0, {
                let mut u = op11n(0x12, 0, 1);
                u.extend(op11n(0x12, 1, 0));
                u.extend(op23x(0x93, 0, 0, 1)); // div por zero no clinit
                u.extend(op10x(0x0E));
                u
            })),
        );
        let cls = b.class("LCaso;", "Ljava/lang/Object;");
        let fail_tidx = b.types.iter().position(|t| t == "LFail;").unwrap() as u16;
        let mut blob = b.code(3, 0, 0, {
            let mut u = op11n(0x12, 0, 0); // 0: flag=0
            u.extend(op21c(0x22, 1, fail_tidx)); // 1..3: new-instance v1 (TRY)
            u.extend(op11n(0x12, 0, 9)); // 3: flag=9 (não deve ocorrer)
            u.extend(op11x(0x0F, 0)); // 4: return flag
            u.extend(op11x(0x0D, 2)); // 5: handler: move-exception v2
            u.extend(op11n(0x12, 0, 1)); // 6: flag=1
            u.extend(op11x(0x0F, 0)); // 7: return flag
            u
        });
        blob.tries.push(TryBlob {
            start: 1,
            count: 2,
            typed: vec![("Ljava/lang/Error;".to_string(), 5)],
            catch_all: None,
        });
        b.direct(
            cls,
            "tenta",
            "I",
            vec![],
            ACC_PUBLIC | ACC_STATIC,
            Some(blob),
        );
        b
    };
    // engine FRESCO: clinit nunca rodou → EIIE (não NCDFE) no primeiro acesso
    let mut e = engine_of(&build());
    let v = invoke(&mut e, "tenta", "()I", &[])
        .unwrap()
        .as_int()
        .unwrap();
    assert_eq!(v, 1, "EIIE de clinit fresco é capturável (catch Error)");
}

#[test]
fn instances_fields_and_constructors() {
    let mut b = DexBuilder::new();
    let ponto = b.class("LPonto;", "Ljava/lang/Object;");
    b.instance_field(ponto, "x", "I");
    b.instance_field(ponto, "y", "I");
    let fidx_x = b.field_idx("LPonto;", "I", "x");
    let fidx_y = b.field_idx("LPonto;", "I", "y");
    // <init>(II)V: regs=5, ins=3 → this=v2, x=v3, y=v4
    let p_void = b.proto_idx("V", vec![]);
    let init_object = b.method_idx("Ljava/lang/Object;", p_void, "<init>");
    b.direct(
        ponto,
        "<init>",
        "V",
        vec!["I", "I"],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(5, 3, 1, {
            let mut u = op35c(0x70, 1, init_object, [2, 0, 0, 0, 0]); // Object.<init> (no-op)
            u.extend(op22c(0x59, 3, 2, fidx_x)); // iput v3, v2, x
            u.extend(op22c(0x59, 4, 2, fidx_y)); // iput v4, v2, y
            u.extend(op10x(0x0E));
            u
        })),
    );
    let p_ii = b.proto_idx("V", vec!["I".to_string(), "I".to_string()]);
    let init_ponto = b.method_idx("LPonto;", p_ii, "<init>");
    let ponto_tidx = b.type_idx("LPonto;");
    let caso = b.class("LCaso;", "Ljava/lang/Object;");
    // soma_ponto(II)I: regs=6, ins=2 → args v4,v5
    b.direct(
        caso,
        "soma_ponto",
        "I",
        vec!["I", "I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(6, 2, 3, {
            let mut u = op21c(0x22, 0, ponto_tidx); // new-instance v0, Ponto
            u.extend(op35c(0x70, 3, init_ponto, [0, 4, 5, 0, 0])); // <init> {v0,v4,v5}
            u.extend(op22c(0x52, 1, 0, fidx_x)); // iget v1, v0, x
            u.extend(op22c(0x52, 2, 0, fidx_y)); // iget v2, v0, y
            u.extend(op23x(0x90, 1, 1, 2)); // add-int v1, v1, v2
            u.extend(op11x(0x0F, 1));
            u
        })),
    );
    let mut e = engine_of(&b);
    assert_eq!(
        invoke(
            &mut e,
            "soma_ponto",
            "(II)I",
            &[Value::Int(30), Value::Int(12)]
        )
        .unwrap()
        .as_int()
        .unwrap(),
        42
    );
}

#[test]
fn virtual_dispatch_and_inheritance() {
    let mut b = DexBuilder::new();
    let base = b.class("LBase;", "Ljava/lang/Object;");
    b.r#virtual(
        base,
        "f",
        "I",
        vec![],
        ACC_PUBLIC,
        Some(b.code(2, 1, 0, {
            let mut u = op11n(0x12, 0, 1); // v0 = scratch (this está em v1)
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    let sub = b.class("LSub;", "LBase;");
    b.r#virtual(
        sub,
        "f",
        "I",
        vec![],
        ACC_PUBLIC,
        Some(b.code(2, 1, 0, {
            let mut u = op11n(0x12, 0, 2);
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    let sub_tidx = b.type_idx("LSub;");
    let p_void = b.proto_idx("V", vec![]);
    let init_object = b.method_idx("Ljava/lang/Object;", p_void, "<init>");
    let p_i = b.proto_idx("I", vec![]);
    let f_sub = b.method_idx("LSub;", p_i, "f");
    let caso = b.class("LCaso;", "Ljava/lang/Object;");
    // despacha(I)I: new Sub; <init> Object; invoke-virtual f → 2
    b.direct(
        caso,
        "despacha",
        "I",
        vec!["I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(4, 1, 1, {
            let mut u = op21c(0x22, 0, sub_tidx);
            u.extend(op35c(0x70, 1, init_object, [0, 0, 0, 0, 0]));
            u.extend(op35c(0x6E, 1, f_sub, [0, 0, 0, 0, 0])); // receiver v0
            u.extend(op11x(0x0A, 1)); // move-result v1
            u.extend(op11x(0x0F, 1));
            u
        })),
    );
    let mut e = engine_of(&b);
    // despacho virtual pela classe de RUNTIME (Sub), não pela referência
    assert_eq!(
        invoke(&mut e, "despacha", "(I)I", &[Value::Int(0)])
            .unwrap()
            .as_int()
            .unwrap(),
        2
    );
}

#[test]
fn exceptions_try_catch() {
    let mut b = DexBuilder::new();
    b.type_idx("Ljava/lang/ArithmeticException;");
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    // div_ou_menos1(II)I — args v3,v4
    // 0..1: div-int v0, v3, v4
    // 2:    return v0
    // 3:    move-exception v0      ← handler
    // 4:    const/4 v0, -1
    // 5:    return v0
    // try [0, +3) catch ArithmeticException → 3
    let mut blob = b.code(5, 2, 0, {
        let mut u = op23x(0x93, 0, 3, 4); // 0..1
        u.extend(op11x(0x0F, 0)); // 2
        u.extend(op11x(0x0D, 0)); // 3: move-exception v0
        u.extend(op11n(0x12, 0, -1)); // 4
        u.extend(op11x(0x0F, 0)); // 5
        u
    });
    blob.tries.push(TryBlob {
        start: 0,
        count: 3,
        typed: vec![("Ljava/lang/ArithmeticException;".to_string(), 3)],
        catch_all: None,
    });
    b.direct(
        cls,
        "div_ou_menos1",
        "I",
        vec!["I", "I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(blob),
    );

    let mut e = engine_of(&b);
    assert_eq!(
        invoke(
            &mut e,
            "div_ou_menos1",
            "(II)I",
            &[Value::Int(10), Value::Int(2)]
        )
        .unwrap()
        .as_int()
        .unwrap(),
        5
    );
    assert_eq!(
        invoke(
            &mut e,
            "div_ou_menos1",
            "(II)I",
            &[Value::Int(1), Value::Int(0)]
        )
        .unwrap()
        .as_int()
        .unwrap(),
        -1
    );
}

#[test]
fn arrays_aput_aget_bounds() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let int_arr_tidx = b.type_idx("[I");
    // arr(I)I: new-array v0[3]; aput 5,7,9; aget idx 2 → 9
    b.direct(
        cls,
        "arr",
        "I",
        vec!["I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(5, 1, 0, {
            let mut u = op11n(0x12, 0, 3); // 0: v0=3
            u.extend(op22c(0x23, 0, 0, int_arr_tidx)); // 1..2: new-array v0, v0, [I
            u.extend(op11n(0x12, 1, 5)); // 3
            u.extend(op11n(0x12, 2, 0)); // 4
            u.extend(op23x(0x4B, 1, 0, 2)); // 5..6: aput v1, v0, v2
            u.extend(op11n(0x12, 1, 7)); // 7
            u.extend(op11n(0x12, 2, 1)); // 8
            u.extend(op23x(0x4B, 1, 0, 2)); // 9..10
            u.extend(op21s(0x13, 1, 9)); // 11..12: const/16 v1, 9
            u.extend(op11n(0x12, 2, 2)); // 13
            u.extend(op23x(0x4B, 1, 0, 2)); // 14..15
            u.extend(op23x(0x44, 1, 0, 2)); // 16..17: aget v1, v0, v2 → 9
            u.extend(op11x(0x0F, 1)); // 18
            u
        })),
    );
    let mut e = engine_of(&b);
    assert_eq!(
        invoke(&mut e, "arr", "(I)I", &[Value::Int(0)])
            .unwrap()
            .as_int()
            .unwrap(),
        9
    );

    // fora dos limites → AIOOBE
    let mut b2 = DexBuilder::new();
    let cls2 = b2.class("LCaso;", "Ljava/lang/Object;");
    let t2 = b2.type_idx("[I");
    b2.direct(
        cls2,
        "estoura",
        "I",
        vec!["I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b2.code(4, 1, 0, {
            let mut u = op11n(0x12, 0, 2);
            u.extend(op22c(0x23, 0, 0, t2));
            u.extend(op11n(0x12, 1, 5));
            u.extend(op11n(0x12, 2, 9));
            u.extend(op23x(0x4B, 1, 0, 2)); // aput v1, v0[2], v2=9 → idx 9 de len 2
            u.extend(op11x(0x0F, 1));
            u
        })),
    );
    let mut e2 = engine_of(&b2);
    match invoke(&mut e2, "estoura", "(I)I", &[Value::Int(0)]) {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/ArrayIndexOutOfBoundsException;")
        }
        other => panic!("esperava AIOOBE, got {other:?}"),
    }
}

#[test]
fn fuel_limit_stops_runaway_loop() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    // loop infinito: goto +0 (fica no próprio goto)
    b.direct(
        cls,
        "loop_infinito",
        "I",
        vec!["I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(2, 1, 0, op10t(0x28, 0))),
    );
    let dex = rd_dex::Dex::parse(b.finish()).expect("dex");
    let cfg = VmConfig {
        fuel: 1_000,
        ..VmConfig::default()
    };
    let mut e = Engine::new(vec![dex], cfg);
    match invoke(&mut e, "loop_infinito", "(I)I", &[Value::Int(0)]) {
        Err(VmExit::Error(err)) => assert_eq!(err.code, "VM_FUEL"),
        other => panic!("esperava VM_FUEL, got {other:?}"),
    }
}

#[test]
fn heap_budget_enforced_via_string_ops() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let sb_tidx = b.type_idx("Ljava/lang/StringBuilder;");
    let p_v = b.proto_idx("V", vec![]);
    let init = b.method_idx("Ljava/lang/StringBuilder;", p_v, "<init>");
    let p_append_s = b.proto_idx(
        "Ljava/lang/StringBuilder;",
        vec!["Ljava/lang/String;".to_string()],
    );
    let p_tostring = b.proto_idx("Ljava/lang/String;", vec![]);
    let append = b.method_idx("Ljava/lang/StringBuilder;", p_append_s, "append");
    let tostring = b.method_idx("Ljava/lang/StringBuilder;", p_tostring, "toString");
    let sidx_big = b.intern(&"x".repeat(10_000));
    // gross()String: new SB; append(big) ×3; toString — 30 KB de texto
    b.direct(
        cls,
        "gross",
        "Ljava/lang/String;",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(4, 0, 2, {
            let mut u = op21c(0x22, 0, sb_tidx);
            u.extend(op35c(0x70, 1, init, [0, 0, 0, 0, 0]));
            u.extend(op21c(0x1A, 1, sidx_big as u16));
            for _ in 0..3 {
                u.extend(op35c(0x6E, 2, append, [0, 1, 0, 0, 0]));
                u.extend(op11x(0x0C, 0));
            }
            u.extend(op35c(0x6E, 1, tostring, [0, 0, 0, 0, 0]));
            u.extend(op11x(0x0C, 0));
            u.extend(op11x(0x11, 0));
            u
        })),
    );
    let dex = rd_dex::Dex::parse(b.finish()).expect("dex");
    let cfg = VmConfig {
        heap_budget: 40_000, // 30 KB de texto + cópias não cabem
        ..VmConfig::default()
    };
    let mut e = Engine::new(vec![dex], cfg);
    match invoke(&mut e, "gross", "()Ljava/lang/String;", &[]) {
        Err(VmExit::Error(err)) => assert_eq!(err.code, "VM_OOM"),
        other => panic!("esperava VM_OOM, got {other:?}"),
    }
}

#[test]
fn unop_conversions_follow_java() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    // to_byte(I)I: int-to-byte v0, v3
    b.direct(
        cls,
        "to_byte",
        "I",
        vec!["I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(4, 1, 0, {
            let mut u = op12x(0x8D, 0, 3);
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    // f2i(F)I: float-to-int v0, v3 (NaN→0, saturante — semântica Java)
    b.direct(
        cls,
        "f2i",
        "I",
        vec!["F"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(4, 1, 0, {
            let mut u = op12x(0x87, 0, 3);
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    let mut e = engine_of(&b);
    assert_eq!(
        invoke(&mut e, "to_byte", "(I)I", &[Value::Int(0x1FF)])
            .unwrap()
            .as_int()
            .unwrap(),
        -1
    );
    // 1e20f → saturação para i32::MAX (semântica Java)
    assert_eq!(
        invoke(&mut e, "f2i", "(F)I", &[Value::Float(1e20)])
            .unwrap()
            .as_int()
            .unwrap(),
        i32::MAX
    );
}

// ═══════════════════ regressões da auditoria rodada 2 (issues #17–#28, #37) ═

fn op31i(op: u8, a: u8, lit: i32) -> Vec<u16> {
    vec![
        (op as u16) | ((a as u16) << 8),
        lit as u16,
        (lit >> 16) as u16,
    ]
}
fn array_payload(width: u16, count: u32, data_units: &[u16]) -> Vec<u16> {
    let mut v = vec![0x0300u16, width, count as u16, (count >> 16) as u16];
    v.extend_from_slice(data_units);
    v
}

/// issue #22: `nop` (0x00) é opcode REAL — não pode matar a execução
#[test]
fn nop_is_executable_real_opcode() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    b.direct(
        cls,
        "com_nop",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(1, 0, 0, {
            let mut u = op21s(0x13, 0, 41); // const/16 v0, 41 (const/4 é 4-bit!)
            u.extend(op10x(0x00)); // nop
            u.extend(op10x(0x00)); // nop
            u.extend(op11x(0x0F, 0)); // return v0
            u
        })),
    );
    let mut e = engine_of(&b);
    assert_eq!(
        invoke(&mut e, "com_nop", "()I", &[])
            .unwrap()
            .as_int()
            .unwrap(),
        41
    );
}

/// issue #23: try aninhado — o handler escolhido é o do try MAIS INTERNO
#[test]
fn nested_try_selects_innermost_handler() {
    let mut b = DexBuilder::new();
    b.type_idx("Ljava/lang/ArithmeticException;");
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    // 0: const/4 v0, 10
    // 1..2: div-int v0, v0, v3   ← dentro do try INTERNO [1..3)
    // 3: return v0
    // 4: move-exception v0       ← handler INTERNO
    // 5: const/4 v0, -1
    // 6: return v0
    // 7: move-exception v0       ← handler EXTERNO
    // 8: const/4 v0, -10
    // 9: return v0
    let mut blob = b.code(5, 2, 0, {
        let mut u = op21s(0x13, 0, 10); // 0 (const/4 é 4-bit com sinal)
        u.extend(op23x(0x93, 0, 0, 3)); // 1..2 div-int v0, v0, v3
        u.extend(op11x(0x0F, 0)); // 3
        u.extend(op11x(0x0D, 0)); // 4
        u.extend(op11n(0x12, 0, -1)); // 5
        u.extend(op11x(0x0F, 0)); // 6
        u.extend(op11x(0x0D, 0)); // 7
        u.extend(op11n(0x12, 0, -10)); // 8
        u.extend(op11x(0x0F, 0)); // 9
        u
    });
    // ordem do DEX: try EXTERNO (start 0) vem ANTES do interno (start 2)
    // layout: const/16@0..1, div@2..3, ret@4, move-exc@5(inner), ret@7,
    //         move-exc@8(outer), ret@10
    blob.tries.push(TryBlob {
        start: 0,
        count: 4,
        typed: vec![("Ljava/lang/ArithmeticException;".to_string(), 8)],
        catch_all: None,
    });
    blob.tries.push(TryBlob {
        start: 2,
        count: 2,
        typed: vec![("Ljava/lang/ArithmeticException;".to_string(), 5)],
        catch_all: None,
    });
    b.direct(
        cls,
        "aninhado",
        "I",
        vec!["I", "I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(blob),
    );
    let mut e = engine_of(&b);
    // Java: div por zero dentro do try interno → handler INTERNO (-1)
    assert_eq!(
        invoke(&mut e, "aninhado", "(II)I", &[Value::Int(0), Value::Int(0)])
            .unwrap()
            .as_int()
            .unwrap(),
        -1
    );
}

/// issue #24: fill-array-data com float[]/double[] (bits decodificados)
#[test]
fn fill_array_data_float_and_double() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let f_arr = b.type_idx("[F");
    // 1.5f = 0x3FC00000 → units LE [0x0000, 0x3FC0]; 2.5f = 0x40200000
    b.direct(
        cls,
        "farr",
        "F",
        vec!["I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(5, 1, 0, {
            let mut u = op11n(0x12, 0, 2); // 0: v0 = 2
            u.extend(op22c(0x23, 0, 0, f_arr)); // 1..2: new-array v0, v0, [F
            u.extend(op31t(0x26, 0, 3)); // 3..5: fill-array-data v0, +3
            u.extend(array_payload(4, 2, &[0x0000, 0x3FC0, 0x0000, 0x4020])); // 6..9: payload (4 units)
            u.extend(op11n(0x12, 2, 0)); // 10: v2 = 0
            u.extend(op23x(0x44, 1, 0, 2)); // 11..12: aget v1, v0, v2
            u.extend(op11x(0x0F, 1)); // 13: return v1
            u
        })),
    );
    let mut e = engine_of(&b);
    let v = invoke(&mut e, "farr", "(I)F", &[Value::Int(0)]).unwrap();
    match v {
        Value::Float(f) => assert_eq!(f, 1.5, "bits do payload devem virar float"),
        other => panic!("esperado Float, got {other:?}"),
    }
}

/// issue #27: StringBuilder.append((String)null) apendeja "null" (não NPE);
/// também exercita new-instance→<clinit> trigger e invoke-direct <init>
#[test]
fn stringbuilder_append_null_appends_literal() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let sb_tidx = b.type_idx("Ljava/lang/StringBuilder;");
    let s_a = b.intern("a") as u16;
    let p_str_ret_sb = b.proto_idx(
        "Ljava/lang/StringBuilder;",
        vec!["Ljava/lang/String;".to_string()],
    );
    let p_v_str = b.proto_idx("V", vec!["Ljava/lang/String;".to_string()]);
    let init_str = b.method_idx("Ljava/lang/StringBuilder;", p_v_str, "<init>");
    let p_sb_ret_str = b.proto_idx("Ljava/lang/String;", vec![]); // toString() — receiver não é param
    let append = b.method_idx("Ljava/lang/StringBuilder;", p_str_ret_sb, "append");
    let to_string = b.method_idx("Ljava/lang/StringBuilder;", p_sb_ret_str, "toString");
    // regs=5, ins=1 → arg em v4; v3 fica Null (nunca escrito)
    b.direct(
        cls,
        "concat_null",
        "Ljava/lang/String;",
        vec!["I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(5, 1, 2, {
            let mut u = op21c(0x22, 0, sb_tidx); // 0..1: new-instance v0, SB
            u.extend(op21c(0x1A, 1, s_a)); // 2..3: const-string v1, "a"
            u.extend(op35c(0x70, 2, init_str, [0, 1, 0, 0, 0])); // 4..6: <init>{recv=v0, arg=v1}
            u.extend(op35c(0x6E, 2, append, [0, 3, 0, 0, 0])); // 7..9: append(v0, v3=Null)
            u.extend(op35c(0x6E, 1, to_string, [0, 0, 0, 0, 0])); // 10..12: toString
            u.extend(op11x(0x0C, 1)); // 13: move-result-object v1
            u.extend(op11x(0x0F, 1)); // 14: return v1
            u
        })),
    );
    let mut e = engine_of(&b);
    let v = invoke(
        &mut e,
        "concat_null",
        "(I)Ljava/lang/String;",
        &[Value::Int(0)],
    )
    .unwrap();
    match v {
        Value::Obj(r) => assert_eq!(e.heap.as_str(r).unwrap(), "anull"),
        other => panic!("esperado Obj(String), got {other:?}"),
    }
}

/// issue #27: autoboxing Integer.valueOf + intValue (d8 emite para todo
/// List<Integer>/Collections)
#[test]
fn integer_boxing_roundtrip() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let p_int_ret_integer = b.proto_idx("Ljava/lang/Integer;", vec!["I".to_string()]);
    let value_of = b.method_idx("Ljava/lang/Integer;", p_int_ret_integer, "valueOf");
    let p_integer_ret_int = b.proto_idx("I", vec![]);
    let int_value = b.method_idx("Ljava/lang/Integer;", p_integer_ret_int, "intValue");
    b.direct(
        cls,
        "box_unbox",
        "I",
        vec!["I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(4, 1, 1, {
            let mut u = op23x(0x90, 0, 3, 3); // 0..1: v0 = v3 + v3 (=arg*2)
            u.extend(op35c(0x71, 1, value_of, [0, 0, 0, 0, 0])); // 2..4: invoke-static valueOf(v0)
            u.extend(op11x(0x0C, 1)); // 5: move-result-object v1
            u.extend(op35c(0x6E, 1, int_value, [1, 0, 0, 0, 0])); // 6..8: intValue()
            u.extend(op11x(0x0A, 1)); // 9: move-result v1
            u.extend(op11x(0x0F, 1)); // 10: return v1
            u
        })),
    );
    let mut e = engine_of(&b);
    assert_eq!(
        invoke(&mut e, "box_unbox", "(I)I", &[Value::Int(21)])
            .unwrap()
            .as_int()
            .unwrap(),
        42
    );
}

/// issue #26: check-cast de String para CharSequence (interface) é válido
#[test]
fn checkcast_to_interface_succeeds() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let charseq = b.type_idx("Ljava/lang/CharSequence;");
    let s_x = b.intern("x") as u16;
    b.direct(
        cls,
        "cast_iface",
        "Ljava/lang/String;",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(1, 0, 0, {
            let mut u = op21c(0x1A, 0, s_x); // const-string v0, "x"
            u.extend(op21c(0x1F, 0, charseq)); // check-cast v0, CharSequence
            u.extend(op11x(0x0F, 0)); // return-object v0
            u
        })),
    );
    let mut e = engine_of(&b);
    let v = invoke(&mut e, "cast_iface", "()Ljava/lang/String;", &[]).unwrap();
    match v {
        Value::Obj(r) => assert_eq!(e.heap.as_str(r).unwrap(), "x"),
        other => panic!("esperado Obj(String), got {other:?}"),
    }
}

/// issue #37: OOM de new-array vira OutOfMemoryError CAPTURÁVEL
#[test]
fn oom_from_new_array_is_catchable() {
    let mut b = DexBuilder::new();
    b.type_idx("Ljava/lang/OutOfMemoryError;");
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let int_arr = b.type_idx("[I");
    // 0..2: const v0, 0x7FFFFFFF (2G elems × 32 B ≫ 256 MB)
    // 3..4: new-array v0, v0, [I
    // 5: return v0
    // 6: move-exception v0
    // 7: const/4 v0, -1
    // 8: return v0
    let mut blob = b.code(4, 0, 0, {
        let mut u = op31i(0x14, 0, 0x7FFF_FFFF); // 0..2
        u.extend(op22c(0x23, 0, 0, int_arr)); // 3..4
        u.extend(op11x(0x0F, 0)); // 5
        u.extend(op11x(0x0D, 0)); // 6
        u.extend(op11n(0x12, 0, -1)); // 7
        u.extend(op11x(0x0F, 0)); // 8
        u
    });
    blob.tries.push(TryBlob {
        start: 0,
        count: 5,
        typed: vec![("Ljava/lang/OutOfMemoryError;".to_string(), 6)],
        catch_all: None,
    });
    b.direct(
        cls,
        "oom_ou_menos1",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(blob),
    );
    let mut e = engine_of(&b);
    assert_eq!(
        invoke(&mut e, "oom_ou_menos1", "()I", &[])
            .unwrap()
            .as_int()
            .unwrap(),
        -1
    );
}

/// issue #25 + #37: <clinit> dispara em sget; falha vira
/// ExceptionInInitializerError no 1º acesso e NoClassDefFoundError nos
/// seguintes; superclasse inicializa antes da subclasse
#[test]
fn clinit_failure_semantics_and_super_first() {
    let mut b = DexBuilder::new();
    b.type_idx("Ljava/lang/ArithmeticException;");
    // LBase; tem <clinit> que divide por zero (falha)
    let base = b.class("LBase;", "Ljava/lang/Object;");
    let _fs = b.field_idx("LBase;", "I", "s");
    b.static_field(base, "s", "I", SVal::Int(0));
    b.direct(
        base,
        "<clinit>",
        "V",
        vec![],
        ACC_STATIC | ACC_CONSTRUCTOR,
        Some(b.code(2, 0, 0, {
            let mut u = op11n(0x12, 0, 1);
            u.extend(op11n(0x12, 1, 0));
            u.extend(op23x(0x93, 0, 0, 1)); // div-int v0, v0, v1 → Arith
            u.extend(op10x(0x0E)); // return-void (nunca alcançado)
            u
        })),
    );
    // LSub; estende LBase; — se a ordem super-first estiver certa, a falha
    // acontece ANTES do <clinit> de LSub rodar (que marcaria uma flag)
    let sub = b.class("LSub;", "LBase;");
    let _ft = b.field_idx("LSub;", "I", "t");
    b.static_field(sub, "t", "I", SVal::Int(0));
    b.direct(
        sub,
        "<clinit>",
        "V",
        vec![],
        ACC_STATIC | ACC_CONSTRUCTOR,
        Some(b.code(2, 0, 0, {
            let mut u = op21s(0x13, 0, 7);
            u.extend(op10x(0x0E));
            u
        })),
    );
    // LCaso.toque()I: sget LBase;->s → dispara a cadeia de init
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let f_s = b.field_idx("LBase;", "I", "s");
    b.direct(
        cls,
        "toque",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(1, 0, 0, {
            let mut u = op21c(0x60, 0, f_s); // sget v0, LBase->s
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    let mut e = engine_of(&b);
    // 1º acesso → ExceptionInInitializerError (não ArithmeticException cru)
    let err1 = invoke(&mut e, "toque", "()I", &[]).unwrap_err();
    match &err1 {
        VmExit::Exception(t) => {
            assert_eq!(
                t.class, "Ljava/lang/ExceptionInInitializerError;",
                "1º acesso: {err1}"
            )
        }
        other => panic!("1º acesso deveria ser Exception, got {other:?}"),
    }
    // 2º acesso → NoClassDefFoundError (JLS 12.4.2)
    let err2 = invoke(&mut e, "toque", "()I", &[]).unwrap_err();
    match &err2 {
        VmExit::Exception(t) => {
            assert_eq!(
                t.class, "Ljava/lang/NoClassDefFoundError;",
                "2º acesso: {err2}"
            )
        }
        other => panic!("2º acesso deveria ser Exception, got {other:?}"),
    }
}

/// issue #39 (regressão do fix #25): `<clinit>` de superclasse não pode rodar
/// DUAS vezes quando a subclasse é inicializada — `initialize_class` precisa
/// resolver o `<clinit>` APENAS na própria classe (o init da super já acontece
/// via ensure_initialized(super)). JVM: `A.cnt == 1`; com a regressão, == 2.
#[test]
fn clinit_runs_once_per_class_in_hierarchy() {
    let mut b = DexBuilder::new();
    // LA; static int cnt; static { cnt++; }
    let a = b.class("LA;", "Ljava/lang/Object;");
    b.static_field(a, "cnt", "I", SVal::Int(0));
    let f_cnt_a = b.field_idx("LA;", "I", "cnt");
    b.direct(
        a,
        "<clinit>",
        "V",
        vec![],
        ACC_STATIC | ACC_CONSTRUCTOR,
        Some(b.code(2, 0, 0, {
            let mut u = op21c(0x60, 0, f_cnt_a); // sget v0, LA->cnt
            u.extend(op22b(0xD8, 0, 0, 1)); // add-int/lit8 v0, v0, 1
            u.extend(op21c(0x67, 0, f_cnt_a)); // sput v0, LA->cnt
            u.extend(op10x(0x0E)); // return-void
            u
        })),
    );
    // LB extends LA e LC extends LB — SEM <clinit> próprio (é o caminho da
    // regressão: resolve_method(B,"<clinit>") subia até A e re-executava)
    let _b_c = b.class("LB;", "LA;");
    let _c_c = b.class("LC;", "LB;");
    // LCaso.dispara()I: sget LC->cnt (field ref herdado) — dispara init de LC
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let f_cnt_lc = b.field_idx("LC;", "I", "cnt");
    b.direct(
        cls,
        "dispara",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(1, 0, 0, {
            let mut u = op21c(0x60, 0, f_cnt_lc); // sget v0, LC->cnt
            u.extend(op11x(0x0F, 0)); // return v0
            u
        })),
    );
    // LCaso.ler_cnt()I: sget LA->cnt — lê o contador real do clinit de A
    let f_cnt_la = b.field_idx("LA;", "I", "cnt");
    b.direct(
        cls,
        "ler_cnt",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(1, 0, 0, {
            let mut u = op21c(0x60, 0, f_cnt_la); // sget v0, LA->cnt
            u.extend(op11x(0x0F, 0)); // return v0
            u
        })),
    );
    let mut e = engine_of(&b);
    let _ = invoke(&mut e, "dispara", "()I", &[]).unwrap(); // dispara C→B→A
    let v = invoke(&mut e, "ler_cnt", "()I", &[])
        .unwrap()
        .as_int()
        .unwrap();
    assert_eq!(
        v, 1,
        "<clinit> de LA executou {v}× (JLS 12.4.2: exatamente 1)"
    );
    // acessos seguintes: continua 1 (nenhum clinit re-executa)
    let v2 = invoke(&mut e, "ler_cnt", "()I", &[])
        .unwrap()
        .as_int()
        .unwrap();
    assert_eq!(v2, 1);
}

// ── issues #43/#48: corretude de intrinsics ─────────────────────────────────

/// issue #43: Integer/Long equals e hashCode POR VALOR (não identidade).
/// Antes: Integer(1000).equals(Integer(1000)) → false (fallback de Object).
#[test]
fn boxed_equals_hashcode_by_value() {
    let mut e = engine_of(&DexBuilder::new());
    let vof = |e: &mut Engine, v| {
        e.invoke_static(
            "Ljava/lang/Integer;",
            "valueOf",
            "(I)Ljava/lang/Integer;",
            &[Value::Int(v)],
        )
        .unwrap()
    };
    let lof = |e: &mut Engine, v| {
        e.invoke_static(
            "Ljava/lang/Long;",
            "valueOf",
            "(J)Ljava/lang/Long;",
            &[Value::Long(v)],
        )
        .unwrap()
    };
    let i_eq = |e: &mut Engine, recv, arg| {
        rd_vm::intrinsics::call_instance_intrinsic(
            e,
            "Ljava/lang/Integer;",
            "equals",
            "(Ljava/lang/Object;)Z",
            recv,
            &[arg],
        )
        .unwrap()
    };
    let i_hash = |e: &mut Engine, recv| {
        rd_vm::intrinsics::call_instance_intrinsic(
            e,
            "Ljava/lang/Integer;",
            "hashCode",
            "()I",
            recv,
            &[],
        )
        .unwrap()
    };
    let b1 = vof(&mut e, 1000);
    let b2 = vof(&mut e, 1000);
    let b3 = vof(&mut e, 1001);
    let Value::Obj(r1) = b1 else {
        panic!("box não é Obj")
    };
    assert_eq!(i_eq(&mut e, r1, b2.clone()), Some(Value::Int(1)));
    assert_eq!(i_eq(&mut e, r1, b3.clone()), Some(Value::Int(0)));
    assert_eq!(i_hash(&mut e, r1), Some(Value::Int(1000)));
    // cross-type: Integer(1).equals(Long(1)) = false (spec Integer.equals)
    let ibox = vof(&mut e, 1);
    let lbox = lof(&mut e, 1);
    let Value::Obj(ri) = ibox else { panic!() };
    assert_eq!(i_eq(&mut e, ri, lbox), Some(Value::Int(0)));
    // Long.hashCode(1L << 40) = (int)(v ^ (v >>> 32)) = 0x100 = 256
    let lb = lof(&mut e, 1 << 40);
    let Value::Obj(rl) = lb else { panic!() };
    let h = rd_vm::intrinsics::call_instance_intrinsic(
        &mut e,
        "Ljava/lang/Long;",
        "hashCode",
        "()I",
        rl,
        &[],
    )
    .unwrap();
    assert_eq!(h, Some(Value::Int(256)));
    // equals de Long por valor
    let la = lof(&mut e, 7);
    let lb2 = lof(&mut e, 7);
    let Value::Obj(rla) = la else { panic!() };
    let v = rd_vm::intrinsics::call_instance_intrinsic(
        &mut e,
        "Ljava/lang/Long;",
        "equals",
        "(Ljava/lang/Object;)Z",
        rla,
        &[lb2],
    )
    .unwrap();
    assert_eq!(v, Some(Value::Int(1)));
}

/// issue #48: Math.floorDiv/floorMod com semântica FLOOR do Java
/// (div_euclid do Rust dava floorDiv(4,−3) = −1; Java = −2).
#[test]
fn floor_div_mod_java_semantics() {
    let mut e = engine_of(&DexBuilder::new());
    let fdi = |e: &mut Engine, a, b| {
        e.invoke_static(
            "Ljava/lang/Math;",
            "floorDiv",
            "(II)I",
            &[Value::Int(a), Value::Int(b)],
        )
        .unwrap()
        .as_int()
        .unwrap()
    };
    let fmi = |e: &mut Engine, a, b| {
        e.invoke_static(
            "Ljava/lang/Math;",
            "floorMod",
            "(II)I",
            &[Value::Int(a), Value::Int(b)],
        )
        .unwrap()
        .as_int()
        .unwrap()
    };
    assert_eq!(fdi(&mut e, 7, 2), 3);
    assert_eq!(fmi(&mut e, 7, 2), 1);
    assert_eq!(
        fdi(&mut e, 4, -3),
        -2,
        "floorDiv(4,-3): Java = -2 (euclid dava -1)"
    );
    assert_eq!(fmi(&mut e, 4, -3), -2, "floorMod(4,-3): Java = -2");
    assert_eq!(fdi(&mut e, -4, 3), -2);
    assert_eq!(fmi(&mut e, -4, 3), 2);
    assert_eq!(fdi(&mut e, 4, 3), 1);
    assert_eq!(fmi(&mut e, 4, 3), 1);
    // canto MIN/-1: wrapping por contrato da JVM
    assert_eq!(fdi(&mut e, i32::MIN, -1), i32::MIN);
    assert_eq!(fmi(&mut e, i32::MIN, -1), 0);
    // variante long
    let v = e
        .invoke_static(
            "Ljava/lang/Math;",
            "floorDiv",
            "(JJ)J",
            &[Value::Long(4), Value::Long(-3)],
        )
        .unwrap()
        .as_long()
        .unwrap();
    assert_eq!(v, -2);
}

/// issue #48: Long.parseLong(null) → NumberFormatException (não NPE),
/// alinhado com parseInt.
#[test]
fn long_parse_null_is_nfe() {
    let mut e = engine_of(&DexBuilder::new());
    match e.invoke_static(
        "Ljava/lang/Long;",
        "parseLong",
        "(Ljava/lang/String;)J",
        &[Value::Null],
    ) {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/NumberFormatException;")
        }
        other => panic!("esperava NFE, got {other:?}"),
    }
    let s42 = rd_vm::intrinsics::alloc_string(&mut e, "42".to_string()).unwrap();
    let v = e
        .invoke_static(
            "Ljava/lang/Long;",
            "parseLong",
            "(Ljava/lang/String;)J",
            &[Value::Obj(s42)],
        )
        .unwrap();
    assert_eq!(v.as_long().unwrap(), 42);
}

/// issue #48: fill-array-data em char[] decodifica SEM sinal (u16):
/// payload 0xFFFD é U+FFFD (65533), não −3.
#[test]
fn fill_array_data_char_unsigned() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let c_arr = b.type_idx("[C");
    b.direct(
        cls,
        "carr",
        "I",
        vec!["I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(5, 1, 0, {
            let mut u = op11n(0x12, 0, 2); // v0 = 2
            u.extend(op22c(0x23, 0, 0, c_arr)); // new-array v0, v0, [C
            u.extend(op31t(0x26, 0, 3)); // fill-array-data v0, +3
            u.extend(array_payload(2, 2, &[0xFFFD, 0x0041])); // U+FFFD e 'A'
            u.extend(op23x(0x44, 1, 0, 4)); // aget v1, v0, v4 (arg = índice)
            u.extend(op11x(0x0F, 1)); // return v1
            u
        })),
    );
    let mut e = engine_of(&b);
    assert_eq!(
        invoke(&mut e, "carr", "(I)I", &[Value::Int(0)])
            .unwrap()
            .as_int()
            .unwrap(),
        0xFFFD,
        "char[] decodifica sem sinal"
    );
    assert_eq!(
        invoke(&mut e, "carr", "(I)I", &[Value::Int(1)])
            .unwrap()
            .as_int()
            .unwrap(),
        0x41
    );
}

/// issue #43: toString declarado pela classe do usuário EXECUTA em
/// StringBuilder.append(Object) e String.valueOf(Object) — antes caía
/// no `Classe@id` silenciosamente.
#[test]
fn user_tostring_used_by_append_object_and_value_of() {
    let mut b = DexBuilder::new();
    // LPonto com toString() → "PTO"
    let ponto = b.class("LPonto;", "Ljava/lang/Object;");
    let s_pto = b.intern("PTO") as u16;
    b.r#virtual(
        ponto,
        "toString",
        "Ljava/lang/String;",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 0, {
            // ins=1: receiver `this` chega em v0 (const-string sobrescreve)
            let mut u = op21c(0x1A, 0, s_pto); // const-string v0, "PTO"
            u.extend(op11x(0x11, 0)); // return-object v0
            u
        })),
    );
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let sb_tidx = b.type_idx("Ljava/lang/StringBuilder;");
    let p_void = b.proto_idx("V", vec![]);
    let sb_init = b.method_idx("Ljava/lang/StringBuilder;", p_void, "<init>");
    let obj_init = b.method_idx("Ljava/lang/Object;", p_void, "<init>");
    let p_obj_ret_sb = b.proto_idx(
        "Ljava/lang/StringBuilder;",
        vec!["Ljava/lang/Object;".to_string()],
    );
    let append = b.method_idx("Ljava/lang/StringBuilder;", p_obj_ret_sb, "append");
    let p_ret_str = b.proto_idx("Ljava/lang/String;", vec![]);
    let sb_to_string = b.method_idx("Ljava/lang/StringBuilder;", p_ret_str, "toString");
    b.direct(
        cls,
        "junta",
        "Ljava/lang/String;",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(3, 0, 1, {
            let mut u = op21c(0x22, 0, sb_tidx); // new-instance v0, SB
            u.extend(op35c(0x70, 1, sb_init, [0, 0, 0, 0, 0])); // SB.<init>()
            u.extend(op21c(0x22, 1, ponto_tidx_of(&b))); // new-instance v1, Ponto
            u.extend(op35c(0x70, 1, obj_init, [1, 0, 0, 0, 0])); // Object.<init>(v1)
            u.extend(op35c(0x6E, 2, append, [0, 1, 0, 0, 0])); // append(v0, v1)
            u.extend(op35c(0x6E, 1, sb_to_string, [0, 0, 0, 0, 0])); // toString
            u.extend(op11x(0x0C, 2)); // move-result-object v2
            u.extend(op11x(0x11, 2)); // return-object v2
            u
        })),
    );
    let mut e = engine_of(&b);
    let v = invoke(&mut e, "junta", "()Ljava/lang/String;", &[]).unwrap();
    match v {
        Value::Obj(r) => assert_eq!(e.heap.as_str(r).unwrap(), "PTO"),
        other => panic!("esperado Obj(String), got {other:?}"),
    }
}

fn ponto_tidx_of(b: &DexBuilder) -> u16 {
    b.types
        .iter()
        .position(|t| t == "LPonto;")
        .expect("LPonto; no builder") as u16
}

/// issues #43/#48: System.out.println executa (String e Object) — materializa
/// PrintStream no sget e despacha para o stdout do host.
#[test]
fn system_out_println_executes() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let s_hi = b.intern("hi") as u16;
    let p_int_ret_integer = b.proto_idx("Ljava/lang/Integer;", vec!["I".to_string()]);
    let i_value_of = b.method_idx("Ljava/lang/Integer;", p_int_ret_integer, "valueOf");
    let p_v_obj = b.proto_idx("V", vec!["Ljava/lang/Object;".to_string()]);
    let ps_println_obj = b.method_idx("Ljava/io/PrintStream;", p_v_obj, "println");
    let p_v_str = b.proto_idx("V", vec!["Ljava/lang/String;".to_string()]);
    let ps_println_str = b.method_idx("Ljava/io/PrintStream;", p_v_str, "println");
    let f_out = b.field_idx("Ljava/lang/System;", "Ljava/io/PrintStream;", "out");
    b.direct(
        cls,
        "oi",
        "V",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(3, 0, 0, {
            let mut u = op21c(0x1A, 0, s_hi); // const-string v0, "hi"
            u.extend(op21c(0x60, 1, f_out)); // sget v1, System.out
            u.extend(op35c(0x6E, 2, ps_println_str, [1, 0, 0, 0, 0])); // v1.println(v0)
            u.extend(op10x(0x0E)); // return-void
            u
        })),
    );
    b.direct(
        cls,
        "oi_num",
        "V",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(3, 0, 0, {
            let mut u = op21s(0x13, 0, 7); // const/4 v0, 7
            u.extend(op35c(0x71, 1, i_value_of, [0, 0, 0, 0, 0])); // Integer.valueOf(v0)
            u.extend(op11x(0x0C, 0)); // move-result-object v0 → Integer(7)
            u.extend(op21c(0x60, 1, f_out)); // sget v1, System.out
            u.extend(op35c(0x6E, 2, ps_println_obj, [1, 0, 0, 0, 0])); // v1.println(v0)
            u.extend(op10x(0x0E)); // return-void
            u
        })),
    );
    let mut e = engine_of(&b);
    // issue #66: o teste ANTES só fazia expect() (println no-op passava —
    // falso-verde). Agora captura a saída via HostOut::Buffer (issue #55)
    // e asserta o CONTEÚDO impresso (String e Object boxado).
    let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    rd_vm::set_app_stdout(rd_vm::HostOut::Buffer(buf.clone()));
    invoke(&mut e, "oi", "()V", &[]).expect("println(String) não pode falhar");
    invoke(&mut e, "oi_num", "()V", &[]).expect("println(Object box) não pode falhar");
    rd_vm::set_app_stdout(rd_vm::HostOut::Std); // restaura (testes paralelos)
    let out = String::from_utf8(buf.lock().unwrap().clone()).expect("utf8");
    assert_eq!(out, "hi\n7\n", "println(String) e println(Object) imprimem");
}

// ── issues #41/#49: gating de erros (EIIE capturável, OOM tipado, if-eq misto)

/// issue #41: ExceptionInInitializerError é CAPTURÁVEL — dispatch na frame
/// corrente (step! no new-instance/sget/sput) + hierarquia EIIE→LinkageError→
/// Error→Throwable no builtin_hierarchy. Antes: `?` bypassava o try/catch e
/// matava o run inteiro. Segundo acesso → NoClassDefFoundError (JLS 12.4.2).
#[test]
fn eiie_is_catchable_and_second_access_is_ncdfe() {
    let mut b = DexBuilder::new();
    b.type_idx("Ljava/lang/ArithmeticException;");
    b.type_idx("Ljava/lang/Error;");
    let fail = b.class("LFail;", "Ljava/lang/Object;");
    b.direct(
        fail,
        "<clinit>",
        "V",
        vec![],
        ACC_STATIC | ACC_CONSTRUCTOR,
        Some(b.code(2, 0, 0, {
            let mut u = op11n(0x12, 0, 1);
            u.extend(op11n(0x12, 1, 0));
            u.extend(op23x(0x93, 0, 0, 1)); // div por zero no clinit
            u.extend(op10x(0x0E));
            u
        })),
    );
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let fail_tidx = b.types.iter().position(|t| t == "LFail;").unwrap() as u16;
    // direto()I: new-instance SEM try → EIIE/NCDFE escapam como Exception
    b.direct(
        cls,
        "direto",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(2, 0, 0, {
            let mut u = op21c(0x22, 1, fail_tidx); // new-instance v1, LFail
            u.extend(op11n(0x12, 0, 99));
            u.extend(op11x(0x0F, 0)); // return v0
            u
        })),
    );
    // tenta()I: try { new-instance LFail } catch (Error) → flag=1
    let mut blob = b.code(3, 0, 0, {
        let mut u = op11n(0x12, 0, 0); // 0: flag=0
        u.extend(op21c(0x22, 1, fail_tidx)); // 1..3: new-instance v1 (TRY)
        u.extend(op11n(0x12, 0, 9)); // 3: flag=9 (não deve ocorrer)
        u.extend(op11x(0x0F, 0)); // 4: return flag
        u.extend(op11x(0x0D, 2)); // 5: handler: move-exception v2
        u.extend(op11n(0x12, 0, 1)); // 6: flag=1
        u.extend(op11x(0x0F, 0)); // 7: return flag
        u
    });
    blob.tries.push(TryBlob {
        start: 1,
        count: 2,
        typed: vec![("Ljava/lang/Error;".to_string(), 5)],
        catch_all: None,
    });
    b.direct(
        cls,
        "tenta",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(blob),
    );
    let mut e = engine_of(&b);
    // 1º acesso sem try: EIIE (não Arithmetic cru — ensure_initialized wrapa)
    match invoke(&mut e, "direto", "()I", &[]) {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/ExceptionInInitializerError;")
        }
        other => panic!("1º acesso: esperava EIIE, got {other:?}"),
    }
    // 2º acesso: NCDFE
    match invoke(&mut e, "direto", "()I", &[]) {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/NoClassDefFoundError;")
        }
        other => panic!("2º acesso: esperava NCDFE, got {other:?}"),
    }
    // COM try: catch (Error) captura o EIIE (hierarquia nova + step!)
    // (o 1º invoke de tenta re-abre? não — LFail já está clinit_failed →
    // NCDFE, que também é <: Error → captura igualmente válida)
    let v = invoke(&mut e, "tenta", "()I", &[])
        .unwrap()
        .as_int()
        .unwrap();
    assert_eq!(
        v, 1,
        "exceção de clinit deve ser capturável por catch (Error)"
    );
}

/// issue #41 (JLS 12.4.2): falha de <clinit> na SUPERCLASSE propaga o EIIE
/// original — sem duplo-wrap (a message não contém "ExceptionInInitializerError").
#[test]
fn clinit_super_failure_no_double_wrap() {
    let mut b = DexBuilder::new();
    b.type_idx("Ljava/lang/ArithmeticException;");
    let base = b.class("LBase2;", "Ljava/lang/Object;");
    b.direct(
        base,
        "<clinit>",
        "V",
        vec![],
        ACC_STATIC | ACC_CONSTRUCTOR,
        Some(b.code(2, 0, 0, {
            let mut u = op11n(0x12, 0, 1);
            u.extend(op11n(0x12, 1, 0));
            u.extend(op23x(0x93, 0, 0, 1)); // div/0
            u.extend(op10x(0x0E));
            u
        })),
    );
    let _sub = b.class("LSub2;", "LBase2;");
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let sub_tidx = b.types.iter().position(|t| t == "LSub2;").unwrap() as u16;
    b.direct(
        cls,
        "instancia_sub",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(2, 0, 0, {
            let mut u = op21c(0x22, 1, sub_tidx); // new-instance LSub2
            u.extend(op11n(0x12, 0, 7));
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    let mut e = engine_of(&b);
    match invoke(&mut e, "instancia_sub", "()I", &[]) {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/ExceptionInInitializerError;");
            let msg = t.message.clone().unwrap_or_default();
            assert!(
                !msg.contains("ExceptionInInitializerError"),
                "duplo-wrap: message = {msg}"
            );
        }
        other => panic!("esperava EIIE propagado da super, got {other:?}"),
    }
}

/// issue #49: put_field sem budget lança OutOfMemoryError CAPTURÁVEL
/// (antes: VM_TYPE_ERROR mentiroso e run-killer).
#[test]
fn iput_oom_is_catchable_oome() {
    let mut b = DexBuilder::new();
    b.type_idx("Ljava/lang/Throwable;");
    let lt = b.class("LT;", "Ljava/lang/Object;");
    b.instance_field(lt, "f", "I");
    let f_f = b.field_idx("LT;", "I", "f");
    let lt_tidx = b.types.iter().position(|t| t == "LT;").unwrap() as u16;
    let p_void = b.proto_idx("V", vec![]);
    let obj_init = b.method_idx("Ljava/lang/Object;", p_void, "<init>");
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    // sem try: Err = Exception OOME (não Error VM_TYPE_ERROR)
    b.direct(
        cls,
        "escreve",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(3, 0, 0, {
            let mut u = op21c(0x22, 0, lt_tidx); // new-instance v0, LT
            u.extend(op35c(0x70, 1, obj_init, [0, 0, 0, 0, 0]));
            u.extend(op11n(0x12, 1, 1)); // const/4 v1, 1
            u.extend(op22c(0x59, 1, 0, f_f)); // iput v1, v0, f
            u.extend(op11x(0x0F, 1)); // return v1
            u
        })),
    );
    // com try: OOME é capturado por catch (Throwable) → flag=1
    let mut blob = b.code(3, 0, 0, {
        let mut u = op11n(0x12, 0, 0); // flag=0
        u.extend(op21c(0x22, 1, lt_tidx)); // TRY [1..8): new+init+iput
        u.extend(op35c(0x70, 1, obj_init, [1, 0, 0, 0, 0]));
        u.extend(op11n(0x12, 2, 1));
        u.extend(op22c(0x59, 2, 1, f_f)); // iput v2, v1, f ← OOM aqui
        u.extend(op11x(0x0F, 0)); // return flag (não deve ocorrer)
        u.extend(op11x(0x0D, 1)); // handler: move-exception v1
        u.extend(op11n(0x12, 0, 1)); // flag=1
        u.extend(op11x(0x0F, 0)); // return flag
        u
    });
    blob.tries.push(TryBlob {
        // layout real (op35c = 3 units): new@1, invoke@3-5, const@6, iput@7-8,
        // return@9, move-exception@10 — o try precisa cobrir até o iput
        start: 1,
        count: 8,
        typed: vec![("Ljava/lang/Throwable;".to_string(), 10)],
        catch_all: None,
    });
    b.direct(
        cls,
        "escreve_cap",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(blob),
    );
    // heap calibrado: new-instance (32 B) passa exato; o iput de campo novo
    // custa size_of::<Value>() + len("f") e ESTOURA → OOM tipado no iput
    let field_cost = std::mem::size_of::<rd_vm::Value>() + "f".len();
    let mk = |b: &DexBuilder| {
        let dex = rd_dex::Dex::parse(b.finish()).expect("DEX parseia");
        Engine::new(
            vec![dex],
            VmConfig {
                heap_budget: 32 + field_cost - 1,
                ..Default::default()
            },
        )
    };
    let mut e = mk(&b);
    match invoke(&mut e, "escreve", "()I", &[]) {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/OutOfMemoryError;", "iput OOM: {t:?}")
        }
        other => panic!("esperava OOME Throwable, got {other:?}"),
    }
    let mut e2 = mk(&b);
    let v = invoke(&mut e2, "escreve_cap", "()I", &[])
        .unwrap()
        .as_int()
        .unwrap();
    assert_eq!(v, 1, "OOME de iput deve ser capturável");
}

/// issue #49: OOM em Integer.valueOf/const-class → OutOfMemoryError Throwable.
#[test]
fn alloc_oom_is_oome_throwable() {
    // valueOf com heap de 1 byte
    let mut e = Engine::new(
        vec![],
        VmConfig {
            heap_budget: 1,
            ..Default::default()
        },
    );
    match e.invoke_static(
        "Ljava/lang/Integer;",
        "valueOf",
        "(I)Ljava/lang/Integer;",
        &[Value::Int(5)],
    ) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Ljava/lang/OutOfMemoryError;"),
        other => panic!("esperava OOME Throwable, got {other:?}"),
    }
    // const-class com heap que comporta a string do nome mas não o objeto Class
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let foo_tidx = b.type_idx("LFoo;");
    b.direct(
        cls,
        "klass",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(1, 0, 0, {
            let mut u = op21c(0x1C, 0, foo_tidx); // const-class v0, LFoo;
            u.extend(op11n(0x12, 0, 1));
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    let dex = rd_dex::Dex::parse(b.finish()).unwrap();
    let mut e2 = Engine::new(
        vec![dex],
        VmConfig {
            heap_budget: 60,
            ..Default::default()
        },
    );
    match invoke(&mut e2, "klass", "()I", &[]) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Ljava/lang/OutOfMemoryError;"),
        other => panic!("esperava OOME Throwable, got {other:?}"),
    }
}

/// issue #49: if-eq entre Int e objeto NÃO mata o run — comparação mista
/// dá false e segue (a convenção Int(0)=null continua válida vs Null).
#[test]
fn if_eq_mixed_int_obj_is_false_not_killer() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let s_x = b.intern("x") as u16;
    b.direct(
        cls,
        "mistura",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(4, 0, 0, {
            // v0 = Int(0) (convenção null); v1 = Obj; v3 = Null (nunca escrito)
            // (const/4 só aceita [-8,7] — 0x32 é if-EQ; 0x33 é if-ne)
            let mut u = op11n(0x12, 0, 0); // 0: const/4 v0, 0
            u.extend(op21c(0x1A, 1, s_x)); // 1..3: const-string v1, "x"
            u.extend(op11n(0x12, 2, 1)); // 3: v2 = 1 (default)
            u.extend(op22t(0x32, 0, 3, 3)); // 4..6: if-eq v0, v3, +3 (Null: TRUE → pula p/ 7)
            u.extend(op11n(0x12, 2, 2)); // 6: v2 = 2 (não deve executar)
            u.extend(op22t(0x32, 0, 1, 3)); // 7..9: if-eq v0, v1, +3 (Obj: FALSE → segue)
            u.extend(op11n(0x12, 2, 3)); // 9: v2 = 3 (executado)
            u.extend(op11x(0x0F, 2)); // 10: return v2
            u
        })),
    );
    let mut e = engine_of(&b);
    let v = invoke(&mut e, "mistura", "()I", &[])
        .unwrap()
        .as_int()
        .unwrap();
    assert_eq!(
        v, 3,
        "if-eq (Int, Obj) dá false e segue; (Int0, Null) dá true"
    );
}

// ── issue #42: covariância de arrays (aput-object + instanceof)

/// aput-object de SUBTIPO em array de supertipo é legal (Java covariante) —
/// antes: ArrayStoreException espúria ([LSup; só casava textualmente).
/// instanceof String[] <: CharSequence[] via covariância do elemento.
#[test]
fn array_covariance_aput_and_instanceof() {
    let mut b = DexBuilder::new();
    b.type_idx("Ljava/lang/String;");
    b.type_idx("Ljava/lang/CharSequence;");
    let t_objarr = b.type_idx("[Ljava/lang/Object;");
    let t_charseq_arr = b.type_idx("[Ljava/lang/CharSequence;");
    let s_x = b.intern("x") as u16;
    let cls = b.class("LCaso;", "Ljava/lang/Object;");

    // cov()I: aput-object "x" em Object[] → ok (antes: ASE espúria)
    b.direct(
        cls,
        "cov",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(4, 0, 0, {
            let mut u = op11n(0x12, 0, 1); // const/4 v0, 1
            u.extend(op22c(0x23, 1, 0, t_objarr)); // new-array v1, v0, [LObject;
            u.extend(op21c(0x1A, 2, s_x)); // const-string v2, "x"
            u.extend(op11n(0x12, 3, 0)); // const/4 v3, 0
            u.extend(op23x(0x4D, 2, 1, 3)); // aput-object v2, v1, v3
            u.extend(op11n(0x12, 0, 1)); // v0 = 1 (chegou aqui)
            u.extend(op11x(0x0F, 0)); // return v0
            u
        })),
    );
    // inst()I: instance-of String[] vs CharSequence[] → 1 (String <: CharSequence)
    b.direct(
        cls,
        "inst",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(3, 0, 0, {
            let mut u = op11n(0x12, 0, 1);
            u.extend(op22c(0x23, 1, 0, t_objarr)); // new-array [LObject; (identidade)
            u.extend(op22c(0x20, 0, 1, t_charseq_arr)); // instanceof v0, v1, [LCharSequence;
            u.extend(op11x(0x0F, 0)); // return v0 (0: Object[] ∉ CharSequence[])
            u
        })),
    );
    // cov2()I: instanceof String[] vs CharSequence[] — covariância REAL
    let t_strarr = b.type_idx("[Ljava/lang/String;");
    b.direct(
        cls,
        "cov2",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(3, 0, 0, {
            let mut u = op11n(0x12, 0, 1);
            u.extend(op22c(0x23, 1, 0, t_strarr)); // new-array [LString;
            u.extend(op22c(0x20, 0, 1, t_charseq_arr)); // instanceof v0, v1, [LCharSequence;
            u.extend(op11x(0x0F, 0)); // return v0 (1: String[] <: CharSequence[])
            u
        })),
    );
    let mut e = engine_of(&b);
    let v = invoke(&mut e, "cov", "()I", &[]).unwrap().as_int().unwrap();
    assert_eq!(v, 1, "String em Object[] é legal (covariância)");
    let v = invoke(&mut e, "inst", "()I", &[])
        .unwrap()
        .as_int()
        .unwrap();
    assert_eq!(v, 0, "Object[] NÃO é CharSequence[] (recursão dá false)");
    let v = invoke(&mut e, "cov2", "()I", &[])
        .unwrap()
        .as_int()
        .unwrap();
    assert_eq!(v, 1, "String[] <: CharSequence[] (String <: CharSequence)");
}

/// Dalvik's integer zero is a null reference at an object-array store site.
#[test]
fn aput_object_zero_register_is_null_reference() {
    let mut b = DexBuilder::new();
    let typed_array = b.type_idx("[Ljava/util/List;");
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    b.direct(
        cls,
        "storeNull",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(4, 0, 0, {
            let mut u = op11n(0x12, 0, 1); // length = 1
            u.extend(op22c(0x23, 1, 0, typed_array));
            u.extend(op11n(0x12, 2, 0)); // Dalvik null literal
            u.extend(op11n(0x12, 3, 0)); // index = 0
            u.extend(op23x(0x4D, 2, 1, 3)); // aput-object null
            u.extend(op11n(0x12, 0, 1));
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    let mut engine = engine_of(&b);
    let result = invoke(&mut engine, "storeNull", "()I", &[]).unwrap();
    assert_eq!(result.as_int().unwrap(), 1);
}

/// issue #42: valor NÃO-relacionado continua ASE real (covariância de escrita
/// com store check preservado).
#[test]
fn aput_object_unrelated_still_ase() {
    let mut b = DexBuilder::new();
    b.type_idx("Ljava/lang/String;");
    let t_strarr = b.type_idx("[Ljava/lang/String;");
    let p_void = b.proto_idx("V", vec![]);
    let p_i_obj = b.proto_idx("Ljava/lang/Integer;", vec!["I".to_string()]);
    let m_valueof = b.method_idx("Ljava/lang/Integer;", p_i_obj, "valueOf");
    let obj_init = b.method_idx("Ljava/lang/Object;", p_void, "<init>");
    let _ = obj_init;
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    b.direct(
        cls,
        "ase",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(6, 0, 0, {
            let mut u = op11n(0x12, 0, 1); // const/4 v0, 1
            u.extend(op22c(0x23, 1, 0, t_strarr)); // new-array v1, [LString;
            u.extend(op11n(0x12, 4, 5)); // const/4 v4, 5
            u.extend(op35c(0x71, 1, m_valueof, [4, 0, 0, 0, 0])); // Integer.valueOf(5)
            u.extend(op11x(0x0C, 5)); // move-result v5
            u.extend(op11n(0x12, 3, 0)); // const/4 v3, 0
            u.extend(op23x(0x4D, 5, 1, 3)); // aput-object v5, v1, v3 → ASE
            u.extend(op11n(0x12, 0, 1));
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    let mut e = engine_of(&b);
    match invoke(&mut e, "ase", "()I", &[]) {
        Err(VmExit::Exception(t)) => {
            assert_eq!(
                t.class, "Ljava/lang/ArrayStoreException;",
                "ASE real: {t:?}"
            )
        }
        other => panic!("esperava ArrayStoreException, got {other:?}"),
    }
}

/// AG-09: cobertura de métodos — denominador = corpos DEX (abstract/native
/// ficam fora), numerador = corpos que executaram neste Engine.
#[test]
fn coverage_snapshot_counts_executed_method_bodies() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCov;", "Ljava/lang/Object;");
    // run()V: return-void (será chamado)
    b.direct(
        cls,
        "run",
        "V",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(1, 0, 0, op10x(0x0e))),
    );
    // never()V: return-void (nunca chamado)
    b.direct(
        cls,
        "never",
        "V",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(1, 0, 0, op10x(0x0e))),
    );
    // abstract (ACC_ABSTRACT 0x400) sem corpo → fora do denominador
    b.direct(
        cls,
        "abs",
        "V",
        vec![],
        ACC_PUBLIC | ACC_STATIC | 0x400,
        None,
    );

    let mut e = engine_of(&b);
    let cov = e.coverage_snapshot();
    assert_eq!(cov.methods_total, 2, "abstract não entra no denominador");
    assert_eq!(cov.methods_executed, 0);
    assert_eq!(cov.classes_total, 1);
    assert_eq!(cov.classes[0].class, "LCov;");

    e.invoke_static("LCov;", "run", "()V", &[]).unwrap();
    let cov = e.coverage_snapshot();
    assert_eq!(cov.methods_executed, 1);
    assert_eq!(cov.classes[0].methods_total, 2);
    assert_eq!(cov.classes[0].methods_executed, 1);
}

/// M8 (compat real): AtomicReference host — gap encontrado no launch do
/// org.primftpd 8.2 (<init>(Object) não resolvido). Estado no campo "value";
/// CAS por identidade de referência; VM single-threaded ⇒ CAS exato.
#[test]
fn atomic_reference_intrinsics() {
    use rd_vm::intrinsics::call_instance_intrinsic;
    use rd_vm::value::Value;
    const AR: &str = "Ljava/util/concurrent/atomic/AtomicReference;";

    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let r = e
        .heap
        .alloc_instance(AR.to_string(), vec![])
        .expect("instância");
    let sa = e.heap.alloc_string("a".into()).unwrap();
    let sb = e.heap.alloc_string("b".into()).unwrap();

    let call = |e: &mut Engine, name: &str, sig: &str, args: Vec<Value>| {
        call_instance_intrinsic(e, AR, name, sig, r, &args).expect("intrínseco")
    };

    // <init>(Object) + get
    call(
        &mut e,
        "<init>",
        "(Ljava/lang/Object;)V",
        vec![Value::Obj(sa)],
    );
    assert_eq!(
        call(&mut e, "get", "()Ljava/lang/Object;", vec![]),
        Some(Value::Obj(sa))
    );

    // compareAndSet com esperado ERRADO → 0, valor intacto
    assert_eq!(
        call(
            &mut e,
            "compareAndSet",
            "(Ljava/lang/Object;Ljava/lang/Object;)Z",
            vec![Value::Obj(sb), Value::Null]
        ),
        Some(Value::Int(0))
    );
    assert_eq!(
        call(&mut e, "get", "()Ljava/lang/Object;", vec![]),
        Some(Value::Obj(sa))
    );

    // compareAndSet com esperado CERTO → 1, valor trocado
    assert_eq!(
        call(
            &mut e,
            "compareAndSet",
            "(Ljava/lang/Object;Ljava/lang/Object;)Z",
            vec![Value::Obj(sa), Value::Obj(sb)]
        ),
        Some(Value::Int(1))
    );
    assert_eq!(
        call(&mut e, "get", "()Ljava/lang/Object;", vec![]),
        Some(Value::Obj(sb))
    );

    // getAndSet devolve o anterior
    assert_eq!(
        call(
            &mut e,
            "getAndSet",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            vec![Value::Obj(sa)]
        ),
        Some(Value::Obj(sb))
    );

    // CAS com Null como esperado/atual (identidade Null==Null)
    call(&mut e, "set", "(Ljava/lang/Object;)V", vec![Value::Null]);
    assert_eq!(
        call(
            &mut e,
            "compareAndSet",
            "(Ljava/lang/Object;Ljava/lang/Object;)Z",
            vec![Value::Null, Value::Obj(sb)]
        ),
        Some(Value::Int(1))
    );

    // <init>()V → Null
    let r2 = e.heap.alloc_instance(AR.to_string(), vec![]).unwrap();
    call_instance_intrinsic(&mut e, AR, "<init>", "()V", r2, &[]).unwrap();
    assert_eq!(
        call_instance_intrinsic(&mut e, AR, "get", "()Ljava/lang/Object;", r2, &[]).unwrap(),
        Some(Value::Null)
    );
}

/// M8 (compat real): HashMap/WeakHashMap host — gap do launch do org.primftpd
/// (`WeakHashMap.<init>`). Chaves String por conteúdo, boxes por valor,
/// senão identidade; put devolve o valor anterior.
#[test]
fn hash_map_host_intrinsics() {
    use rd_vm::intrinsics::call_instance_intrinsic;
    use rd_vm::value::Value;
    const HM: &str = "Ljava/util/HashMap;";

    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let m = e.heap.alloc_instance(HM.to_string(), vec![]).unwrap();
    let call = |e: &mut Engine, name: &str, sig: &str, args: Vec<Value>| {
        call_instance_intrinsic(e, HM, name, sig, m, &args).expect("intrínseco")
    };

    call(&mut e, "<init>", "()V", vec![]);
    assert_eq!(call(&mut e, "size", "()I", vec![]), Some(Value::Int(0)));
    assert_eq!(call(&mut e, "isEmpty", "()Z", vec![]), Some(Value::Int(1)));

    // duas Strings com o MESMO conteúdo são a mesma chave (equals Java)
    let k1 = e.heap.alloc_string("chave".into()).unwrap();
    let k2 = e.heap.alloc_string("chave".into()).unwrap();
    assert_eq!(
        call(
            &mut e,
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            vec![Value::Obj(k1), Value::Int(1)]
        ),
        Some(Value::Null),
        "put novo devolve null"
    );
    assert_eq!(
        call(
            &mut e,
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            vec![Value::Obj(k2), Value::Int(2)]
        ),
        Some(Value::Int(1)),
        "mesma chave (conteúdo) substitui e devolve o anterior"
    );
    assert_eq!(call(&mut e, "size", "()I", vec![]), Some(Value::Int(1)));
    assert_eq!(
        call(
            &mut e,
            "get",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            vec![Value::Obj(k2)]
        ),
        Some(Value::Int(2))
    );
    assert_eq!(
        call(
            &mut e,
            "containsKey",
            "(Ljava/lang/Object;)Z",
            vec![Value::Obj(k1)]
        ),
        Some(Value::Int(1))
    );
    assert_eq!(
        call(
            &mut e,
            "containsValue",
            "(Ljava/lang/Object;)Z",
            vec![Value::Int(1)]
        ),
        Some(Value::Int(0))
    );
    // chave Null
    assert_eq!(
        call(
            &mut e,
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            vec![Value::Null, Value::Int(7)]
        ),
        Some(Value::Null)
    );
    assert_eq!(
        call(
            &mut e,
            "get",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            vec![Value::Null]
        ),
        Some(Value::Int(7))
    );
    // remove devolve o valor e shrinks
    assert_eq!(
        call(
            &mut e,
            "remove",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            vec![Value::Obj(k1)]
        ),
        Some(Value::Int(2))
    );
    assert_eq!(call(&mut e, "size", "()I", vec![]), Some(Value::Int(1)));
    call(&mut e, "clear", "()V", vec![]);
    assert_eq!(call(&mut e, "isEmpty", "()Z", vec![]), Some(Value::Int(1)));
}

/// M8: ArrayList host — add/get/set/indexOf/remove + OOB levanta
/// IndexOutOfBoundsException materializada.
#[test]
fn array_list_host_intrinsics() {
    use rd_vm::intrinsics::call_instance_intrinsic;
    use rd_vm::value::Value;
    const AL: &str = "Ljava/util/ArrayList;";

    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let l = e.heap.alloc_instance(AL.to_string(), vec![]).unwrap();
    let call = |e: &mut Engine, name: &str, sig: &str, args: Vec<Value>| {
        call_instance_intrinsic(e, AL, name, sig, l, &args).expect("intrínseco")
    };

    call(&mut e, "<init>", "()V", vec![]);
    assert_eq!(
        call(&mut e, "add", "(Ljava/lang/Object;)Z", vec![Value::Int(10)]),
        Some(Value::Int(1))
    );
    assert_eq!(
        call(&mut e, "add", "(Ljava/lang/Object;)Z", vec![Value::Int(20)]),
        Some(Value::Int(1))
    );
    assert_eq!(call(&mut e, "size", "()I", vec![]), Some(Value::Int(2)));
    assert_eq!(
        call(&mut e, "get", "(I)Ljava/lang/Object;", vec![Value::Int(1)]),
        Some(Value::Int(20))
    );
    assert_eq!(
        call(
            &mut e,
            "indexOf",
            "(Ljava/lang/Object;)I",
            vec![Value::Int(20)]
        ),
        Some(Value::Int(1))
    );
    assert_eq!(
        call(
            &mut e,
            "indexOf",
            "(Ljava/lang/Object;)I",
            vec![Value::Int(99)]
        ),
        Some(Value::Int(-1))
    );
    assert_eq!(
        call(
            &mut e,
            "contains",
            "(Ljava/lang/Object;)Z",
            vec![Value::Int(10)]
        ),
        Some(Value::Int(1))
    );
    // insert no meio
    call(
        &mut e,
        "add",
        "(ILjava/lang/Object;)V",
        vec![Value::Int(1), Value::Int(15)],
    );
    assert_eq!(
        call(&mut e, "get", "(I)Ljava/lang/Object;", vec![Value::Int(1)]),
        Some(Value::Int(15))
    );
    assert_eq!(call(&mut e, "size", "()I", vec![]), Some(Value::Int(3)));
    // set devolve o anterior
    assert_eq!(
        call(
            &mut e,
            "set",
            "(ILjava/lang/Object;)Ljava/lang/Object;",
            vec![Value::Int(0), Value::Int(11)]
        ),
        Some(Value::Int(10))
    );
    // remove por índice e por objeto
    assert_eq!(
        call(
            &mut e,
            "remove",
            "(I)Ljava/lang/Object;",
            vec![Value::Int(0)]
        ),
        Some(Value::Int(11))
    );
    assert_eq!(
        call(
            &mut e,
            "remove",
            "(Ljava/lang/Object;)Z",
            vec![Value::Int(20)]
        ),
        Some(Value::Int(1))
    );
    assert_eq!(
        call(
            &mut e,
            "remove",
            "(Ljava/lang/Object;)Z",
            vec![Value::Int(20)]
        ),
        Some(Value::Int(0))
    );
    assert_eq!(call(&mut e, "size", "()I", vec![]), Some(Value::Int(1)));

    // OOB → exceção Java materializada (não erro do motor)
    let oob = call_instance_intrinsic(
        &mut e,
        AL,
        "get",
        "(I)Ljava/lang/Object;",
        l,
        &[Value::Int(5)],
    );
    match oob {
        Err(rd_vm::err::VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/IndexOutOfBoundsException;");
        }
        other => panic!("esperava IndexOutOfBoundsException, got {other:?}"),
    }
}

/// M8 (compat real): Enum host — super-chamada `Enum.<init>(String,int)`
/// (mref declarante = Enum) preenche name/ordinal; name()/ordinal() finais e
/// toString() default devolvem o nome. Construtor da classe do app
/// (mref = classe do app) NÃO é sequestrado — pertence ao DEX.
#[test]
fn enum_host_intrinsics() {
    use rd_vm::intrinsics::call_instance_intrinsic_mref;
    use rd_vm::value::Value;
    const ENUM: &str = "Ljava/lang/Enum;";
    const MY: &str = "LMyEnum;";

    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let r = e.heap.alloc_instance(MY.to_string(), vec![]).unwrap();
    let nm = e.heap.alloc_string("CONST_A".into()).unwrap();

    // construtor da classe do app (mref = MY) não é intrínseco
    let hijack = call_instance_intrinsic_mref(
        &mut e,
        MY,
        MY,
        "<init>",
        "(Ljava/lang/String;I)V",
        r,
        &[Value::Obj(nm), Value::Int(0)],
    )
    .unwrap();
    assert!(hijack.is_none(), "construtor do app pertence ao DEX");

    // super-chamada (mref = Enum) preenche o estado
    let v = call_instance_intrinsic_mref(
        &mut e,
        MY,
        ENUM,
        "<init>",
        "(Ljava/lang/String;I)V",
        r,
        &[Value::Obj(nm), Value::Int(3)],
    )
    .unwrap();
    assert_eq!(v, Some(Value::Null));
    assert_eq!(
        call_instance_intrinsic_mref(&mut e, MY, ENUM, "name", "()Ljava/lang/String;", r, &[])
            .unwrap(),
        Some(Value::Obj(nm))
    );
    assert_eq!(
        call_instance_intrinsic_mref(&mut e, MY, ENUM, "ordinal", "()I", r, &[]).unwrap(),
        Some(Value::Int(3))
    );
    assert_eq!(
        call_instance_intrinsic_mref(&mut e, MY, ENUM, "toString", "()Ljava/lang/String;", r, &[])
            .unwrap(),
        Some(Value::Obj(nm))
    );
}

/// M8 (compat real): família java.lang.ref host — quarto gap do launch do
/// org.primftpd (`WeakReference.<init>(Object)`). Sem GC na arena: o referent
/// vive até `clear()`; ReferenceQueue está permanentemente vazia (poll → null).
#[test]
fn weak_soft_reference_host_intrinsics() {
    use rd_vm::intrinsics::{call_instance_intrinsic, call_instance_intrinsic_mref};
    use rd_vm::value::Value;
    const WR: &str = "Ljava/lang/ref/WeakReference;";
    const SR: &str = "Ljava/lang/ref/SoftReference;";
    const RQ: &str = "Ljava/lang/ref/ReferenceQueue;";

    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let r = e.heap.alloc_instance(WR.to_string(), vec![]).unwrap();
    let payload = e.heap.alloc_string("alvo".into()).unwrap();

    // <init>(Object) + get devolve o referent
    let v = call_instance_intrinsic(
        &mut e,
        WR,
        "<init>",
        "(Ljava/lang/Object;)V",
        r,
        &[Value::Obj(payload)],
    )
    .unwrap();
    assert_eq!(v, Some(Value::Null));
    assert_eq!(
        call_instance_intrinsic(&mut e, WR, "get", "()Ljava/lang/Object;", r, &[]).unwrap(),
        Some(Value::Obj(payload))
    );
    // clear() → get devolve null (única forma de limpar: arena não tem GC)
    call_instance_intrinsic(&mut e, WR, "clear", "()V", r, &[]).unwrap();
    assert_eq!(
        call_instance_intrinsic(&mut e, WR, "get", "()Ljava/lang/Object;", r, &[]).unwrap(),
        Some(Value::Null)
    );

    // <init>() vazio e <init>(Object, ReferenceQueue) — fila ignorada
    let r2 = e.heap.alloc_instance(SR.to_string(), vec![]).unwrap();
    call_instance_intrinsic(&mut e, SR, "<init>", "()V", r2, &[]).unwrap();
    assert_eq!(
        call_instance_intrinsic(&mut e, SR, "get", "()Ljava/lang/Object;", r2, &[]).unwrap(),
        Some(Value::Null)
    );
    let rq = e.heap.alloc_instance(RQ.to_string(), vec![]).unwrap();
    call_instance_intrinsic(&mut e, RQ, "<init>", "()V", rq, &[]).unwrap();
    assert_eq!(
        call_instance_intrinsic(
            &mut e,
            SR,
            "<init>",
            "(Ljava/lang/Object;Ljava/lang/ref/ReferenceQueue;)V",
            r2,
            &[Value::Obj(payload), Value::Obj(rq)]
        )
        .unwrap(),
        Some(Value::Null)
    );
    assert_eq!(
        call_instance_intrinsic(&mut e, SR, "get", "()Ljava/lang/Object;", r2, &[]).unwrap(),
        Some(Value::Obj(payload))
    );
    // fila permanentemente vazia (sem GC ⇒ nada é enfileirado)
    assert_eq!(
        call_instance_intrinsic(&mut e, RQ, "poll", "()Ljava/lang/ref/Reference;", rq, &[])
            .unwrap(),
        Some(Value::Null)
    );

    // subclasse do app: super-chamada (mref = WeakReference) cai no host…
    let sub = e
        .heap
        .alloc_instance("LMyRef;".to_string(), vec![])
        .unwrap();
    let v = call_instance_intrinsic_mref(
        &mut e,
        "LMyRef;",
        WR,
        "<init>",
        "(Ljava/lang/Object;)V",
        sub,
        &[Value::Obj(payload)],
    )
    .unwrap();
    assert_eq!(v, Some(Value::Null));
    assert_eq!(
        call_instance_intrinsic_mref(
            &mut e,
            "LMyRef;",
            WR,
            "get",
            "()Ljava/lang/Object;",
            sub,
            &[]
        )
        .unwrap(),
        Some(Value::Obj(payload))
    );
    // …mas <init> declarado pela própria subclasse (mref = LMyRef;) é DEX
    let hijack = call_instance_intrinsic_mref(
        &mut e,
        "LMyRef;",
        "LMyRef;",
        "<init>",
        "(Ljava/lang/Object;)V",
        sub,
        &[Value::Obj(payload)],
    )
    .unwrap();
    assert!(hijack.is_none(), "construtor da subclasse pertence ao DEX");
}

/// M8 (compat real): AtomicReferenceFieldUpdater host — quinto gap do launch
/// do org.primftpd (newUpdater caía no NOT_IMPLEMENTED do catch-all Ljava/).
/// newUpdater captura o nome do campo; get/set/getAndSet/CAS operam no alvo.
#[test]
fn atomic_field_updater_host_intrinsics() {
    use rd_vm::intrinsics::{call_instance_intrinsic_mref, call_static_intrinsic};
    use rd_vm::value::Value;
    const ARFU: &str = "Ljava/util/concurrent/atomic/AtomicReferenceFieldUpdater;";
    const NEWSIG: &str =
        "(Ljava/lang/Class;Ljava/lang/Class;Ljava/lang/String;)Ljava/util/concurrent/atomic/AtomicReferenceFieldUpdater;";

    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let fname = e.heap.alloc_string("holder".into()).unwrap();
    let up = call_static_intrinsic(
        &mut e,
        ARFU,
        "newUpdater",
        NEWSIG,
        &[Value::Null, Value::Null, Value::Obj(fname)],
    )
    .unwrap();
    let up = match up {
        Some(Value::Obj(r)) => r,
        other => panic!("updater: {other:?}"),
    };

    let target = e.heap.alloc_instance("LApp;".to_string(), vec![]).unwrap();
    let s1 = e.heap.alloc_string("v1".into()).unwrap();
    let s2 = e.heap.alloc_string("v2".into()).unwrap();
    let call = |e: &mut Engine, name: &str, sig: &str, args: Vec<Value>| {
        call_instance_intrinsic_mref(e, ARFU, ARFU, name, sig, up, &args).expect("intrínseco")
    };

    // get com campo ainda ausente → null
    assert_eq!(
        call(
            &mut e,
            "get",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            vec![Value::Obj(target)]
        ),
        Some(Value::Null)
    );
    // set + get
    call(
        &mut e,
        "set",
        "(Ljava/lang/Object;Ljava/lang/Object;)V",
        vec![Value::Obj(target), Value::Obj(s1)],
    );
    assert_eq!(
        call(
            &mut e,
            "get",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            vec![Value::Obj(target)]
        ),
        Some(Value::Obj(s1))
    );
    // CAS errado → 0 (valor intacto); certo → 1 (troca)
    assert_eq!(
        call(
            &mut e,
            "compareAndSet",
            "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Z",
            vec![Value::Obj(target), Value::Obj(s2), Value::Null]
        ),
        Some(Value::Int(0))
    );
    assert_eq!(
        call(
            &mut e,
            "compareAndSet",
            "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Z",
            vec![Value::Obj(target), Value::Obj(s1), Value::Obj(s2)]
        ),
        Some(Value::Int(1))
    );
    // getAndSet devolve o anterior
    assert_eq!(
        call(
            &mut e,
            "getAndSet",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            vec![Value::Obj(target), Value::Obj(s1)]
        ),
        Some(Value::Obj(s2))
    );
    // alvo null → NPE materializado (semântica Java)
    let npe = call_instance_intrinsic_mref(
        &mut e,
        ARFU,
        ARFU,
        "get",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        up,
        &[Value::Null],
    );
    match npe {
        Err(rd_vm::err::VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/NullPointerException;")
        }
        other => panic!("esperava NPE, got {other:?}"),
    }
    // fieldName null → NPE no newUpdater
    let npe2 = call_static_intrinsic(
        &mut e,
        ARFU,
        "newUpdater",
        NEWSIG,
        &[Value::Null, Value::Null, Value::Null],
    );
    assert!(
        matches!(npe2, Err(rd_vm::err::VmExit::Exception(_))),
        "newUpdater(null) → NPE: {npe2:?}"
    );
}

#[test]
fn atomic_integer_field_updater_host_intrinsics() {
    use rd_vm::intrinsics::{call_instance_intrinsic_mref, call_static_intrinsic};
    use rd_vm::value::Value;
    const AIFU: &str = "Ljava/util/concurrent/atomic/AtomicIntegerFieldUpdater;";
    const NEWSIG: &str =
        "(Ljava/lang/Class;Ljava/lang/String;)Ljava/util/concurrent/atomic/AtomicIntegerFieldUpdater;";
    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let name = e.heap.alloc_string("count".into()).unwrap();
    let updater = call_static_intrinsic(
        &mut e,
        AIFU,
        "newUpdater",
        NEWSIG,
        &[Value::Null, Value::Obj(name)],
    )
    .unwrap()
    .unwrap();
    let Value::Obj(updater) = updater else {
        panic!("updater object")
    };
    let target = e
        .heap
        .alloc_instance("LTarget;".into(), vec![("count".into(), Value::Int(3))])
        .unwrap();
    assert_eq!(
        call_instance_intrinsic_mref(
            &mut e,
            AIFU,
            AIFU,
            "compareAndSet",
            "(Ljava/lang/Object;II)Z",
            updater,
            &[Value::Obj(target), Value::Int(3), Value::Int(9)],
        )
        .unwrap(),
        Some(Value::Int(1))
    );
    assert_eq!(
        call_instance_intrinsic_mref(
            &mut e,
            AIFU,
            AIFU,
            "getAndAdd",
            "(Ljava/lang/Object;I)I",
            updater,
            &[Value::Obj(target), Value::Int(4)],
        )
        .unwrap(),
        Some(Value::Int(9))
    );
    assert_eq!(e.heap.get_field(target, "count").unwrap(), Value::Int(13));
}

/// M8 (compat real): coleções concorrentes host — sexto gap do launch do
/// org.primftpd (`CopyOnWriteArraySet.<init>`). VM single-threaded ⇒
/// cópia-em-escrita irrelevante: Set deduplica no add; COWArrayList ≡ ArrayList.
#[test]
fn cow_collections_host_intrinsics() {
    use rd_vm::intrinsics::call_instance_intrinsic;
    use rd_vm::value::Value;
    const CS: &str = "Ljava/util/concurrent/CopyOnWriteArraySet;";
    const CL: &str = "Ljava/util/concurrent/CopyOnWriteArrayList;";

    let b = DexBuilder::new();
    let mut e = engine_of(&b);

    // ── CopyOnWriteArraySet: add deduplica por igualdade Java ──
    let st = e.heap.alloc_instance(CS.to_string(), vec![]).unwrap();
    let call_s = |e: &mut Engine, name: &str, sig: &str, args: Vec<Value>| {
        call_instance_intrinsic(e, CS, name, sig, st, &args).expect("intrínseco")
    };
    call_s(&mut e, "<init>", "()V", vec![]);
    assert_eq!(
        call_s(&mut e, "isEmpty", "()Z", vec![]),
        Some(Value::Int(1))
    );
    let s1 = e.heap.alloc_string("x".into()).unwrap();
    let s1b = e.heap.alloc_string("x".into()).unwrap(); // mesmo conteúdo
    assert_eq!(
        call_s(&mut e, "add", "(Ljava/lang/Object;)Z", vec![Value::Obj(s1)]),
        Some(Value::Int(1))
    );
    assert_eq!(
        call_s(
            &mut e,
            "add",
            "(Ljava/lang/Object;)Z",
            vec![Value::Obj(s1b)]
        ),
        Some(Value::Int(0)),
        "Set não duplica: add de elemento igual → false"
    );
    assert_eq!(call_s(&mut e, "size", "()I", vec![]), Some(Value::Int(1)));
    assert_eq!(
        call_s(
            &mut e,
            "contains",
            "(Ljava/lang/Object;)Z",
            vec![Value::Obj(s1b)]
        ),
        Some(Value::Int(1))
    );
    assert_eq!(
        call_s(
            &mut e,
            "remove",
            "(Ljava/lang/Object;)Z",
            vec![Value::Obj(s1)]
        ),
        Some(Value::Int(1))
    );
    assert_eq!(call_s(&mut e, "size", "()I", vec![]), Some(Value::Int(0)));

    // ── CopyOnWriteArrayList ≡ ArrayList plus the legacy addIfAbsent API ──
    let lt = e.heap.alloc_instance(CL.to_string(), vec![]).unwrap();
    let call_l = |e: &mut Engine, name: &str, sig: &str, args: Vec<Value>| {
        call_instance_intrinsic(e, CL, name, sig, lt, &args).expect("intrínseco")
    };
    call_l(&mut e, "<init>", "()V", vec![]);
    assert_eq!(
        call_l(&mut e, "add", "(Ljava/lang/Object;)Z", vec![Value::Int(7)]),
        Some(Value::Int(1))
    );
    assert_eq!(
        call_l(&mut e, "add", "(Ljava/lang/Object;)Z", vec![Value::Int(7)]),
        Some(Value::Int(1)),
        "List aceita duplicatas (diferente do Set)"
    );
    assert_eq!(call_l(&mut e, "size", "()I", vec![]), Some(Value::Int(2)));
    assert_eq!(
        call_l(&mut e, "get", "(I)Ljava/lang/Object;", vec![Value::Int(1)]),
        Some(Value::Int(7))
    );
    assert_eq!(
        call_l(
            &mut e,
            "indexOf",
            "(Ljava/lang/Object;)I",
            vec![Value::Int(7)]
        ),
        Some(Value::Int(0))
    );
    assert_eq!(
        call_l(
            &mut e,
            "addIfAbsent",
            "(Ljava/lang/Object;)Z",
            vec![Value::Int(7)]
        ),
        Some(Value::Int(0)),
        "addIfAbsent keeps the existing equal element"
    );
    assert_eq!(
        call_l(
            &mut e,
            "addIfAbsent",
            "(Ljava/lang/Object;)Z",
            vec![Value::Int(8)]
        ),
        Some(Value::Int(1))
    );
    assert_eq!(call_l(&mut e, "size", "()I", vec![]), Some(Value::Int(3)));
    call_l(&mut e, "clear", "()V", vec![]);
    assert_eq!(
        call_l(&mut e, "isEmpty", "()Z", vec![]),
        Some(Value::Int(1))
    );
}

/// M8 (compat real): LinkedHashMap host — gap do launch do org.primftpd.
/// Os métodos essenciais acompanham HashMap e as views preservam ordem de
/// inserção; o construtor access-order tem regressão semântica separada.
#[test]
fn linked_hash_map_host_intrinsics() {
    use rd_vm::intrinsics::call_instance_intrinsic;
    use rd_vm::value::Value;
    const LHM: &str = "Ljava/util/LinkedHashMap;";

    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let m = e.heap.alloc_instance(LHM.to_string(), vec![]).unwrap();
    let call = |e: &mut Engine, name: &str, sig: &str, args: Vec<Value>| {
        call_instance_intrinsic(e, LHM, name, sig, m, &args).expect("intrínseco")
    };

    call(&mut e, "<init>", "()V", vec![]);
    assert_eq!(call(&mut e, "isEmpty", "()Z", vec![]), Some(Value::Int(1)));
    let k = e.heap.alloc_string("chave".into()).unwrap();
    let v = e.heap.alloc_string("valor".into()).unwrap();
    assert_eq!(
        call(
            &mut e,
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            vec![Value::Obj(k), Value::Obj(v)]
        ),
        Some(Value::Null),
        "put de chave nova → null (anterior)"
    );
    assert_eq!(call(&mut e, "size", "()I", vec![]), Some(Value::Int(1)));
    assert_eq!(
        call(
            &mut e,
            "get",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            vec![Value::Obj(k)]
        ),
        Some(Value::Obj(v))
    );
    assert_eq!(
        call(
            &mut e,
            "containsKey",
            "(Ljava/lang/Object;)Z",
            vec![Value::Obj(k)]
        ),
        Some(Value::Int(1))
    );
    // subtipagem: LinkedHashMap <: HashMap <: AbstractMap e <: Map
    assert!(e.cp.is_subtype(LHM, "Ljava/util/HashMap;"));
    assert!(e.cp.is_subtype(LHM, "Ljava/util/AbstractMap;"));
    assert!(e.cp.is_subtype(LHM, "Ljava/util/Map;"));
    assert!(!e.cp.is_subtype(LHM, "Ljava/util/List;"));
    // remove devolve o valor removido
    assert_eq!(
        call(
            &mut e,
            "remove",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            vec![Value::Obj(k)]
        ),
        Some(Value::Obj(v))
    );
    assert_eq!(call(&mut e, "size", "()I", vec![]), Some(Value::Int(0)));
}

#[test]
fn linked_hash_map_access_order_constructor_and_validation() {
    use rd_vm::intrinsics::call_instance_intrinsic;
    use rd_vm::value::Value;
    const LHM: &str = "Ljava/util/LinkedHashMap;";
    const INIT_ACCESS: &str = "(IFZ)V";
    const PUT: &str = "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
    const GET: &str = "(Ljava/lang/Object;)Ljava/lang/Object;";

    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let m = e.heap.alloc_instance(LHM.to_string(), vec![]).unwrap();
    let call = |e: &mut Engine, name: &str, sig: &str, args: Vec<Value>| {
        call_instance_intrinsic(e, LHM, name, sig, m, &args)
    };
    call(
        &mut e,
        "<init>",
        INIT_ACCESS,
        vec![Value::Int(8), Value::Float(0.75), Value::Int(1)],
    )
    .unwrap();
    let k1 = e.heap.alloc_string("first".into()).unwrap();
    let k2 = e.heap.alloc_string("second".into()).unwrap();
    call(&mut e, "put", PUT, vec![Value::Obj(k1), Value::Int(1)]).unwrap();
    call(&mut e, "put", PUT, vec![Value::Obj(k2), Value::Int(2)]).unwrap();
    assert_eq!(
        call(&mut e, "get", GET, vec![Value::Obj(k1)]).unwrap(),
        Some(Value::Int(1))
    );
    let keys = call(&mut e, "keySet", "()Ljava/util/Set;", vec![])
        .unwrap()
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    assert_eq!(
        e.heap.get_field(keys, "rd$key0").unwrap(),
        Value::Obj(k2),
        "get(first) moves first to the tail in access-order mode"
    );
    assert_eq!(e.heap.get_field(keys, "rd$key1").unwrap(), Value::Obj(k1));

    let invalid = e.heap.alloc_instance(LHM.to_string(), vec![]).unwrap();
    let result = call_instance_intrinsic(
        &mut e,
        LHM,
        "<init>",
        INIT_ACCESS,
        invalid,
        &[Value::Int(-1), Value::Float(0.75), Value::Int(0)],
    );
    match result {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/IllegalArgumentException;");
        }
        other => panic!("negative initial capacity must throw IllegalArgumentException: {other:?}"),
    }
}

/// M8 (compat real): LinkedHashSet host — 25º gap do launch de org.primftpd.
/// Reusa Set/List, mas preserva a ordem de inserção como a classe JDK.
#[test]
fn linked_hash_set_host_intrinsics() {
    use rd_vm::intrinsics::call_instance_intrinsic;
    use rd_vm::value::Value;
    const LHS: &str = "Ljava/util/LinkedHashSet;";
    const IT: &str = "Ljava/util/Iterator;";

    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let set = e.heap.alloc_instance(LHS.to_string(), vec![]).unwrap();
    let call = |e: &mut Engine, name: &str, sig: &str, args: Vec<Value>| {
        call_instance_intrinsic(e, LHS, name, sig, set, &args).expect("intrínseco")
    };

    call(&mut e, "<init>", "()V", vec![]);
    let first = e.heap.alloc_string("first".into()).unwrap();
    let duplicate = e.heap.alloc_string("first".into()).unwrap();
    let second = e.heap.alloc_string("second".into()).unwrap();
    assert_eq!(
        call(
            &mut e,
            "add",
            "(Ljava/lang/Object;)Z",
            vec![Value::Obj(first)]
        ),
        Some(Value::Int(1))
    );
    assert_eq!(
        call(
            &mut e,
            "add",
            "(Ljava/lang/Object;)Z",
            vec![Value::Obj(second)]
        ),
        Some(Value::Int(1))
    );
    assert_eq!(
        call(
            &mut e,
            "add",
            "(Ljava/lang/Object;)Z",
            vec![Value::Obj(duplicate)]
        ),
        Some(Value::Int(0)),
        "valor igual não pode duplicar no Set"
    );
    assert_eq!(call(&mut e, "size", "()I", vec![]), Some(Value::Int(2)));
    assert_eq!(
        call(
            &mut e,
            "contains",
            "(Ljava/lang/Object;)Z",
            vec![Value::Obj(duplicate)]
        ),
        Some(Value::Int(1))
    );

    let ordered_values = |e: &mut Engine| -> Vec<String> {
        let iterator =
            match call_instance_intrinsic(e, LHS, "iterator", "()Ljava/util/Iterator;", set, &[])
                .unwrap()
            {
                Some(Value::Obj(r)) => r,
                other => panic!("iterator: {other:?}"),
            };
        let mut values = Vec::new();
        while call_instance_intrinsic(e, IT, "hasNext", "()Z", iterator, &[]).unwrap()
            == Some(Value::Int(1))
        {
            let value =
                call_instance_intrinsic(e, IT, "next", "()Ljava/lang/Object;", iterator, &[])
                    .unwrap();
            match value {
                Some(Value::Obj(r)) => values.push(e.heap.as_str(r).unwrap().to_string()),
                other => panic!("iterator next: {other:?}"),
            }
        }
        values
    };
    assert_eq!(ordered_values(&mut e), vec!["first", "second"]);

    // Remover e inserir novamente desloca o elemento para o fim (JDK).
    assert_eq!(
        call(
            &mut e,
            "remove",
            "(Ljava/lang/Object;)Z",
            vec![Value::Obj(first)]
        ),
        Some(Value::Int(1))
    );
    assert_eq!(
        call(
            &mut e,
            "add",
            "(Ljava/lang/Object;)Z",
            vec![Value::Obj(first)]
        ),
        Some(Value::Int(1))
    );
    assert_eq!(ordered_values(&mut e), vec!["second", "first"]);

    assert!(e.cp.is_subtype(LHS, "Ljava/util/HashSet;"));
    assert!(e.cp.is_subtype(LHS, "Ljava/util/AbstractSet;"));
    assert!(e.cp.is_subtype(LHS, "Ljava/util/Set;"));
    assert!(e.cp.is_subtype(LHS, "Ljava/util/Collection;"));
    assert!(e.cp.is_subtype(LHS, "Ljava/lang/Iterable;"));
    assert!(!e.cp.is_subtype(LHS, "Ljava/util/List;"));
}

/// M8 (compat real): AtomicInteger host — nono gap do launch do org.primftpd
/// (`<init>()V`). Mesmo padrão do AtomicReference (campo "value"); aritmética
/// i32 com overflow wrapping (semântica Java); CAS exato (VM single-threaded).
#[test]
fn atomic_integer_intrinsics() {
    use rd_vm::intrinsics::call_instance_intrinsic;
    use rd_vm::value::Value;
    const AI: &str = "Ljava/util/concurrent/atomic/AtomicInteger;";

    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let a = e.heap.alloc_instance(AI.to_string(), vec![]).unwrap();
    let call = |e: &mut Engine, name: &str, sig: &str, args: Vec<Value>| {
        call_instance_intrinsic(e, AI, name, sig, a, &args).expect("intrínseco")
    };

    // <init>()V → 0; get/set
    call(&mut e, "<init>", "()V", vec![]);
    assert_eq!(call(&mut e, "get", "()I", vec![]), Some(Value::Int(0)));
    call(&mut e, "set", "(I)V", vec![Value::Int(5)]);
    assert_eq!(call(&mut e, "get", "()I", vec![]), Some(Value::Int(5)));

    // increment/decrement (get- e -and-set variants)
    assert_eq!(
        call(&mut e, "incrementAndGet", "()I", vec![]),
        Some(Value::Int(6))
    );
    assert_eq!(
        call(&mut e, "getAndIncrement", "()I", vec![]),
        Some(Value::Int(6))
    );
    assert_eq!(call(&mut e, "get", "()I", vec![]), Some(Value::Int(7)));
    assert_eq!(
        call(&mut e, "decrementAndGet", "()I", vec![]),
        Some(Value::Int(6))
    );
    assert_eq!(
        call(&mut e, "getAndDecrement", "()I", vec![]),
        Some(Value::Int(6))
    );
    assert_eq!(call(&mut e, "get", "()I", vec![]), Some(Value::Int(5)));

    // addAndGet/getAndAdd
    assert_eq!(
        call(&mut e, "getAndAdd", "(I)I", vec![Value::Int(10)]),
        Some(Value::Int(5))
    );
    assert_eq!(
        call(&mut e, "addAndGet", "(I)I", vec![Value::Int(-3)]),
        Some(Value::Int(12))
    );

    // CAS exato
    assert_eq!(
        call(
            &mut e,
            "compareAndSet",
            "(II)Z",
            vec![Value::Int(99), Value::Int(1)]
        ),
        Some(Value::Int(0))
    );
    assert_eq!(
        call(
            &mut e,
            "compareAndSet",
            "(II)Z",
            vec![Value::Int(12), Value::Int(1)]
        ),
        Some(Value::Int(1))
    );
    assert_eq!(
        call(&mut e, "getAndSet", "(I)I", vec![Value::Int(42)]),
        Some(Value::Int(1))
    );

    // overflow wrapping (semântica Java)
    call(&mut e, "set", "(I)V", vec![Value::Int(i32::MAX)]);
    assert_eq!(
        call(&mut e, "incrementAndGet", "()I", vec![]),
        Some(Value::Int(i32::MIN))
    );

    // conversões Number + toString
    call(&mut e, "set", "(I)V", vec![Value::Int(42)]);
    assert_eq!(
        call(&mut e, "intValue", "()I", vec![]),
        Some(Value::Int(42))
    );
    assert_eq!(
        call(&mut e, "longValue", "()J", vec![]),
        Some(Value::Long(42))
    );
    assert_eq!(
        call(&mut e, "floatValue", "()F", vec![]),
        Some(Value::Float(42.0))
    );
    assert_eq!(
        call(&mut e, "doubleValue", "()D", vec![]),
        Some(Value::Double(42.0))
    );
    let t = match call(&mut e, "toString", "()Ljava/lang/String;", vec![]) {
        Some(Value::Obj(r)) => r,
        other => panic!("toString: {other:?}"),
    };
    assert_eq!(e.heap.as_str(t).unwrap(), "42");

    // <init>(I)V em instância nova
    let a2 = e.heap.alloc_instance(AI.to_string(), vec![]).unwrap();
    call_instance_intrinsic(&mut e, AI, "<init>", "(I)V", a2, &[Value::Int(7)]).unwrap();
    assert_eq!(
        call_instance_intrinsic(&mut e, AI, "get", "()I", a2, &[]).unwrap(),
        Some(Value::Int(7))
    );

    // subtipagem: AtomicInteger <: Number; Serializable
    assert!(e.cp.is_subtype(AI, "Ljava/lang/Number;"));
    assert!(e.cp.is_subtype(AI, "Ljava/io/Serializable;"));
}

/// M8 (compat real): Executors/ExecutorService host — décimo gap do launch do
/// org.primftpd (`newFixedThreadPool(I,ThreadFactory)`). VM single-threaded ⇒
/// execução INLINE determinística: execute/submit rodam na hora; submit devolve
/// Future já completado; pool/ThreadFactory ignorados.
#[test]
fn executors_host_inline_execution() {
    use rd_vm::intrinsics::{call_instance_intrinsic, call_static_intrinsic};
    use rd_vm::value::Value;
    const EXECUTORS: &str = "Ljava/util/concurrent/Executors;";
    const SVC: &str = "Ljava/util/concurrent/ExecutorService;";
    const FUT: &str = "Ljava/util/concurrent/Future;";

    // LTickX: run() { t++ } · call() { t += 10; return "ok-call" }
    let mut b = DexBuilder::new();
    let tick = b.class("LTickX;", "Ljava/lang/Object;");
    let _ft = b.field_idx("LTickX;", "I", "t");
    b.static_field(tick, "t", "I", SVal::Int(0));
    let f_t = b.find_field("LTickX;", "I", "t") as u16;
    b.direct(
        tick,
        "run",
        "V",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 0, {
            let mut u = op21c(0x60, 0, f_t); // sget v0, t
            u.extend(op22b(0xD8, 0, 0, 1)); // add-int/lit8 v0, v0, 1
            u.extend(op21c(0x67, 0, f_t)); // sput t, v0
            u.extend(op10x(0x0E)); // return-void
            u
        })),
    );
    let s_ok = b.intern("ok-call") as u16;
    b.direct(
        tick,
        "call",
        "Ljava/lang/Object;",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 0, {
            let mut u = op21c(0x60, 0, f_t); // sget v0, t
            u.extend(op22b(0xD8, 0, 0, 10)); // add-int/lit8 v0, v0, 10
            u.extend(op21c(0x67, 0, f_t)); // sput t, v0
            u.extend(op21c(0x1A, 0, s_ok)); // const-string v0, "ok-call"
            u.extend(op11x(0x11, 0)); // return v0
            u
        })),
    );

    let mut e = engine_of(&b);
    let t_of = |e: &Engine| {
        e.statics
            .get(&("LTickX;".to_string(), "t".to_string()))
            .cloned()
    };
    let tick_inst = e
        .heap
        .alloc_instance("LTickX;".to_string(), vec![])
        .unwrap();
    let time_unit = e
        .heap
        .alloc_instance("Ljava/util/concurrent/TimeUnit;".to_string(), vec![])
        .unwrap();

    // newFixedThreadPool(I, ThreadFactory) → executor host
    let ex = call_static_intrinsic(
        &mut e,
        EXECUTORS,
        "newFixedThreadPool",
        "(ILjava/util/concurrent/ThreadFactory;)Ljava/util/concurrent/ExecutorService;",
        &[Value::Int(4), Value::Null],
    )
    .unwrap();
    let ex = match ex {
        Some(Value::Obj(r)) => r,
        other => panic!("executor: {other:?}"),
    };
    assert_eq!(
        call_instance_intrinsic(&mut e, SVC, "isShutdown", "()Z", ex, &[]).unwrap(),
        Some(Value::Int(0))
    );
    assert_eq!(
        call_instance_intrinsic(
            &mut e,
            SVC,
            "awaitTermination",
            "(JLjava/util/concurrent/TimeUnit;)Z",
            ex,
            &[Value::Long(0), Value::Obj(time_unit)],
        )
        .unwrap(),
        Some(Value::Int(0)),
        "an open executor has not terminated"
    );

    // execute roda INLINE (síncrono — t já vale 1 na volta)
    call_instance_intrinsic(
        &mut e,
        SVC,
        "execute",
        "(Ljava/lang/Runnable;)V",
        ex,
        &[Value::Obj(tick_inst)],
    )
    .unwrap();
    assert_eq!(t_of(&e), Some(Value::Int(1)), "execute inline: t=1");

    // submit(Runnable) → Future já completado com null
    let f = call_instance_intrinsic(
        &mut e,
        SVC,
        "submit",
        "(Ljava/lang/Runnable;)Ljava/util/concurrent/Future;",
        ex,
        &[Value::Obj(tick_inst)],
    )
    .unwrap();
    assert_eq!(
        t_of(&e),
        Some(Value::Int(2)),
        "submit(Runnable) inline: t=2"
    );
    let f = match f {
        Some(Value::Obj(r)) => r,
        other => panic!("future: {other:?}"),
    };
    assert_eq!(
        call_instance_intrinsic(&mut e, FUT, "isDone", "()Z", f, &[]).unwrap(),
        Some(Value::Int(1))
    );
    assert_eq!(
        call_instance_intrinsic(&mut e, FUT, "isCancelled", "()Z", f, &[]).unwrap(),
        Some(Value::Int(0))
    );
    assert_eq!(
        call_instance_intrinsic(&mut e, FUT, "get", "()Ljava/lang/Object;", f, &[]).unwrap(),
        Some(Value::Null)
    );
    assert_eq!(
        call_instance_intrinsic(&mut e, FUT, "cancel", "(Z)Z", f, &[Value::Int(1)]).unwrap(),
        Some(Value::Int(0)),
        "Future já completado não é cancelável"
    );

    // submit(Callable) → roda call() e captura o resultado no Future
    let f2 = call_instance_intrinsic(
        &mut e,
        SVC,
        "submit",
        "(Ljava/util/concurrent/Callable;)Ljava/util/concurrent/Future;",
        ex,
        &[Value::Obj(tick_inst)],
    )
    .unwrap();
    assert_eq!(
        t_of(&e),
        Some(Value::Int(12)),
        "submit(Callable) inline: t=12"
    );
    let f2 = match f2 {
        Some(Value::Obj(r)) => r,
        other => panic!("future2: {other:?}"),
    };
    let got = call_instance_intrinsic(&mut e, FUT, "get", "()Ljava/lang/Object;", f2, &[]).unwrap();
    match got {
        Some(Value::Obj(r)) => assert_eq!(e.heap.as_str(r).unwrap(), "ok-call"),
        other => panic!("get do Callable: {other:?}"),
    }

    // submit(Runnable, result) → Future completado com o resultado dado
    let f3 = call_instance_intrinsic(
        &mut e,
        SVC,
        "submit",
        "(Ljava/lang/Runnable;Ljava/lang/Object;)Ljava/util/concurrent/Future;",
        ex,
        &[Value::Obj(tick_inst), Value::Int(99)],
    )
    .unwrap();
    let f3 = match f3 {
        Some(Value::Obj(r)) => r,
        other => panic!("future3: {other:?}"),
    };
    assert_eq!(
        call_instance_intrinsic(&mut e, FUT, "get", "()Ljava/lang/Object;", f3, &[]).unwrap(),
        Some(Value::Int(99))
    );

    // shutdown → isShutdown/isTerminated; shutdownNow → lista vazia
    call_instance_intrinsic(&mut e, SVC, "shutdown", "()V", ex, &[]).unwrap();
    assert_eq!(
        call_instance_intrinsic(&mut e, SVC, "isShutdown", "()Z", ex, &[]).unwrap(),
        Some(Value::Int(1))
    );
    assert_eq!(
        call_instance_intrinsic(&mut e, SVC, "isTerminated", "()Z", ex, &[]).unwrap(),
        Some(Value::Int(1))
    );
    assert_eq!(
        call_instance_intrinsic(
            &mut e,
            SVC,
            "awaitTermination",
            "(JLjava/util/concurrent/TimeUnit;)Z",
            ex,
            &[Value::Long(0), Value::Obj(time_unit)],
        )
        .unwrap(),
        Some(Value::Int(1)),
        "all inline work has finished when the executor is shut down"
    );
    let pending =
        call_instance_intrinsic(&mut e, SVC, "shutdownNow", "()Ljava/util/List;", ex, &[]).unwrap();
    match pending {
        Some(Value::Obj(l)) => {
            assert_eq!(
                call_instance_intrinsic(&mut e, "Ljava/util/ArrayList;", "size", "()I", l, &[])
                    .unwrap(),
                Some(Value::Int(0)),
                "execução inline ⇒ nada pendente"
            );
        }
        other => panic!("shutdownNow: {other:?}"),
    }

    // subtipagem: ExecutorService <: Executor
    assert!(e.cp.is_subtype(SVC, "Ljava/util/concurrent/Executor;"));
}

/// M8 (compat real): Enum.compareTo + herança de métodos do Enum host por
/// enums do APP — 12º gap do launch do org.primftpd
/// (`Lifecycle$State.compareTo(Ljava/lang/Enum;)I`): o method ref declara a
/// SUBCLASSE DEX, Enum não tem DEX ⇒ resolução falhava. Fallback por
/// subtipagem (recv <: Enum, sem declaração DEX) + compareTo por ordinais.
#[test]
fn enum_inherited_methods_and_compareto() {
    use rd_vm::intrinsics::call_instance_intrinsic_mref;
    use rd_vm::value::Value;
    const ENUM: &str = "Ljava/lang/Enum;";
    const MY: &str = "LMyEnum;";

    let mut b = DexBuilder::new();
    b.class(MY, ENUM);
    b.class("LNotEnum;", "Ljava/lang/Object;");
    let mut e = engine_of(&b);

    // duas constantes do enum "app": A(ordinal 0), B(ordinal 1)
    let mk = |e: &mut Engine, name: &str, ord: i32| {
        let r = e.heap.alloc_instance(MY.to_string(), vec![]).unwrap();
        let s = e.heap.alloc_string(name.into()).unwrap();
        call_instance_intrinsic_mref(
            e,
            MY,
            ENUM,
            "<init>",
            "(Ljava/lang/String;I)V",
            r,
            &[Value::Obj(s), Value::Int(ord)],
        )
        .expect("enum <init>");
        r
    };
    let a = mk(&mut e, "A", 0);
    let bb = mk(&mut e, "B", 1);

    // chamada herdada: mref DECLARA a subclasse (caso real do androidx)
    let call = |e: &mut Engine, r: rd_vm::heap::ObjRef, name: &str, sig: &str, args: Vec<Value>| {
        call_instance_intrinsic_mref(e, MY, MY, name, sig, r, &args)
            .expect("intrínseco")
            .unwrap_or(Value::Null)
    };
    assert_eq!(
        call(
            &mut e,
            a,
            "compareTo",
            "(Ljava/lang/Enum;)I",
            vec![Value::Obj(bb)]
        ),
        Value::Int(-1)
    );
    assert_eq!(
        call(
            &mut e,
            bb,
            "compareTo",
            "(Ljava/lang/Enum;)I",
            vec![Value::Obj(a)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut e,
            a,
            "compareTo",
            "(Ljava/lang/Enum;)I",
            vec![Value::Obj(a)]
        ),
        Value::Int(0)
    );
    let nm = call_instance_intrinsic_mref(&mut e, MY, MY, "name", "()Ljava/lang/String;", a, &[])
        .unwrap();
    match nm {
        Some(Value::Obj(r)) => assert_eq!(e.heap.as_str(r).unwrap(), "A"),
        other => panic!("name herdado: {other:?}"),
    }
    assert_eq!(call(&mut e, a, "ordinal", "()I", vec![]), Value::Int(0));
    let ts =
        call_instance_intrinsic_mref(&mut e, MY, MY, "toString", "()Ljava/lang/String;", bb, &[])
            .unwrap();
    match ts {
        Some(Value::Obj(r)) => assert_eq!(e.heap.as_str(r).unwrap(), "B"),
        other => panic!("toString herdado: {other:?}"),
    }

    // compareTo(null) → NPE materializado
    match call_instance_intrinsic_mref(
        &mut e,
        MY,
        MY,
        "compareTo",
        "(Ljava/lang/Enum;)I",
        a,
        &[Value::Null],
    ) {
        Err(rd_vm::err::VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/NullPointerException;")
        }
        other => panic!("esperava NPE, got {other:?}"),
    }

    // NÃO sequestra classes que não são enum
    let ne = e
        .heap
        .alloc_instance("LNotEnum;".to_string(), vec![])
        .unwrap();
    assert!(call_instance_intrinsic_mref(
        &mut e,
        "LNotEnum;",
        "LNotEnum;",
        "compareTo",
        "(Ljava/lang/Enum;)I",
        ne,
        &[Value::Obj(a)]
    )
    .expect("sem erro")
    .is_none());
}

/// M8 (compat real): views + iteradores host — 13º gap do launch do
/// org.primftpd (`LinkedHashMap.entrySet`). entrySet/keySet/values devolvem
/// SNAPSHOT (classe HashSet host, ordem de inserção); iterator() com
/// hasNext/next; Map.Entry host com getKey/getValue/setValue.
#[test]
fn collection_views_and_iterators() {
    use rd_vm::intrinsics::call_instance_intrinsic;
    use rd_vm::value::Value;
    const HM: &str = "Ljava/util/HashMap;";
    const AL: &str = "Ljava/util/ArrayList;";
    const SET: &str = "Ljava/util/HashSet;";
    const IT: &str = "Ljava/util/Iterator;";
    const EN: &str = "Ljava/util/Map$Entry;";

    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let k1 = e.heap.alloc_string("k1".into()).unwrap();
    let k2 = e.heap.alloc_string("k2".into()).unwrap();
    let v1 = e.heap.alloc_string("v1".into()).unwrap();
    let v2 = e.heap.alloc_string("v2".into()).unwrap();

    // mapa com 2 pares (ordem de inserção k1, k2)
    let m = e.heap.alloc_instance(HM.to_string(), vec![]).unwrap();
    call_instance_intrinsic(&mut e, HM, "<init>", "()V", m, &[]).unwrap();
    for (k, v) in [(k1, v1), (k2, v2)] {
        call_instance_intrinsic(
            &mut e,
            HM,
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            m,
            &[Value::Obj(k), Value::Obj(v)],
        )
        .unwrap();
    }

    // entrySet → view HashSet com size/contains do Set host
    let es = call_instance_intrinsic(&mut e, HM, "entrySet", "()Ljava/util/Set;", m, &[]).unwrap();
    let es = match es {
        Some(Value::Obj(r)) => r,
        other => panic!("entrySet: {other:?}"),
    };
    assert_eq!(e.heap.class_of(es).unwrap(), SET);
    assert_eq!(
        call_instance_intrinsic(&mut e, SET, "size", "()I", es, &[]).unwrap(),
        Some(Value::Int(2))
    );
    assert!(e.cp.is_subtype(SET, "Ljava/util/Set;"));
    assert!(e.cp.is_subtype(SET, "Ljava/util/AbstractSet;"));

    // for-each: iterator → hasNext/next → Map.Entry getKey/getValue
    let it = call_instance_intrinsic(&mut e, SET, "iterator", "()Ljava/util/Iterator;", es, &[])
        .unwrap();
    let it = match it {
        Some(Value::Obj(r)) => r,
        other => panic!("iterator: {other:?}"),
    };
    let mut seen = Vec::new();
    loop {
        let has = call_instance_intrinsic(&mut e, IT, "hasNext", "()Z", it, &[]).unwrap();
        if has != Some(Value::Int(1)) {
            break;
        }
        let n =
            call_instance_intrinsic(&mut e, IT, "next", "()Ljava/lang/Object;", it, &[]).unwrap();
        let entry = match n {
            Some(Value::Obj(r)) => r,
            other => panic!("next: {other:?}"),
        };
        assert_eq!(e.heap.class_of(entry).unwrap(), EN);
        let key = call_instance_intrinsic(&mut e, EN, "getKey", "()Ljava/lang/Object;", entry, &[])
            .unwrap();
        let val =
            call_instance_intrinsic(&mut e, EN, "getValue", "()Ljava/lang/Object;", entry, &[])
                .unwrap();
        let ks = match key {
            Some(Value::Obj(r)) => e.heap.as_str(r).unwrap().to_string(),
            other => panic!("getKey: {other:?}"),
        };
        let vs = match val {
            Some(Value::Obj(r)) => e.heap.as_str(r).unwrap().to_string(),
            other => panic!("getValue: {other:?}"),
        };
        seen.push((ks, vs));
    }
    assert_eq!(
        seen,
        vec![
            ("k1".to_string(), "v1".to_string()),
            ("k2".to_string(), "v2".to_string())
        ],
        "ordem de inserção"
    );
    // next esgotado → NoSuchElementException
    match call_instance_intrinsic(&mut e, IT, "next", "()Ljava/lang/Object;", it, &[]) {
        Err(rd_vm::err::VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/util/NoSuchElementException;")
        }
        other => panic!("esperava NSE, got {other:?}"),
    }
    // A failed next() does not invalidate the last successful element.
    // remove writes through to the map; then restore the fixture for views.
    assert_eq!(
        call_instance_intrinsic(&mut e, IT, "remove", "()V", it, &[]).unwrap(),
        Some(Value::Null)
    );
    assert_eq!(
        call_instance_intrinsic(&mut e, HM, "size", "()I", m, &[]).unwrap(),
        Some(Value::Int(1))
    );
    call_instance_intrinsic(
        &mut e,
        HM,
        "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        m,
        &[Value::Obj(k2), Value::Obj(v2)],
    )
    .unwrap();

    // keySet / values
    let ks = call_instance_intrinsic(&mut e, HM, "keySet", "()Ljava/util/Set;", m, &[]).unwrap();
    let ks = match ks {
        Some(Value::Obj(r)) => r,
        other => panic!("keySet: {other:?}"),
    };
    assert_eq!(
        call_instance_intrinsic(
            &mut e,
            SET,
            "contains",
            "(Ljava/lang/Object;)Z",
            ks,
            &[Value::Obj(k2)]
        )
        .unwrap(),
        Some(Value::Int(1))
    );
    let vs =
        call_instance_intrinsic(&mut e, HM, "values", "()Ljava/util/Collection;", m, &[]).unwrap();
    let vs = match vs {
        Some(Value::Obj(r)) => r,
        other => panic!("values: {other:?}"),
    };
    assert_eq!(
        call_instance_intrinsic(&mut e, SET, "size", "()I", vs, &[]).unwrap(),
        Some(Value::Int(2))
    );

    // setValue no entry devolve o anterior e atualiza o par do snapshot
    let it2 = call_instance_intrinsic(&mut e, SET, "iterator", "()Ljava/util/Iterator;", es, &[])
        .unwrap();
    let it2 = match it2 {
        Some(Value::Obj(r)) => r,
        other => panic!("it2: {other:?}"),
    };
    let first =
        call_instance_intrinsic(&mut e, IT, "next", "()Ljava/lang/Object;", it2, &[]).unwrap();
    let first = match first {
        Some(Value::Obj(r)) => r,
        other => panic!("first: {other:?}"),
    };
    let v9 = e.heap.alloc_string("v9".into()).unwrap();
    assert_eq!(
        call_instance_intrinsic(
            &mut e,
            EN,
            "setValue",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            first,
            &[Value::Obj(v9)]
        )
        .unwrap(),
        Some(Value::Obj(v1)),
        "setValue devolve o valor anterior"
    );
    assert_eq!(
        call_instance_intrinsic(&mut e, EN, "getValue", "()Ljava/lang/Object;", first, &[])
            .unwrap(),
        Some(Value::Obj(v9))
    );

    // List também itera
    let l = e.heap.alloc_instance(AL.to_string(), vec![]).unwrap();
    call_instance_intrinsic(&mut e, AL, "<init>", "()V", l, &[]).unwrap();
    call_instance_intrinsic(
        &mut e,
        AL,
        "add",
        "(Ljava/lang/Object;)Z",
        l,
        &[Value::Int(11)],
    )
    .unwrap();
    call_instance_intrinsic(
        &mut e,
        AL,
        "add",
        "(Ljava/lang/Object;)Z",
        l,
        &[Value::Int(22)],
    )
    .unwrap();
    let lit =
        call_instance_intrinsic(&mut e, AL, "iterator", "()Ljava/util/Iterator;", l, &[]).unwrap();
    let lit = match lit {
        Some(Value::Obj(r)) => r,
        other => panic!("list iterator: {other:?}"),
    };
    assert_eq!(
        call_instance_intrinsic(&mut e, IT, "hasNext", "()Z", lit, &[]).unwrap(),
        Some(Value::Int(1))
    );
    assert_eq!(
        call_instance_intrinsic(&mut e, IT, "next", "()Ljava/lang/Object;", lit, &[]).unwrap(),
        Some(Value::Int(11))
    );
    assert_eq!(
        call_instance_intrinsic(&mut e, IT, "next", "()Ljava/lang/Object;", lit, &[]).unwrap(),
        Some(Value::Int(22))
    );
    assert_eq!(
        call_instance_intrinsic(&mut e, IT, "hasNext", "()Z", lit, &[]).unwrap(),
        Some(Value::Int(0))
    );
}

/// M8 (compat real): Collections.synchronized* — 14º gap do launch do
/// org.primftpd (`synchronizedMap(Map)`). VM single-threaded ⇒ wrapper é
/// IDENTIDADE (devolve a própria coleção); null → NPE materializado.
#[test]
fn collections_synchronized_wrappers_are_identity() {
    use rd_vm::intrinsics::{call_instance_intrinsic, call_static_intrinsic};
    use rd_vm::value::Value;
    const COLL: &str = "Ljava/util/Collections;";
    const HM: &str = "Ljava/util/HashMap;";

    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let m = e.heap.alloc_instance(HM.to_string(), vec![]).unwrap();
    call_instance_intrinsic(&mut e, HM, "<init>", "()V", m, &[]).unwrap();

    // synchronizedMap devolve a MESMA referência (identidade)
    let w = call_static_intrinsic(
        &mut e,
        COLL,
        "synchronizedMap",
        "(Ljava/util/Map;)Ljava/util/Map;",
        &[Value::Obj(m)],
    )
    .unwrap();
    assert_eq!(w, Some(Value::Obj(m)), "wrapper = identidade");

    // e o mapa segue funcional pelos intrínsecos de Map
    let k = e.heap.alloc_string("k".into()).unwrap();
    let v = e.heap.alloc_string("v".into()).unwrap();
    call_instance_intrinsic(
        &mut e,
        HM,
        "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        m,
        &[Value::Obj(k), Value::Obj(v)],
    )
    .unwrap();
    assert_eq!(
        call_instance_intrinsic(
            &mut e,
            HM,
            "get",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            m,
            &[Value::Obj(k)]
        )
        .unwrap(),
        Some(Value::Obj(v))
    );

    // synchronizedList mesma política
    let l = e
        .heap
        .alloc_instance("Ljava/util/ArrayList;".to_string(), vec![])
        .unwrap();
    let wl = call_static_intrinsic(
        &mut e,
        COLL,
        "synchronizedList",
        "(Ljava/util/List;)Ljava/util/List;",
        &[Value::Obj(l)],
    )
    .unwrap();
    assert_eq!(wl, Some(Value::Obj(l)));

    // null → NPE
    match call_static_intrinsic(
        &mut e,
        COLL,
        "synchronizedMap",
        "(Ljava/util/Map;)Ljava/util/Map;",
        &[Value::Null],
    ) {
        Err(rd_vm::err::VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/NullPointerException;")
        }
        other => panic!("esperava NPE, got {other:?}"),
    }

    // fora da whitelist (ex.: sort) NÃO é capturado aqui — segue o fluxo
    // tipado existente (catch-all Ljava/ → NOT_IMPLEMENTED)
    assert!(call_static_intrinsic(
        &mut e,
        COLL,
        "sort",
        "(Ljava/util/List;)V",
        &[Value::Obj(l)]
    )
    .is_err());
}

/// M8 (compat real): ArrayDeque host — 15º gap do launch do org.primftpd.
/// Deque sobre o estado List (frente = índice 0): addFirst/addLast, push/pop,
/// peek/poll (null em vazio) vs get/remove (NoSuchElementException), rejeição
/// de null (como na JDK), removeFirst/LastOccurrence.
#[test]
fn array_deque_host_intrinsics() {
    use rd_vm::intrinsics::call_instance_intrinsic;
    use rd_vm::value::Value;
    const DQ: &str = "Ljava/util/ArrayDeque;";
    const IT: &str = "Ljava/util/Iterator;";

    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let d = e.heap.alloc_instance(DQ.to_string(), vec![]).unwrap();
    let call = |e: &mut Engine, name: &str, sig: &str, args: Vec<Value>| {
        call_instance_intrinsic(e, DQ, name, sig, d, &args)
    };

    call(&mut e, "<init>", "()V", vec![]).unwrap();
    assert_eq!(
        call(&mut e, "isEmpty", "()Z", vec![]).unwrap(),
        Some(Value::Int(1))
    );
    // vazio: peek/poll → null; get/remove → NSE
    assert_eq!(
        call(&mut e, "peekFirst", "()Ljava/lang/Object;", vec![]).unwrap(),
        Some(Value::Null)
    );
    assert_eq!(
        call(&mut e, "pollLast", "()Ljava/lang/Object;", vec![]).unwrap(),
        Some(Value::Null)
    );
    match call(&mut e, "removeFirst", "()Ljava/lang/Object;", vec![]) {
        Err(rd_vm::err::VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/util/NoSuchElementException;")
        }
        other => panic!("esperava NSE, got {other:?}"),
    }
    match call(&mut e, "getFirst", "()Ljava/lang/Object;", vec![]) {
        Err(rd_vm::err::VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/util/NoSuchElementException;")
        }
        other => panic!("esperava NSE, got {other:?}"),
    }

    // null rejeitado (JDK: ArrayDeque não aceita null)
    match call(
        &mut e,
        "addFirst",
        "(Ljava/lang/Object;)V",
        vec![Value::Null],
    ) {
        Err(rd_vm::err::VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/NullPointerException;")
        }
        other => panic!("esperava NPE, got {other:?}"),
    }

    // fila: addLast 1, addFirst 0, push -1 → [-1, 0, 1]
    call(
        &mut e,
        "addLast",
        "(Ljava/lang/Object;)V",
        vec![Value::Int(1)],
    )
    .unwrap();
    call(
        &mut e,
        "addFirst",
        "(Ljava/lang/Object;)V",
        vec![Value::Int(0)],
    )
    .unwrap();
    assert_eq!(
        call(
            &mut e,
            "push",
            "(Ljava/lang/Object;)V",
            vec![Value::Int(-1)]
        )
        .unwrap(),
        Some(Value::Null)
    );
    assert_eq!(
        call(&mut e, "size", "()I", vec![]).unwrap(),
        Some(Value::Int(3))
    );
    assert_eq!(
        call(&mut e, "getFirst", "()Ljava/lang/Object;", vec![]).unwrap(),
        Some(Value::Int(-1))
    );
    assert_eq!(
        call(&mut e, "getLast", "()Ljava/lang/Object;", vec![]).unwrap(),
        Some(Value::Int(1))
    );
    // pop = removeFirst
    assert_eq!(
        call(&mut e, "pop", "()Ljava/lang/Object;", vec![]).unwrap(),
        Some(Value::Int(-1))
    );
    // add(Object)Z = addLast → true
    assert_eq!(
        call(&mut e, "add", "(Ljava/lang/Object;)Z", vec![Value::Int(2)]).unwrap(),
        Some(Value::Int(1))
    );
    // [0, 1, 2]
    assert_eq!(
        call(
            &mut e,
            "offer",
            "(Ljava/lang/Object;)Z",
            vec![Value::Int(1)]
        )
        .unwrap(),
        Some(Value::Int(1))
    );
    // [0, 1, 2, 1]
    assert_eq!(
        call(
            &mut e,
            "removeLastOccurrence",
            "(Ljava/lang/Object;)Z",
            vec![Value::Int(1)]
        )
        .unwrap(),
        Some(Value::Int(1))
    );
    // [0, 1, 2]
    assert_eq!(
        call(
            &mut e,
            "removeFirstOccurrence",
            "(Ljava/lang/Object;)Z",
            vec![Value::Int(1)]
        )
        .unwrap(),
        Some(Value::Int(1))
    );
    // [0, 2]
    assert_eq!(
        call(
            &mut e,
            "contains",
            "(Ljava/lang/Object;)Z",
            vec![Value::Int(2)]
        )
        .unwrap(),
        Some(Value::Int(1))
    );
    assert_eq!(
        call(
            &mut e,
            "remove",
            "(Ljava/lang/Object;)Z",
            vec![Value::Int(0)]
        )
        .unwrap(),
        Some(Value::Int(1))
    );
    // [2]
    assert_eq!(
        call(&mut e, "removeLast", "()Ljava/lang/Object;", vec![]).unwrap(),
        Some(Value::Int(2))
    );
    assert_eq!(
        call(&mut e, "isEmpty", "()Z", vec![]).unwrap(),
        Some(Value::Int(1))
    );

    // iterator (frente → fundo) + clear
    call(
        &mut e,
        "addLast",
        "(Ljava/lang/Object;)V",
        vec![Value::Int(7)],
    )
    .unwrap();
    call(
        &mut e,
        "addLast",
        "(Ljava/lang/Object;)V",
        vec![Value::Int(8)],
    )
    .unwrap();
    let it = call(&mut e, "iterator", "()Ljava/util/Iterator;", vec![]).unwrap();
    let it = match it {
        Some(Value::Obj(r)) => r,
        other => panic!("iterator: {other:?}"),
    };
    assert_eq!(
        call_instance_intrinsic(&mut e, IT, "next", "()Ljava/lang/Object;", it, &[]).unwrap(),
        Some(Value::Int(7))
    );
    assert_eq!(
        call_instance_intrinsic(&mut e, IT, "next", "()Ljava/lang/Object;", it, &[]).unwrap(),
        Some(Value::Int(8))
    );
    call(&mut e, "clear", "()V", vec![]).unwrap();
    assert_eq!(
        call(&mut e, "size", "()I", vec![]).unwrap(),
        Some(Value::Int(0))
    );

    // subtipagem: ArrayDeque <: Deque/Queue/Collection; <: AbstractCollection
    assert!(e.cp.is_subtype(DQ, "Ljava/util/Deque;"));
    assert!(e.cp.is_subtype(DQ, "Ljava/util/Queue;"));
    assert!(e.cp.is_subtype(DQ, "Ljava/util/AbstractCollection;"));
    assert!(!e.cp.is_subtype(DQ, "Ljava/util/List;"));
}

/// M8 (compat real): Object.getClass + Class host — 16º gap do launch do
/// org.primftpd (`MainTabsActivity.getClass()`). getClass é final ⇒ captura
/// irrestrita; objeto Class canônico por descritor (identidade estável);
/// getName/getSimpleName/toString.
#[test]
fn get_class_and_class_host() {
    use rd_vm::intrinsics::{call_instance_intrinsic, call_instance_intrinsic_mref};
    use rd_vm::value::Value;
    const CLS: &str = "Ljava/lang/Class;";

    let mut b = DexBuilder::new();
    b.class("Lcom/exemplo/MinhaClasse;", "Ljava/lang/Object;");
    b.class("La/b/C$D;", "Ljava/lang/Object;");
    let mut e = engine_of(&b);
    let o = e
        .heap
        .alloc_instance("Lcom/exemplo/MinhaClasse;".to_string(), vec![])
        .unwrap();
    let nested = e
        .heap
        .alloc_instance("La/b/C$D;".to_string(), vec![])
        .unwrap();

    // getClass via mref de Object (caso real: método final herdado)
    let g = call_instance_intrinsic_mref(
        &mut e,
        "Lcom/exemplo/MinhaClasse;",
        "Ljava/lang/Object;",
        "getClass",
        "()Ljava/lang/Class;",
        o,
        &[],
    )
    .unwrap();
    let c = match g {
        Some(Value::Obj(r)) => r,
        other => panic!("getClass: {other:?}"),
    };
    assert_eq!(e.heap.class_of(c).unwrap(), CLS);

    // identidade estável: segunda chamada → mesma referência
    let g2 = call_instance_intrinsic(
        &mut e,
        "Lcom/exemplo/MinhaClasse;",
        "getClass",
        "()Ljava/lang/Class;",
        o,
        &[],
    )
    .unwrap();
    assert_eq!(g2, Some(Value::Obj(c)), "Class canônico: mesma ref");

    // getName / getSimpleName / toString
    let call_c = |e: &mut Engine, name: &str, r: rd_vm::heap::ObjRef| {
        call_instance_intrinsic(e, CLS, name, "()Ljava/lang/String;", r, &[]).unwrap()
    };
    match call_c(&mut e, "getName", c) {
        Some(Value::Obj(s)) => assert_eq!(e.heap.as_str(s).unwrap(), "com.exemplo.MinhaClasse"),
        other => panic!("getName: {other:?}"),
    }
    match call_c(&mut e, "getSimpleName", c) {
        Some(Value::Obj(s)) => assert_eq!(e.heap.as_str(s).unwrap(), "MinhaClasse"),
        other => panic!("getSimpleName: {other:?}"),
    }
    match call_c(&mut e, "toString", c) {
        Some(Value::Obj(s)) => {
            assert_eq!(e.heap.as_str(s).unwrap(), "class com.exemplo.MinhaClasse")
        }
        other => panic!("toString: {other:?}"),
    }

    // classe aninhada: getSimpleName descarta pacote E classe externa
    let gn = call_instance_intrinsic(
        &mut e,
        "La/b/C$D;",
        "getClass",
        "()Ljava/lang/Class;",
        nested,
        &[],
    )
    .unwrap();
    let cn = match gn {
        Some(Value::Obj(r)) => r,
        other => panic!("getClass nested: {other:?}"),
    };
    match call_c(&mut e, "getName", cn) {
        Some(Value::Obj(s)) => assert_eq!(e.heap.as_str(s).unwrap(), "a.b.C$D"),
        other => panic!("getName nested: {other:?}"),
    }
    match call_c(&mut e, "getSimpleName", cn) {
        Some(Value::Obj(s)) => assert_eq!(e.heap.as_str(s).unwrap(), "D"),
        other => panic!("getSimpleName nested: {other:?}"),
    }

    // Class <: Object
    assert!(e.cp.is_subtype(CLS, "Ljava/lang/Object;"));
}

/// M8 (compat real): ConcurrentHashMap host — 17º gap do launch do
/// org.primftpd. Single-threaded ⇒ mesmo estado/métodos do HashMap;
/// hierarquia JDK correta (NÃO estende HashMap; AbstractMap direto).
#[test]
fn concurrent_hash_map_host_intrinsics() {
    use rd_vm::intrinsics::call_instance_intrinsic;
    use rd_vm::value::Value;
    const CHM: &str = "Ljava/util/concurrent/ConcurrentHashMap;";

    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let m = e.heap.alloc_instance(CHM.to_string(), vec![]).unwrap();
    let call = |e: &mut Engine, name: &str, sig: &str, args: Vec<Value>| {
        call_instance_intrinsic(e, CHM, name, sig, m, &args).expect("intrínseco")
    };

    call(&mut e, "<init>", "()V", vec![]);
    let k = e.heap.alloc_string("k".into()).unwrap();
    let v = e.heap.alloc_string("v".into()).unwrap();
    assert_eq!(
        call(
            &mut e,
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            vec![Value::Obj(k), Value::Obj(v)]
        ),
        Some(Value::Null)
    );
    assert_eq!(call(&mut e, "size", "()I", vec![]), Some(Value::Int(1)));
    assert_eq!(
        call(
            &mut e,
            "get",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            vec![Value::Obj(k)]
        ),
        Some(Value::Obj(v))
    );
    // entrySet também funciona (views host)
    let es = call(&mut e, "entrySet", "()Ljava/util/Set;", vec![]);
    match es {
        Some(Value::Obj(r)) => {
            assert_eq!(
                call_instance_intrinsic(&mut e, "Ljava/util/HashSet;", "size", "()I", r, &[])
                    .unwrap(),
                Some(Value::Int(1))
            );
        }
        other => panic!("entrySet: {other:?}"),
    }
    // hierarquia JDK: CHM <: AbstractMap, NÃO <: HashMap; ifaces Map/ConcurrentMap
    assert!(e.cp.is_subtype(CHM, "Ljava/util/AbstractMap;"));
    assert!(!e.cp.is_subtype(CHM, "Ljava/util/HashMap;"));
    assert!(e.cp.is_subtype(CHM, "Ljava/util/concurrent/ConcurrentMap;"));
    assert!(e.cp.is_subtype(CHM, "Ljava/util/Map;"));
}

/// M8 (compat real): LinkedBlockingQueue host — 18º gap do launch do
/// org.primftpd. FIFO unbounded sobre List; bloqueantes fail-fast em VM
/// single-threaded (take/poll(J) com fila vazia → IllegalStateException;
/// element → NSE); null rejeitado.
#[test]
fn linked_blocking_queue_host_intrinsics() {
    use rd_vm::intrinsics::call_instance_intrinsic;
    use rd_vm::value::Value;
    const Q: &str = "Ljava/util/concurrent/LinkedBlockingQueue;";

    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let q = e.heap.alloc_instance(Q.to_string(), vec![]).unwrap();
    let call = |e: &mut Engine, name: &str, sig: &str, args: Vec<Value>| {
        call_instance_intrinsic(e, Q, name, sig, q, &args)
    };

    call(&mut e, "<init>", "()V", vec![]).unwrap();
    assert_eq!(
        call(&mut e, "isEmpty", "()Z", vec![]).unwrap(),
        Some(Value::Int(1))
    );
    assert_eq!(
        call(&mut e, "remainingCapacity", "()I", vec![]).unwrap(),
        Some(Value::Int(i32::MAX))
    );

    // vazia: poll()/peek → null; take/poll(J,TimeUnit) → ISE (fail-fast honesto)
    assert_eq!(
        call(&mut e, "poll", "()Ljava/lang/Object;", vec![]).unwrap(),
        Some(Value::Null)
    );
    assert_eq!(
        call(&mut e, "peek", "()Ljava/lang/Object;", vec![]).unwrap(),
        Some(Value::Null)
    );
    match call(&mut e, "take", "()Ljava/lang/Object;", vec![]) {
        Err(rd_vm::err::VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/IllegalStateException;")
        }
        other => panic!("esperava ISE, got {other:?}"),
    }
    match call(
        &mut e,
        "poll",
        "(JLjava/util/concurrent/TimeUnit;)Ljava/lang/Object;",
        vec![Value::Long(10), Value::Null],
    ) {
        Err(rd_vm::err::VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/IllegalStateException;")
        }
        other => panic!("esperava ISE, got {other:?}"),
    }
    match call(&mut e, "element", "()Ljava/lang/Object;", vec![]) {
        Err(rd_vm::err::VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/util/NoSuchElementException;")
        }
        other => panic!("esperava NSE, got {other:?}"),
    }

    // null rejeitado (JDK)
    match call(&mut e, "offer", "(Ljava/lang/Object;)Z", vec![Value::Null]) {
        Err(rd_vm::err::VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/NullPointerException;")
        }
        other => panic!("esperava NPE, got {other:?}"),
    }

    // FIFO: offer 1, put 2, add 3 → poll 1, take 2, peek 3
    assert_eq!(
        call(
            &mut e,
            "offer",
            "(Ljava/lang/Object;)Z",
            vec![Value::Int(1)]
        )
        .unwrap(),
        Some(Value::Int(1))
    );
    assert_eq!(
        call(&mut e, "put", "(Ljava/lang/Object;)V", vec![Value::Int(2)]).unwrap(),
        Some(Value::Null)
    );
    assert_eq!(
        call(&mut e, "add", "(Ljava/lang/Object;)Z", vec![Value::Int(3)]).unwrap(),
        Some(Value::Int(1))
    );
    assert_eq!(
        call(&mut e, "size", "()I", vec![]).unwrap(),
        Some(Value::Int(3))
    );
    assert_eq!(
        call(&mut e, "poll", "()Ljava/lang/Object;", vec![]).unwrap(),
        Some(Value::Int(1))
    );
    assert_eq!(
        call(&mut e, "take", "()Ljava/lang/Object;", vec![]).unwrap(),
        Some(Value::Int(2))
    );
    assert_eq!(
        call(&mut e, "peek", "()Ljava/lang/Object;", vec![]).unwrap(),
        Some(Value::Int(3))
    );
    assert_eq!(
        call(&mut e, "size", "()I", vec![]).unwrap(),
        Some(Value::Int(1))
    );
    assert_eq!(
        call(
            &mut e,
            "contains",
            "(Ljava/lang/Object;)Z",
            vec![Value::Int(3)]
        )
        .unwrap(),
        Some(Value::Int(1))
    );
    assert_eq!(
        call(
            &mut e,
            "remove",
            "(Ljava/lang/Object;)Z",
            vec![Value::Int(3)]
        )
        .unwrap(),
        Some(Value::Int(1))
    );
    assert_eq!(
        call(&mut e, "isEmpty", "()Z", vec![]).unwrap(),
        Some(Value::Int(1))
    );

    // subtipagem JDK: <: AbstractQueue <: AbstractCollection; BlockingQueue/Queue
    assert!(e.cp.is_subtype(Q, "Ljava/util/concurrent/BlockingQueue;"));
    assert!(e.cp.is_subtype(Q, "Ljava/util/Queue;"));
    assert!(e.cp.is_subtype(Q, "Ljava/util/AbstractQueue;"));
    assert!(!e.cp.is_subtype(Q, "Ljava/util/List;"));
}

/// M8 (compat real): ThreadLocal host — 19º gap do launch do org.primftpd.
/// Single-threaded ⇒ um valor por instância: get chama initialValue() (DEX)
/// na primeira leitura (como na JDK); set/remove; override DEX vence.
#[test]
fn thread_local_host_intrinsics() {
    use rd_vm::intrinsics::call_instance_intrinsic_mref;
    use rd_vm::value::Value;
    const TL: &str = "Ljava/lang/ThreadLocal;";

    // LMyTL extends ThreadLocal com initialValue() { return "inicial" }
    let mut b = DexBuilder::new();
    let my = b.class("LMyTL;", TL);
    let s_ini = b.intern("inicial") as u16;
    b.direct(
        my,
        "initialValue",
        "Ljava/lang/Object;",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 0, {
            let mut u = op21c(0x1A, 0, s_ini); // const-string v0, "inicial"
            u.extend(op11x(0x11, 0)); // return v0
            u
        })),
    );
    let mut e = engine_of(&b);

    // ThreadLocal simples: get sem set → null (initialValue default)
    let plain = e.heap.alloc_instance(TL.to_string(), vec![]).unwrap();
    let call = |e: &mut Engine,
                cls: &str,
                r: rd_vm::heap::ObjRef,
                name: &str,
                sig: &str,
                args: Vec<Value>| {
        call_instance_intrinsic_mref(e, cls, cls, name, sig, r, &args).expect("intrínseco")
    };
    call(&mut e, TL, plain, "<init>", "()V", vec![]);
    assert_eq!(
        call(&mut e, TL, plain, "get", "()Ljava/lang/Object;", vec![]),
        Some(Value::Null)
    );
    // set → get devolve o valor; remove → volta ao initialValue (null)
    let v = e.heap.alloc_string("x".into()).unwrap();
    call(
        &mut e,
        TL,
        plain,
        "set",
        "(Ljava/lang/Object;)V",
        vec![Value::Obj(v)],
    );
    assert_eq!(
        call(&mut e, TL, plain, "get", "()Ljava/lang/Object;", vec![]),
        Some(Value::Obj(v))
    );
    call(&mut e, TL, plain, "remove", "()V", vec![]);
    assert_eq!(
        call(&mut e, TL, plain, "get", "()Ljava/lang/Object;", vec![]),
        Some(Value::Null)
    );

    // subclasse: primeira leitura chama initialValue() do DEX
    let mine = e.heap.alloc_instance("LMyTL;".to_string(), vec![]).unwrap();
    // super() via mref ThreadLocal (ctor da subclasse segue DEX; aqui o init host)
    call_instance_intrinsic_mref(&mut e, "LMyTL;", TL, "<init>", "()V", mine, &[])
        .expect("super init");
    let g = call(
        &mut e,
        "LMyTL;",
        mine,
        "get",
        "()Ljava/lang/Object;",
        vec![],
    );
    match g {
        Some(Value::Obj(r)) => assert_eq!(e.heap.as_str(r).unwrap(), "inicial"),
        other => panic!("get com initialValue DEX: {other:?}"),
    }
    // segunda leitura usa o valor memoizado (set sobrescreve)
    let v2 = e.heap.alloc_string("y".into()).unwrap();
    call(
        &mut e,
        "LMyTL;",
        mine,
        "set",
        "(Ljava/lang/Object;)V",
        vec![Value::Obj(v2)],
    );
    assert_eq!(
        call(
            &mut e,
            "LMyTL;",
            mine,
            "get",
            "()Ljava/lang/Object;",
            vec![]
        ),
        Some(Value::Obj(v2))
    );
    // ThreadLocal <: Object
    assert!(e.cp.is_subtype(TL, "Ljava/lang/Object;"));

    // InheritableThreadLocal (subclasse direta): mesmo host, mesma política
    const ITL: &str = "Ljava/lang/InheritableThreadLocal;";
    let inh = e.heap.alloc_instance(ITL.to_string(), vec![]).unwrap();
    call_instance_intrinsic_mref(&mut e, ITL, ITL, "<init>", "()V", inh, &[])
        .expect("ITL init")
        .expect("capturado");
    let v3 = e.heap.alloc_string("z".into()).unwrap();
    call_instance_intrinsic_mref(
        &mut e,
        ITL,
        ITL,
        "set",
        "(Ljava/lang/Object;)V",
        inh,
        &[Value::Obj(v3)],
    )
    .expect("ITL set");
    assert_eq!(
        call_instance_intrinsic_mref(&mut e, ITL, ITL, "get", "()Ljava/lang/Object;", inh, &[])
            .expect("ITL get"),
        Some(Value::Obj(v3))
    );
    assert!(e.cp.is_subtype(ITL, TL));
}

/// M8 (compat real): System.getProperty host — 21º gap do launch do
/// org.primftpd. Tabela da plataforma coerente com a VM; desconhecida → null;
/// variante (key, def) devolve def; key null → NPE.
#[test]
fn system_get_property_host() {
    use rd_vm::intrinsics::call_static_intrinsic;
    use rd_vm::value::Value;
    const SYS: &str = "Ljava/lang/System;";

    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let k_os = Value::Obj(e.heap.alloc_string("os.name".into()).unwrap());
    let k_sep = Value::Obj(e.heap.alloc_string("line.separator".into()).unwrap());
    let k_none = Value::Obj(
        e.heap
            .alloc_string("propriedade.inexistente".into())
            .unwrap(),
    );
    let k_def = Value::Obj(e.heap.alloc_string("fallback".into()).unwrap());

    // conhecidas
    let os = call_static_intrinsic(
        &mut e,
        SYS,
        "getProperty",
        "(Ljava/lang/String;)Ljava/lang/String;",
        &[k_os],
    )
    .unwrap();
    match os {
        Some(Value::Obj(r)) => assert_eq!(e.heap.as_str(r).unwrap(), "Linux"),
        other => panic!("os.name: {other:?}"),
    }
    let sep = call_static_intrinsic(
        &mut e,
        SYS,
        "getProperty",
        "(Ljava/lang/String;)Ljava/lang/String;",
        &[k_sep],
    )
    .unwrap();
    match sep {
        Some(Value::Obj(r)) => assert_eq!(e.heap.as_str(r).unwrap(), "\n", "LF real"),
        other => panic!("line.separator: {other:?}"),
    }

    // desconhecida → null (semântica JDK)
    assert_eq!(
        call_static_intrinsic(
            &mut e,
            SYS,
            "getProperty",
            "(Ljava/lang/String;)Ljava/lang/String;",
            std::slice::from_ref(&k_none)
        )
        .unwrap(),
        Some(Value::Null)
    );

    // (key, def): desconhecida devolve def
    assert_eq!(
        call_static_intrinsic(
            &mut e,
            SYS,
            "getProperty",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[k_none, k_def.clone()]
        )
        .unwrap(),
        Some(k_def)
    );

    // key null → NPE materializado
    match call_static_intrinsic(
        &mut e,
        SYS,
        "getProperty",
        "(Ljava/lang/String;)Ljava/lang/String;",
        &[Value::Null],
    ) {
        Err(rd_vm::err::VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/NullPointerException;")
        }
        other => panic!("esperava NPE, got {other:?}"),
    }
}

/// M8 (compat real): Class.getClassLoader — 22º gap do launch do org.primftpd.
/// ClassLoader canônico por VM (single-classloader; identidade estável).
#[test]
fn class_get_class_loader_host() {
    use rd_vm::intrinsics::call_instance_intrinsic;
    use rd_vm::value::Value;
    const CLS: &str = "Ljava/lang/Class;";
    const LOADER: &str = "Ljava/lang/ClassLoader;";

    let mut b = DexBuilder::new();
    b.class("Lcom/exemplo/A;", "Ljava/lang/Object;");
    b.class("Lcom/exemplo/B;", "Ljava/lang/Object;");
    let mut e = engine_of(&b);
    let a = e
        .heap
        .alloc_instance("Lcom/exemplo/A;".to_string(), vec![])
        .unwrap();
    let bb = e
        .heap
        .alloc_instance("Lcom/exemplo/B;".to_string(), vec![])
        .unwrap();

    let ca = match call_instance_intrinsic(
        &mut e,
        "Lcom/exemplo/A;",
        "getClass",
        "()Ljava/lang/Class;",
        a,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("getClass A: {other:?}"),
    };
    let cb = match call_instance_intrinsic(
        &mut e,
        "Lcom/exemplo/B;",
        "getClass",
        "()Ljava/lang/Class;",
        bb,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("getClass B: {other:?}"),
    };

    let la = call_instance_intrinsic(
        &mut e,
        CLS,
        "getClassLoader",
        "()Ljava/lang/ClassLoader;",
        ca,
        &[],
    )
    .unwrap();
    let lb = call_instance_intrinsic(
        &mut e,
        CLS,
        "getClassLoader",
        "()Ljava/lang/ClassLoader;",
        cb,
        &[],
    )
    .unwrap();
    assert_eq!(
        la, lb,
        "ClassLoader canônico: mesmo objeto para toda classe"
    );
    let l = match la {
        Some(Value::Obj(r)) => r,
        other => panic!("loader: {other:?}"),
    };
    assert_eq!(e.heap.class_of(l).unwrap(), LOADER);
    assert!(e.cp.is_subtype(LOADER, "Ljava/lang/Object;"));

    // chamada repetida → mesma referência (identidade estável)
    let la2 = call_instance_intrinsic(
        &mut e,
        CLS,
        "getClassLoader",
        "()Ljava/lang/ClassLoader;",
        ca,
        &[],
    )
    .unwrap();
    assert_eq!(la2, Some(Value::Obj(l)));
}

/// M8 (compat real): System.getSecurityManager — 23º gap do launch do
/// org.primftpd. Android não tem SecurityManager (deprecated na JDK) ⇒ null.
#[test]
fn system_get_security_manager_is_null() {
    use rd_vm::intrinsics::call_static_intrinsic;
    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    assert_eq!(
        call_static_intrinsic(
            &mut e,
            "Ljava/lang/System;",
            "getSecurityManager",
            "()Ljava/lang/SecurityManager;",
            &[]
        )
        .unwrap(),
        Some(rd_vm::value::Value::Null)
    );
}

/// M8 (compat real): ServiceLoader.load era o 24º gap do launch de
/// org.primftpd. Sem leitura de META-INF/services, o host devolve um loader
/// vazio: a iteração termina e `next` ainda respeita NoSuchElementException.
#[test]
fn service_loader_host_is_empty_and_iterable() {
    use rd_vm::intrinsics::{call_instance_intrinsic, call_static_intrinsic};
    use rd_vm::value::Value;
    const SL: &str = "Ljava/util/ServiceLoader;";
    const IT: &str = "Ljava/util/Iterator;";

    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let marker = e
        .heap
        .alloc_instance("Lcom/example/Service;".to_string(), vec![])
        .unwrap();
    let service_class = match call_instance_intrinsic(
        &mut e,
        "Lcom/example/Service;",
        "getClass",
        "()Ljava/lang/Class;",
        marker,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("service Class: {other:?}"),
    };

    // As duas sobrecargas relevantes retornam loaders vazios; loader null é
    // permitido pela API (bootstrap), mas service null continua sendo NPE.
    let one_arg = call_static_intrinsic(
        &mut e,
        SL,
        "load",
        "(Ljava/lang/Class;)Ljava/util/ServiceLoader;",
        &[Value::Obj(service_class)],
    )
    .unwrap();
    let two_args = call_static_intrinsic(
        &mut e,
        SL,
        "load",
        "(Ljava/lang/Class;Ljava/lang/ClassLoader;)Ljava/util/ServiceLoader;",
        &[Value::Obj(service_class), Value::Null],
    )
    .unwrap();
    let loader = match one_arg {
        Some(Value::Obj(r)) => r,
        other => panic!("ServiceLoader.load: {other:?}"),
    };
    assert!(matches!(two_args, Some(Value::Obj(_))));
    assert_eq!(e.heap.class_of(loader).unwrap(), SL);

    let it = match call_instance_intrinsic(
        &mut e,
        SL,
        "iterator",
        "()Ljava/util/Iterator;",
        loader,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("ServiceLoader.iterator: {other:?}"),
    };
    assert_eq!(e.heap.class_of(it).unwrap(), IT);
    assert_eq!(
        call_instance_intrinsic(&mut e, IT, "hasNext", "()Z", it, &[]).unwrap(),
        Some(Value::Int(0))
    );
    match call_instance_intrinsic(&mut e, IT, "next", "()Ljava/lang/Object;", it, &[]) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Ljava/util/NoSuchElementException;"),
        other => panic!("esperava NoSuchElementException, got {other:?}"),
    }
    match call_static_intrinsic(
        &mut e,
        SL,
        "load",
        "(Ljava/lang/Class;)Ljava/util/ServiceLoader;",
        &[Value::Null],
    ) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Ljava/lang/NullPointerException;"),
        other => panic!("esperava NPE, got {other:?}"),
    }
    assert!(e.cp.is_subtype(SL, "Ljava/lang/Iterable;"));
    assert!(e.cp.is_subtype(SL, "Ljava/lang/Object;"));
}

#[test]
fn class_loader_get_resources_without_attached_apk_returns_empty_enumeration() {
    use rd_vm::intrinsics::call_instance_intrinsic;
    use rd_vm::value::Value;
    const CLS: &str = "Ljava/lang/Class;";
    const LOADER: &str = "Ljava/lang/ClassLoader;";
    const EN: &str = "Ljava/util/Enumeration;";

    let mut b = DexBuilder::new();
    b.class("Lcom/exemplo/Res;", "Ljava/lang/Object;");
    let mut e = engine_of(&b);
    let marker = e
        .heap
        .alloc_instance("Lcom/exemplo/Res;".to_string(), vec![])
        .unwrap();

    // ClassLoader canônico via Class.getClassLoader (identidade estável por VM).
    let class = match call_instance_intrinsic(
        &mut e,
        "Lcom/exemplo/Res;",
        "getClass",
        "()Ljava/lang/Class;",
        marker,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("getClass: {other:?}"),
    };
    let loader = match call_instance_intrinsic(
        &mut e,
        CLS,
        "getClassLoader",
        "()Ljava/lang/ClassLoader;",
        class,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("getClassLoader: {other:?}"),
    };
    assert_eq!(e.heap.class_of(loader).unwrap(), LOADER);

    // Sem APK anexado, getResources(nome) devolve Enumeration vazia, nunca
    // null. Cada chamada devolve um cursor novo e independente.
    let name = Value::Obj(
        e.heap
            .alloc_string("META-INF/services/com.exemplo.Res".into())
            .unwrap(),
    );
    let en = match call_instance_intrinsic(
        &mut e,
        LOADER,
        "getResources",
        "(Ljava/lang/String;)Ljava/util/Enumeration;",
        loader,
        &[name],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("getResources: {other:?}"),
    };
    assert_eq!(e.heap.class_of(en).unwrap(), EN);
    assert_eq!(
        call_instance_intrinsic(&mut e, EN, "hasMoreElements", "()Z", en, &[]).unwrap(),
        Some(Value::Int(0))
    );
    match call_instance_intrinsic(&mut e, EN, "nextElement", "()Ljava/lang/Object;", en, &[]) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Ljava/util/NoSuchElementException;"),
        other => panic!("esperava NoSuchElementException, got {other:?}"),
    }
    let name2 = Value::Obj(e.heap.alloc_string("res/outro".into()).unwrap());
    let en2 = match call_instance_intrinsic(
        &mut e,
        LOADER,
        "getResources",
        "(Ljava/lang/String;)Ljava/util/Enumeration;",
        loader,
        &[name2],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("getResources 2: {other:?}"),
    };
    assert_ne!(en, en2);

    // name null → NPE (Objects.requireNonNull na JDK 9+).
    match call_instance_intrinsic(
        &mut e,
        LOADER,
        "getResources",
        "(Ljava/lang/String;)Ljava/util/Enumeration;",
        loader,
        &[Value::Null],
    ) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Ljava/lang/NullPointerException;"),
        other => panic!("esperava NPE, got {other:?}"),
    }
}

#[test]
fn class_loader_reads_resource_entries_from_attached_apk() {
    use rd_vm::intrinsics::call_instance_intrinsic;

    const CLASS: &str = "Ljava/lang/Class;";
    const CLASS_LOADER: &str = "Ljava/lang/ClassLoader;";
    const URL: &str = "Ljava/net/URL;";
    const ENUMERATION: &str = "Ljava/util/Enumeration;";
    const RESOURCE: &str = "assets/logback.xml";

    let apk_bytes = common::apkfix::build_apk(
        "com.test.resources",
        "LMain;",
        common::apkfix::ArscFix::new().build(),
        vec![
            (RESOURCE, b"<configuration/>".to_vec()),
            (
                "META-INF/services/demo.Service",
                b"demo.Provider\n".to_vec(),
            ),
        ],
    );
    let apk = rd_apk::Apk::from_bytes(apk_bytes).expect("APK de recursos parseia");

    // O método DEX segue o caminho real invoke-virtual: ClassLoader.getResource
    // → URL.openConnection → URLConnection.getInputStream → InputStream.read.
    let mut b = DexBuilder::new();
    b.class("LMarker;", "Ljava/lang/Object;");
    let resource_name = b.intern(RESOURCE) as u16;
    let p_get_resource = b.proto_idx(URL, vec!["Ljava/lang/String;".into()]);
    let p_open_connection = b.proto_idx("Ljava/net/URLConnection;", vec![]);
    let p_set_use_caches = b.proto_idx("V", vec!["Z".into()]);
    let p_get_input_stream = b.proto_idx("Ljava/io/InputStream;", vec![]);
    let p_read = b.proto_idx("I", vec![]);
    let get_resource = b.method_idx(CLASS_LOADER, p_get_resource, "getResource");
    let open_connection = b.method_idx(URL, p_open_connection, "openConnection");
    let set_use_caches = b.method_idx("Ljava/net/URLConnection;", p_set_use_caches, "setUseCaches");
    let get_input_stream = b.method_idx(
        "Ljava/net/URLConnection;",
        p_get_input_stream,
        "getInputStream",
    );
    let read = b.method_idx("Ljava/io/InputStream;", p_read, "read");
    let caller = b.class("LCaso;", "Ljava/lang/Object;");
    b.direct(
        caller,
        "readFirst",
        "I",
        vec![CLASS_LOADER],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(5, 1, 2, {
            let mut u = op21c(0x1A, 0, resource_name); // const-string v0, asset path
            u.extend(op35c(0x6E, 2, get_resource, [4, 0, 0, 0, 0])); // loader.getResource
            u.extend(op11x(0x0C, 1)); // move-result-object URL v1
            u.extend(op35c(0x6E, 1, open_connection, [1, 0, 0, 0, 0]));
            u.extend(op11x(0x0C, 1)); // move-result-object URLConnection v1
            u.extend(op11n(0x12, 0, 0)); // setUseCaches(false)
            u.extend(op35c(0x6E, 2, set_use_caches, [1, 0, 0, 0, 0]));
            u.extend(op35c(0x6E, 1, get_input_stream, [1, 0, 0, 0, 0]));
            u.extend(op11x(0x0C, 2)); // move-result-object InputStream v2
            u.extend(op35c(0x6E, 1, read, [2, 0, 0, 0, 0]));
            u.extend(op11x(0x0A, 0)); // move-result v0
            u.extend(op11x(0x0F, 0)); // return first byte
            u
        })),
    );

    let dex = rd_dex::Dex::parse(b.finish()).expect("DEX de ClassLoader parseia");
    let mut vm = Engine::new(vec![dex], VmConfig::default());
    vm.set_resources(apk);
    let marker = vm
        .heap
        .alloc_instance("LMarker;".to_string(), Vec::new())
        .unwrap();
    let class = match call_instance_intrinsic(
        &mut vm,
        "LMarker;",
        "getClass",
        "()Ljava/lang/Class;",
        marker,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("getClass: {other:?}"),
    };
    let loader = match call_instance_intrinsic(
        &mut vm,
        CLASS,
        "getClassLoader",
        "()Ljava/lang/ClassLoader;",
        class,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("getClassLoader: {other:?}"),
    };

    assert_eq!(
        invoke(
            &mut vm,
            "readFirst",
            "(Ljava/lang/ClassLoader;)I",
            &[Value::Obj(loader)],
        )
        .unwrap()
        .as_int()
        .unwrap(),
        b'<' as i32
    );

    let resource = Value::Obj(vm.heap.alloc_string(RESOURCE.into()).unwrap());
    let url = match call_instance_intrinsic(
        &mut vm,
        CLASS_LOADER,
        "getResource",
        "(Ljava/lang/String;)Ljava/net/URL;",
        loader,
        std::slice::from_ref(&resource),
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("getResource: {other:?}"),
    };
    assert_eq!(vm.heap.class_of(url).unwrap(), URL);
    let url_string = match call_instance_intrinsic(
        &mut vm,
        URL,
        "toExternalForm",
        "()Ljava/lang/String;",
        url,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("URL.toExternalForm: {other:?}"),
    };
    assert_eq!(
        vm.heap.as_str(url_string).unwrap(),
        "apk:/assets/logback.xml"
    );

    let enumeration = match call_instance_intrinsic(
        &mut vm,
        CLASS_LOADER,
        "getResources",
        "(Ljava/lang/String;)Ljava/util/Enumeration;",
        loader,
        &[resource],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("getResources: {other:?}"),
    };
    assert_eq!(vm.heap.class_of(enumeration).unwrap(), ENUMERATION);
    assert_eq!(
        call_instance_intrinsic(
            &mut vm,
            ENUMERATION,
            "hasMoreElements",
            "()Z",
            enumeration,
            &[],
        )
        .unwrap(),
        Some(Value::Int(1))
    );
    let enumerated_url = match call_instance_intrinsic(
        &mut vm,
        ENUMERATION,
        "nextElement",
        "()Ljava/lang/Object;",
        enumeration,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("Enumeration.nextElement: {other:?}"),
    };
    assert_eq!(vm.heap.class_of(enumerated_url).unwrap(), URL);
    assert_eq!(
        call_instance_intrinsic(
            &mut vm,
            ENUMERATION,
            "hasMoreElements",
            "()Z",
            enumeration,
            &[],
        )
        .unwrap(),
        Some(Value::Int(0))
    );

    let stream_name = Value::Obj(vm.heap.alloc_string(RESOURCE.into()).unwrap());
    let resource_stream = match call_instance_intrinsic(
        &mut vm,
        CLASS_LOADER,
        "getResourceAsStream",
        "(Ljava/lang/String;)Ljava/io/InputStream;",
        loader,
        &[stream_name],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("getResourceAsStream: {other:?}"),
    };
    let url_stream = match call_instance_intrinsic(
        &mut vm,
        URL,
        "openStream",
        "()Ljava/io/InputStream;",
        url,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("URL.openStream: {other:?}"),
    };
    for (method, stream) in [
        ("getResourceAsStream", resource_stream),
        ("URL.openStream", url_stream),
    ] {
        assert_eq!(
            call_instance_intrinsic(
                &mut vm,
                "Ljava/io/ByteArrayInputStream;",
                "read",
                "()I",
                stream,
                &[],
            )
            .unwrap(),
            Some(Value::Int(b'<' as i32)),
            "{method} returns APK bytes"
        );
    }

    let missing_name = Value::Obj(vm.heap.alloc_string("assets/missing".into()).unwrap());
    assert_eq!(
        call_instance_intrinsic(
            &mut vm,
            CLASS_LOADER,
            "getResource",
            "(Ljava/lang/String;)Ljava/net/URL;",
            loader,
            &[missing_name],
        )
        .unwrap(),
        Some(Value::Null)
    );
}

#[test]
fn android_environment_exposes_deterministic_virtual_external_storage() {
    use rd_vm::framework::call_host_static;
    use rd_vm::intrinsics::call_instance_intrinsic;

    let mut vm = Engine::new(vec![], VmConfig::default());
    let state = call_host_static(
        &mut vm,
        "Landroid/os/Environment;",
        "getExternalStorageState",
        "()Ljava/lang/String;",
        &[],
    )
    .unwrap()
    .unwrap();
    let Value::Obj(state) = state else {
        panic!("Environment.getExternalStorageState did not return a String");
    };
    assert_eq!(vm.heap.as_str(state).unwrap(), "mounted");

    let storage = call_host_static(
        &mut vm,
        "Landroid/os/Environment;",
        "getExternalStorageDirectory",
        "()Ljava/io/File;",
        &[],
    )
    .unwrap()
    .unwrap();
    let Value::Obj(storage) = storage else {
        panic!("Environment.getExternalStorageDirectory did not return a File");
    };
    let path = call_instance_intrinsic(
        &mut vm,
        "Ljava/io/File;",
        "getAbsolutePath",
        "()Ljava/lang/String;",
        storage,
        &[],
    )
    .unwrap()
    .unwrap();
    let Value::Obj(path) = path else {
        panic!("File.getAbsolutePath did not return a String");
    };
    assert_eq!(vm.heap.as_str(path).unwrap(), "/storage/emulated/0");
    assert_eq!(
        call_instance_intrinsic(&mut vm, "Ljava/io/File;", "exists", "()Z", storage, &[]).unwrap(),
        Some(Value::Int(1))
    );
    assert_eq!(
        call_host_static(
            &mut vm,
            "Landroid/os/Environment;",
            "isExternalStorageManager",
            "()Z",
            &[],
        )
        .unwrap(),
        Some(Value::Int(0)),
        "the default virtual device does not grant all-files access"
    );
}

#[test]
fn vector_and_stack_host_intrinsics_cover_legacy_collection_apis() {
    use rd_vm::intrinsics::call_instance_intrinsic;

    const VECTOR: &str = "Ljava/util/Vector;";
    const STACK: &str = "Ljava/util/Stack;";
    const ENUMERATION: &str = "Ljava/util/Enumeration;";

    let mut b = DexBuilder::new();
    b.class("LMarker;", "Ljava/lang/Object;");
    let mut vm = engine_of(&b);

    let vector = vm
        .heap
        .alloc_instance(VECTOR.to_string(), Vec::new())
        .unwrap();
    call_instance_intrinsic(&mut vm, VECTOR, "<init>", "(I)V", vector, &[Value::Int(0)]).unwrap();
    let first = Value::Obj(vm.heap.alloc_string("first".into()).unwrap());
    call_instance_intrinsic(
        &mut vm,
        VECTOR,
        "addElement",
        "(Ljava/lang/Object;)V",
        vector,
        std::slice::from_ref(&first),
    )
    .unwrap();
    assert_eq!(
        call_instance_intrinsic(
            &mut vm,
            VECTOR,
            "elementAt",
            "(I)Ljava/lang/Object;",
            vector,
            &[Value::Int(0)],
        )
        .unwrap(),
        Some(first.clone())
    );
    let enumeration = match call_instance_intrinsic(
        &mut vm,
        VECTOR,
        "elements",
        "()Ljava/util/Enumeration;",
        vector,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("Vector.elements: {other:?}"),
    };
    assert_eq!(
        call_instance_intrinsic(
            &mut vm,
            ENUMERATION,
            "nextElement",
            "()Ljava/lang/Object;",
            enumeration,
            &[],
        )
        .unwrap(),
        Some(first.clone())
    );

    let stack = vm
        .heap
        .alloc_instance(STACK.to_string(), Vec::new())
        .unwrap();
    call_instance_intrinsic(&mut vm, STACK, "<init>", "()V", stack, &[]).unwrap();
    let second = Value::Obj(vm.heap.alloc_string("second".into()).unwrap());
    assert_eq!(
        call_instance_intrinsic(
            &mut vm,
            STACK,
            "push",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            stack,
            std::slice::from_ref(&first),
        )
        .unwrap(),
        Some(first.clone())
    );
    call_instance_intrinsic(
        &mut vm,
        STACK,
        "push",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        stack,
        std::slice::from_ref(&second),
    )
    .unwrap();
    assert_eq!(
        call_instance_intrinsic(
            &mut vm,
            STACK,
            "search",
            "(Ljava/lang/Object;)I",
            stack,
            std::slice::from_ref(&first)
        )
        .unwrap(),
        Some(Value::Int(2))
    );
    assert_eq!(
        call_instance_intrinsic(&mut vm, STACK, "pop", "()Ljava/lang/Object;", stack, &[]).unwrap(),
        Some(second)
    );
    assert_eq!(
        call_instance_intrinsic(&mut vm, STACK, "pop", "()Ljava/lang/Object;", stack, &[]).unwrap(),
        Some(first)
    );
    assert_eq!(
        call_instance_intrinsic(&mut vm, STACK, "empty", "()Z", stack, &[]).unwrap(),
        Some(Value::Int(1))
    );
    match call_instance_intrinsic(&mut vm, STACK, "pop", "()Ljava/lang/Object;", stack, &[]) {
        Err(VmExit::Exception(throwable)) => {
            assert_eq!(throwable.class, "Ljava/util/EmptyStackException;");
        }
        other => panic!("empty Stack.pop should throw, got {other:?}"),
    }
}

#[test]
fn sax_input_source_retains_input_stream_and_system_id() {
    use rd_vm::intrinsics::{call_instance_intrinsic, call_instance_intrinsic_mref};

    const INPUT_SOURCE: &str = "Lorg/xml/sax/InputSource;";
    const INPUT_STREAM: &str = "Ljava/io/InputStream;";
    const DEFAULT_HANDLER: &str = "Lorg/xml/sax/helpers/DefaultHandler;";

    let mut b = DexBuilder::new();
    b.class("LMarker;", "Ljava/lang/Object;");
    b.class("LTestHandler;", DEFAULT_HANDLER);
    let mut vm = engine_of(&b);
    let source = vm
        .heap
        .alloc_instance(INPUT_SOURCE.to_string(), Vec::new())
        .unwrap();
    let stream = vm
        .heap
        .alloc_instance(INPUT_STREAM.to_string(), Vec::new())
        .unwrap();
    assert_eq!(
        call_instance_intrinsic(
            &mut vm,
            INPUT_SOURCE,
            "<init>",
            "(Ljava/io/InputStream;)V",
            source,
            &[Value::Obj(stream)],
        )
        .unwrap(),
        Some(Value::Null)
    );
    assert_eq!(
        call_instance_intrinsic(
            &mut vm,
            INPUT_SOURCE,
            "getByteStream",
            "()Ljava/io/InputStream;",
            source,
            &[],
        )
        .unwrap(),
        Some(Value::Obj(stream))
    );

    let system_id = Value::Obj(
        vm.heap
            .alloc_string("apk:/assets/logback.xml".into())
            .unwrap(),
    );
    call_instance_intrinsic(
        &mut vm,
        INPUT_SOURCE,
        "setSystemId",
        "(Ljava/lang/String;)V",
        source,
        &[system_id],
    )
    .unwrap();
    let returned_system_id = match call_instance_intrinsic(
        &mut vm,
        INPUT_SOURCE,
        "getSystemId",
        "()Ljava/lang/String;",
        source,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("InputSource.getSystemId: {other:?}"),
    };
    assert_eq!(
        vm.heap.as_str(returned_system_id).unwrap(),
        "apk:/assets/logback.xml"
    );

    let handler = vm
        .heap
        .alloc_instance("LTestHandler;".to_string(), Vec::new())
        .unwrap();
    assert_eq!(
        call_instance_intrinsic_mref(
            &mut vm,
            "LTestHandler;",
            DEFAULT_HANDLER,
            "<init>",
            "()V",
            handler,
            &[],
        )
        .unwrap(),
        Some(Value::Null)
    );
}

#[test]
fn array_list_collection_ctor_snapshots_source() {
    use rd_vm::intrinsics::call_instance_intrinsic;
    use rd_vm::value::Value;
    const AL: &str = "Ljava/util/ArrayList;";
    const LHS: &str = "Ljava/util/LinkedHashSet;";

    let mut b = DexBuilder::new();
    b.class("Lcom/exemplo/Col;", "Ljava/lang/Object;");
    let mut e = engine_of(&b);

    // Fonte 1: LinkedHashSet host — a cópia segue a ordem de inserção da fonte.
    let set = e.heap.alloc_instance(LHS.to_string(), vec![]).unwrap();
    call_instance_intrinsic(&mut e, LHS, "<init>", "()V", set, &[]).unwrap();
    let sb = e.heap.alloc_string("b".into()).unwrap();
    let sa = e.heap.alloc_string("a".into()).unwrap();
    call_instance_intrinsic(
        &mut e,
        LHS,
        "add",
        "(Ljava/lang/Object;)Z",
        set,
        &[Value::Obj(sb)],
    )
    .unwrap();
    call_instance_intrinsic(
        &mut e,
        LHS,
        "add",
        "(Ljava/lang/Object;)Z",
        set,
        &[Value::Obj(sa)],
    )
    .unwrap();

    let list = e.heap.alloc_instance(AL.to_string(), vec![]).unwrap();
    call_instance_intrinsic(
        &mut e,
        AL,
        "<init>",
        "(Ljava/util/Collection;)V",
        list,
        &[Value::Obj(set)],
    )
    .unwrap();
    assert_eq!(
        call_instance_intrinsic(&mut e, AL, "size", "()I", list, &[]).unwrap(),
        Some(Value::Int(2))
    );
    for (i, want) in ["b", "a"].iter().enumerate() {
        match call_instance_intrinsic(
            &mut e,
            AL,
            "get",
            "(I)Ljava/lang/Object;",
            list,
            &[Value::Int(i as i32)],
        )
        .unwrap()
        {
            Some(Value::Obj(r)) => assert_eq!(e.heap.as_str(r).unwrap(), *want),
            other => panic!("get({i}): {other:?}"),
        }
    }

    // Snapshot: remover da fonte NÃO altera a cópia (sem aliasing).
    call_instance_intrinsic(
        &mut e,
        LHS,
        "remove",
        "(Ljava/lang/Object;)Z",
        set,
        &[Value::Obj(sb)],
    )
    .unwrap();
    assert_eq!(
        call_instance_intrinsic(&mut e, AL, "size", "()I", list, &[]).unwrap(),
        Some(Value::Int(2)),
        "cópia deve ser independente da fonte"
    );

    // Fonte 2: outra ArrayList host — duplicatas preservadas (dedup é da fonte).
    call_instance_intrinsic(
        &mut e,
        AL,
        "add",
        "(Ljava/lang/Object;)Z",
        list,
        &[Value::Obj(sa)],
    )
    .unwrap();
    let list2 = e.heap.alloc_instance(AL.to_string(), vec![]).unwrap();
    call_instance_intrinsic(
        &mut e,
        AL,
        "<init>",
        "(Ljava/util/Collection;)V",
        list2,
        &[Value::Obj(list)],
    )
    .unwrap();
    assert_eq!(
        call_instance_intrinsic(&mut e, AL, "size", "()I", list2, &[]).unwrap(),
        Some(Value::Int(3))
    );

    // Fonte null → NPE (c.toArray() na JDK).
    let list3 = e.heap.alloc_instance(AL.to_string(), vec![]).unwrap();
    match call_instance_intrinsic(
        &mut e,
        AL,
        "<init>",
        "(Ljava/util/Collection;)V",
        list3,
        &[Value::Null],
    ) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Ljava/lang/NullPointerException;"),
        other => panic!("esperava NPE, got {other:?}"),
    }

    // Fonte sem estado host (coleção do app) → erro tipado, nunca cópia errada.
    let app_col = e
        .heap
        .alloc_instance("Lcom/exemplo/Col;".to_string(), vec![])
        .unwrap();
    match call_instance_intrinsic(
        &mut e,
        AL,
        "<init>",
        "(Ljava/util/Collection;)V",
        list3,
        &[Value::Obj(app_col)],
    ) {
        Err(VmExit::Error(err)) => assert_eq!(err.code, "VM_TYPE_ERROR"),
        other => panic!("esperava VM_TYPE_ERROR, got {other:?}"),
    }
}

#[test]
fn linked_blocking_queue_drain_to_host() {
    use rd_vm::intrinsics::call_instance_intrinsic;
    use rd_vm::value::Value;
    const Q: &str = "Ljava/util/concurrent/LinkedBlockingQueue;";
    const AL: &str = "Ljava/util/ArrayList;";
    const LHS: &str = "Ljava/util/LinkedHashSet;";
    const DRAIN: &str = "(Ljava/util/Collection;)I";
    const DRAIN_N: &str = "(Ljava/util/Collection;I)I";

    let mut b = DexBuilder::new();
    b.class("Lcom/exemplo/Dst;", "Ljava/lang/Object;");
    let mut e = engine_of(&b);

    let mk_queue = |e: &mut Engine, vals: &[&str]| {
        let q = e.heap.alloc_instance(Q.to_string(), vec![]).unwrap();
        call_instance_intrinsic(e, Q, "<init>", "()V", q, &[]).unwrap();
        for s in vals {
            let o = e.heap.alloc_string((*s).into()).unwrap();
            call_instance_intrinsic(e, Q, "add", "(Ljava/lang/Object;)Z", q, &[Value::Obj(o)])
                .unwrap();
        }
        q
    };
    let mk_list = |e: &mut Engine| {
        let l = e.heap.alloc_instance(AL.to_string(), vec![]).unwrap();
        call_instance_intrinsic(e, AL, "<init>", "()V", l, &[]).unwrap();
        l
    };

    // drainTo(c): esvazia na ordem FIFO; retorno = retirados.
    let q = mk_queue(&mut e, &["a", "b", "c"]);
    let list = mk_list(&mut e);
    assert_eq!(
        call_instance_intrinsic(&mut e, Q, "drainTo", DRAIN, q, &[Value::Obj(list)]).unwrap(),
        Some(Value::Int(3))
    );
    assert_eq!(
        call_instance_intrinsic(&mut e, Q, "size", "()I", q, &[]).unwrap(),
        Some(Value::Int(0))
    );
    assert_eq!(
        call_instance_intrinsic(&mut e, AL, "size", "()I", list, &[]).unwrap(),
        Some(Value::Int(3))
    );
    for (i, want) in ["a", "b", "c"].iter().enumerate() {
        match call_instance_intrinsic(
            &mut e,
            AL,
            "get",
            "(I)Ljava/lang/Object;",
            list,
            &[Value::Int(i as i32)],
        )
        .unwrap()
        {
            Some(Value::Obj(r)) => assert_eq!(e.heap.as_str(r).unwrap(), *want),
            other => panic!("get({i}): {other:?}"),
        }
    }

    // drainTo(c, max): só os primeiros `max`; o resto permanece na fila.
    let q2 = mk_queue(&mut e, &["x", "y", "z"]);
    let list2 = mk_list(&mut e);
    assert_eq!(
        call_instance_intrinsic(
            &mut e,
            Q,
            "drainTo",
            DRAIN_N,
            q2,
            &[Value::Obj(list2), Value::Int(2)]
        )
        .unwrap(),
        Some(Value::Int(2))
    );
    assert_eq!(
        call_instance_intrinsic(&mut e, Q, "size", "()I", q2, &[]).unwrap(),
        Some(Value::Int(1))
    );
    assert_eq!(
        call_instance_intrinsic(&mut e, AL, "size", "()I", list2, &[]).unwrap(),
        Some(Value::Int(2))
    );

    // maxElements <= 0 → 0, nada se move (JDK).
    let q3 = mk_queue(&mut e, &["s"]);
    let list3 = mk_list(&mut e);
    assert_eq!(
        call_instance_intrinsic(
            &mut e,
            Q,
            "drainTo",
            DRAIN_N,
            q3,
            &[Value::Obj(list3), Value::Int(0)]
        )
        .unwrap(),
        Some(Value::Int(0))
    );
    assert_eq!(
        call_instance_intrinsic(&mut e, Q, "size", "()I", q3, &[]).unwrap(),
        Some(Value::Int(1))
    );

    // Destino Set: deduplica no add, mas o retorno conta os RETIRADOS (JDK).
    let q4 = mk_queue(&mut e, &["a", "a", "b"]);
    let set = e.heap.alloc_instance(LHS.to_string(), vec![]).unwrap();
    call_instance_intrinsic(&mut e, LHS, "<init>", "()V", set, &[]).unwrap();
    assert_eq!(
        call_instance_intrinsic(&mut e, Q, "drainTo", DRAIN, q4, &[Value::Obj(set)]).unwrap(),
        Some(Value::Int(3))
    );
    assert_eq!(
        call_instance_intrinsic(&mut e, LHS, "size", "()I", set, &[]).unwrap(),
        Some(Value::Int(2))
    );
    assert_eq!(
        call_instance_intrinsic(&mut e, Q, "size", "()I", q4, &[]).unwrap(),
        Some(Value::Int(0))
    );

    // Destino lazy-init (ArrayList sem <init> host interceptado antes).
    let q5 = mk_queue(&mut e, &["w"]);
    let lazy = e.heap.alloc_instance(AL.to_string(), vec![]).unwrap();
    assert_eq!(
        call_instance_intrinsic(&mut e, Q, "drainTo", DRAIN, q5, &[Value::Obj(lazy)]).unwrap(),
        Some(Value::Int(1))
    );
    assert_eq!(
        call_instance_intrinsic(&mut e, AL, "size", "()I", lazy, &[]).unwrap(),
        Some(Value::Int(1))
    );

    // Contratos: null → NPE; drainTo(this) → IllegalArgumentException; destino
    // do app (sem superfície Collection host) → VM_TYPE_ERROR sem drenar nada.
    let q6 = mk_queue(&mut e, &["k"]);
    match call_instance_intrinsic(&mut e, Q, "drainTo", DRAIN, q6, &[Value::Null]) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Ljava/lang/NullPointerException;"),
        other => panic!("esperava NPE, got {other:?}"),
    }
    match call_instance_intrinsic(&mut e, Q, "drainTo", DRAIN, q6, &[Value::Obj(q6)]) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Ljava/lang/IllegalArgumentException;"),
        other => panic!("esperava IllegalArgumentException, got {other:?}"),
    }
    let app = e
        .heap
        .alloc_instance("Lcom/exemplo/Dst;".to_string(), vec![])
        .unwrap();
    match call_instance_intrinsic(&mut e, Q, "drainTo", DRAIN, q6, &[Value::Obj(app)]) {
        Err(VmExit::Error(err)) => assert_eq!(err.code, "VM_TYPE_ERROR"),
        other => panic!("esperava VM_TYPE_ERROR, got {other:?}"),
    }
    assert_eq!(
        call_instance_intrinsic(&mut e, Q, "size", "()I", q6, &[]).unwrap(),
        Some(Value::Int(1)),
        "falha de contrato não pode drenar a fila"
    );
}

#[test]
fn activity_get_fragment_manager_canonical() {
    use rd_vm::framework::call_host_instance;
    use rd_vm::value::Value;
    const ACT: &str = "Landroid/app/Activity;";
    const FM: &str = "Landroid/app/FragmentManager;";

    let mut b = DexBuilder::new();
    b.class("Lcom/exemplo/MainAct;", ACT);
    b.class("Lcom/exemplo/SubAct;", "Lcom/exemplo/MainAct;");
    let mut e = engine_of(&b);
    let act = e
        .heap
        .alloc_instance("Lcom/exemplo/MainAct;".to_string(), vec![])
        .unwrap();

    let fm1 = match call_host_instance(
        &mut e,
        ACT,
        "getFragmentManager",
        "()Landroid/app/FragmentManager;",
        act,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("getFragmentManager: {other:?}"),
    };
    assert_eq!(e.heap.class_of(fm1).unwrap(), FM);

    // Identidade estável: chamadas repetidas devolvem o MESMO objeto host.
    let fm2 = match call_host_instance(
        &mut e,
        ACT,
        "getFragmentManager",
        "()Landroid/app/FragmentManager;",
        act,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("getFragmentManager 2: {other:?}"),
    };
    assert_eq!(fm1, fm2, "FragmentManager deve ser canônico por VM");

    // Subclasse de Activity também resolve (is_subtype) e compartilha o mesmo FM.
    let sub = e
        .heap
        .alloc_instance("Lcom/exemplo/SubAct;".to_string(), vec![])
        .unwrap();
    let fm3 = match call_host_instance(
        &mut e,
        ACT,
        "getFragmentManager",
        "()Landroid/app/FragmentManager;",
        sub,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("getFragmentManager 3: {other:?}"),
    };
    assert_eq!(fm1, fm3);
    assert!(e.cp.is_subtype(FM, "Ljava/lang/Object;"));
}

#[test]
fn fragment_manager_vazio_find_devolve_null() {
    use rd_vm::framework::call_host_instance;
    use rd_vm::value::Value;
    const ACT: &str = "Landroid/app/Activity;";
    const FM: &str = "Landroid/app/FragmentManager;";

    let mut b = DexBuilder::new();
    b.class("Lcom/exemplo/FindAct;", ACT);
    let mut e = engine_of(&b);
    let act = e
        .heap
        .alloc_instance("Lcom/exemplo/FindAct;".to_string(), vec![])
        .unwrap();
    let fm = match call_host_instance(
        &mut e,
        ACT,
        "getFragmentManager",
        "()Landroid/app/FragmentManager;",
        act,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("getFragmentManager: {other:?}"),
    };

    // FM do host ainda não tem fragments registrados ⇒ null (contrato Android).
    let tag = Value::Obj(e.heap.alloc_string("prefs".into()).unwrap());
    assert_eq!(
        call_host_instance(
            &mut e,
            FM,
            "findFragmentByTag",
            "(Ljava/lang/String;)Landroid/app/Fragment;",
            fm,
            &[tag],
        )
        .unwrap(),
        Some(Value::Null)
    );
    // Tag null também devolve null (FragmentManagerImpl ignora tag null).
    assert_eq!(
        call_host_instance(
            &mut e,
            FM,
            "findFragmentByTag",
            "(Ljava/lang/String;)Landroid/app/Fragment;",
            fm,
            &[Value::Null],
        )
        .unwrap(),
        Some(Value::Null)
    );
    assert_eq!(
        call_host_instance(
            &mut e,
            FM,
            "findFragmentById",
            "(I)Landroid/app/Fragment;",
            fm,
            &[Value::Int(0x7f0a0001)],
        )
        .unwrap(),
        Some(Value::Null)
    );
}

#[test]
fn fragment_transaction_commits_registered() {
    use rd_vm::framework::call_host_instance;
    use rd_vm::value::Value;
    const ACT: &str = "Landroid/app/Activity;";
    const FM: &str = "Landroid/app/FragmentManager;";
    const FT: &str = "Landroid/app/FragmentTransaction;";
    const FRAG: &str = "Lcom/exemplo/PrefsFrag;";
    const ADD_TS: &str =
        "(Landroid/app/Fragment;Ljava/lang/String;)Landroid/app/FragmentTransaction;";
    const ADD_I: &str = "(ILandroid/app/Fragment;)Landroid/app/FragmentTransaction;";
    const ADD_ITS: &str =
        "(ILandroid/app/Fragment;Ljava/lang/String;)Landroid/app/FragmentTransaction;";
    const REPLACE_I: &str = "(ILandroid/app/Fragment;)Landroid/app/FragmentTransaction;";
    const REMOVE_F: &str = "(Landroid/app/Fragment;)Landroid/app/FragmentTransaction;";
    const FIND_TAG: &str = "(Ljava/lang/String;)Landroid/app/Fragment;";
    const FIND_ID: &str = "(I)Landroid/app/Fragment;";

    let mut b = DexBuilder::new();
    b.class("Lcom/exemplo/TxAct;", ACT);
    b.class(FRAG, "Ljava/lang/Object;");
    let mut e = engine_of(&b);
    let act = e
        .heap
        .alloc_instance("Lcom/exemplo/TxAct;".to_string(), vec![])
        .unwrap();
    let fm = match call_host_instance(
        &mut e,
        ACT,
        "getFragmentManager",
        "()Landroid/app/FragmentManager;",
        act,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("getFragmentManager: {other:?}"),
    };
    let new_frag = |e: &mut Engine| e.heap.alloc_instance(FRAG.to_string(), vec![]).unwrap();
    let find_tag = |e: &mut Engine, t: &str| {
        let ts = Value::Obj(e.heap.alloc_string(t.into()).unwrap());
        call_host_instance(e, FM, "findFragmentByTag", FIND_TAG, fm, &[ts]).unwrap()
    };
    let find_id = |e: &mut Engine, id: i32| {
        call_host_instance(e, FM, "findFragmentById", FIND_ID, fm, &[Value::Int(id)]).unwrap()
    };

    // beginTransaction: objeto novo por chamada; ops sem commit não têm efeito.
    let tx0 = match call_host_instance(
        &mut e,
        FM,
        "beginTransaction",
        "()Landroid/app/FragmentTransaction;",
        fm,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("beginTransaction: {other:?}"),
    };
    assert_eq!(e.heap.class_of(tx0).unwrap(), FT);
    let f0 = new_frag(&mut e);
    let tag0 = Value::Obj(e.heap.alloc_string("orcamento".into()).unwrap());
    assert_eq!(
        call_host_instance(&mut e, FT, "add", ADD_TS, tx0, &[Value::Obj(f0), tag0]).unwrap(),
        Some(Value::Obj(tx0)),
        "add deve ser fluente (devolve a própria tx)"
    );
    assert_eq!(
        find_tag(&mut e, "orcamento"),
        Some(Value::Null),
        "sem commit não registra"
    );

    // commit aplica: find por tag/id passa a ver a fragment registrada.
    let tx = match call_host_instance(
        &mut e,
        FM,
        "beginTransaction",
        "()Landroid/app/FragmentTransaction;",
        fm,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("beginTransaction 2: {other:?}"),
    };
    assert_ne!(tx, tx0);
    let prefs = new_frag(&mut e);
    let tag_prefs = Value::Obj(e.heap.alloc_string("prefs".into()).unwrap());
    call_host_instance(
        &mut e,
        FT,
        "add",
        ADD_TS,
        tx,
        &[Value::Obj(prefs), tag_prefs],
    )
    .unwrap();
    let side = new_frag(&mut e);
    call_host_instance(
        &mut e,
        FT,
        "add",
        ADD_I,
        tx,
        &[Value::Int(0x7f0a0001), Value::Obj(side)],
    )
    .unwrap();
    assert_eq!(
        call_host_instance(&mut e, FT, "commit", "()I", tx, &[]).unwrap(),
        Some(Value::Int(0))
    );
    assert_eq!(find_tag(&mut e, "prefs"), Some(Value::Obj(prefs)));
    assert_eq!(find_id(&mut e, 0x7f0a0001), Some(Value::Obj(side)));

    // replace no MESMO container remove a anterior; commitAllowingStateLoss idem.
    let tx2 = match call_host_instance(
        &mut e,
        FM,
        "beginTransaction",
        "()Landroid/app/FragmentTransaction;",
        fm,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("beginTransaction 3: {other:?}"),
    };
    let newer = new_frag(&mut e);
    call_host_instance(
        &mut e,
        FT,
        "replace",
        REPLACE_I,
        tx2,
        &[Value::Int(0x7f0a0001), Value::Obj(newer)],
    )
    .unwrap();
    call_host_instance(&mut e, FT, "commitAllowingStateLoss", "()I", tx2, &[]).unwrap();
    assert_eq!(find_id(&mut e, 0x7f0a0001), Some(Value::Obj(newer)));

    // remove + contratos: 2º commit → ISE; add null → NPE; Add de fragment já
    // registrada → ISE sem aplicação parcial; remove após commit → ISE.
    let tx3 = match call_host_instance(
        &mut e,
        FM,
        "beginTransaction",
        "()Landroid/app/FragmentTransaction;",
        fm,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("beginTransaction 4: {other:?}"),
    };
    call_host_instance(&mut e, FT, "remove", REMOVE_F, tx3, &[Value::Obj(newer)]).unwrap();
    call_host_instance(&mut e, FT, "commit", "()I", tx3, &[]).unwrap();
    assert_eq!(find_id(&mut e, 0x7f0a0001), Some(Value::Null));
    match call_host_instance(&mut e, FT, "commit", "()I", tx3, &[]) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Ljava/lang/IllegalStateException;"),
        other => panic!("esperava ISE no 2º commit, got {other:?}"),
    }
    match call_host_instance(&mut e, FT, "remove", REMOVE_F, tx3, &[Value::Obj(prefs)]) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Ljava/lang/IllegalStateException;"),
        other => panic!("esperava ISE pós-commit, got {other:?}"),
    }
    let tx4 = match call_host_instance(
        &mut e,
        FM,
        "beginTransaction",
        "()Landroid/app/FragmentTransaction;",
        fm,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("beginTransaction 5: {other:?}"),
    };
    match call_host_instance(
        &mut e,
        FT,
        "add",
        ADD_ITS,
        tx4,
        &[Value::Int(1), Value::Null, Value::Null],
    ) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Ljava/lang/NullPointerException;"),
        other => panic!("esperava NPE add null, got {other:?}"),
    }
    let tx5 = match call_host_instance(
        &mut e,
        FM,
        "beginTransaction",
        "()Landroid/app/FragmentTransaction;",
        fm,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("beginTransaction 6: {other:?}"),
    };
    let tag_dup = Value::Obj(e.heap.alloc_string("dup".into()).unwrap());
    call_host_instance(
        &mut e,
        FT,
        "add",
        ADD_TS,
        tx5,
        &[Value::Obj(prefs), tag_dup],
    )
    .unwrap();
    match call_host_instance(&mut e, FT, "commit", "()I", tx5, &[]) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Ljava/lang/IllegalStateException;"),
        other => panic!("esperava ISE already-added, got {other:?}"),
    }
    assert_eq!(
        find_tag(&mut e, "dup"),
        Some(Value::Null),
        "falha de commit não aplica parcialmente"
    );
}

#[test]
fn fragment_init_e_subtipagem() {
    use rd_vm::framework::call_host_instance;
    use rd_vm::value::Value;
    const FRAG_CLS: &str = "Landroid/app/Fragment;";

    let mut b = DexBuilder::new();
    b.class("Lcom/exemplo/MiniFrag;", FRAG_CLS);
    let mut e = engine_of(&b);
    let frag = e
        .heap
        .alloc_instance("Lcom/exemplo/MiniFrag;".to_string(), vec![])
        .unwrap();

    // ctor do super (invokespecial da fragment do app): no-op host, nunca erro.
    assert_eq!(
        call_host_instance(&mut e, FRAG_CLS, "<init>", "()V", frag, &[]).unwrap(),
        Some(Value::Null)
    );
    assert!(e.cp.is_subtype("Lcom/exemplo/MiniFrag;", FRAG_CLS));
    assert!(e.cp.is_subtype(FRAG_CLS, "Ljava/lang/Object;"));
}

#[test]
fn fragment_manager_execute_pending_sempre_false() {
    use rd_vm::framework::call_host_instance;
    use rd_vm::value::Value;
    const ACT: &str = "Landroid/app/Activity;";
    const FM: &str = "Landroid/app/FragmentManager;";

    let mut b = DexBuilder::new();
    b.class("Lcom/exemplo/PendAct;", ACT);
    let mut e = engine_of(&b);
    let act = e
        .heap
        .alloc_instance("Lcom/exemplo/PendAct;".to_string(), vec![])
        .unwrap();
    let fm = match call_host_instance(
        &mut e,
        ACT,
        "getFragmentManager",
        "()Landroid/app/FragmentManager;",
        act,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("getFragmentManager: {other:?}"),
    };

    // Sem transação alguma: false (nada executado).
    assert_eq!(
        call_host_instance(&mut e, FM, "executePendingTransactions", "()Z", fm, &[]).unwrap(),
        Some(Value::Int(0))
    );
    // Após commit real: ainda false — o commit já aplicou sincronamente.
    let tx = match call_host_instance(
        &mut e,
        FM,
        "beginTransaction",
        "()Landroid/app/FragmentTransaction;",
        fm,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("beginTransaction: {other:?}"),
    };
    const FT: &str = "Landroid/app/FragmentTransaction;";
    call_host_instance(&mut e, FT, "commit", "()I", tx, &[]).unwrap();
    assert_eq!(
        call_host_instance(&mut e, FM, "executePendingTransactions", "()Z", fm, &[]).unwrap(),
        Some(Value::Int(0))
    );
}

#[test]
fn enum_clone_lanca_clone_not_supported() {
    use rd_vm::intrinsics::call_instance_intrinsic_mref;
    use rd_vm::value::Value;
    const ENUM: &str = "Ljava/lang/Enum;";
    const MY: &str = "LCor;";

    let mut b = DexBuilder::new();
    b.class(MY, ENUM);
    let mut e = engine_of(&b);

    let cor = e.heap.alloc_instance(MY.to_string(), vec![]).unwrap();
    let nome = e.heap.alloc_string("VERMELHO".into()).unwrap();
    call_instance_intrinsic_mref(
        &mut e,
        MY,
        ENUM,
        "<init>",
        "(Ljava/lang/String;I)V",
        cor,
        &[Value::Obj(nome), Value::Int(0)],
    )
    .unwrap();

    // 34º gap real: Enum.clone() é final e SEMPRE lança (JDK/libcore).
    // method ref DECLARA a subclasse (caso real do androidx Lifecycle$Event).
    match call_instance_intrinsic_mref(&mut e, MY, MY, "clone", "()Ljava/lang/Object;", cor, &[]) {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/CloneNotSupportedException;");
            assert!(t
                .message
                .unwrap_or_default()
                .contains("Cannot clone an enum"));
        }
        other => panic!("esperava CloneNotSupportedException, got {other:?}"),
    }
    // method ref declarando Enum diretamente: mesmo resultado.
    match call_instance_intrinsic_mref(&mut e, MY, ENUM, "clone", "()Ljava/lang/Object;", cor, &[])
    {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Ljava/lang/CloneNotSupportedException;"),
        other => panic!("esperava CNSE via mref Enum, got {other:?}"),
    }
    // clone NÃO copiou nem corrompeu o estado: name/ordinal seguem íntegros.
    assert_eq!(
        call_instance_intrinsic_mref(&mut e, MY, MY, "name", "()Ljava/lang/String;", cor, &[])
            .unwrap(),
        Some(Value::Obj(nome))
    );
    assert_eq!(
        call_instance_intrinsic_mref(&mut e, MY, MY, "ordinal", "()I", cor, &[]).unwrap(),
        Some(Value::Int(0))
    );
}

#[test]
fn array_clone_e_shallow_copy_com_descritor() {
    use rd_vm::intrinsics::call_instance_intrinsic_mref;
    use rd_vm::value::Value;

    let mut b = DexBuilder::new();
    b.class("Lcom/exemplo/Evt;", "Ljava/lang/Object;");
    let mut e = engine_of(&b);

    // array de objetos [Lcom/exemplo/Evt; com 2 elementos (referências).
    let src = e
        .heap
        .alloc_array(rd_vm::heap::ElemKind::Obj, 2, "[Lcom/exemplo/Evt;")
        .unwrap();
    let f0 = e
        .heap
        .alloc_instance("Lcom/exemplo/Evt;".to_string(), vec![])
        .unwrap();
    let f1 = e
        .heap
        .alloc_instance("Lcom/exemplo/Evt;".to_string(), vec![])
        .unwrap();
    {
        let elems = e.heap.array_elems_mut(src).unwrap();
        elems.push(Value::Obj(f0));
        elems.push(Value::Obj(f1));
    }

    let dst = match call_instance_intrinsic_mref(
        &mut e,
        "[Lcom/exemplo/Evt;",
        "[Lcom/exemplo/Evt;",
        "clone",
        "()Ljava/lang/Object;",
        src,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("array.clone: {other:?}"),
    };
    assert_ne!(dst, src, "clone deve ser objeto novo");
    assert_eq!(
        e.heap.class_of(dst).unwrap(),
        "[Lcom/exemplo/Evt;",
        "descritor real preservado (issue #37)"
    );
    let elems = e.heap.as_array_elems(dst).unwrap().0;
    assert_eq!(
        elems,
        &[Value::Obj(f0), Value::Obj(f1)],
        "shallow: MESMAS referências"
    );
    // mutação no clone NÃO toca o original (cópia do Vec, compartilhamento dos elementos)
    e.heap.array_elems_mut(dst).unwrap().remove(0);
    assert_eq!(e.heap.as_array_elems(src).unwrap().0.len(), 2);
}

#[test]
fn exception_trace_anota_frames_ao_desempilhar() {
    let mut b = DexBuilder::new();
    let p_void = b.proto_idx("V", vec![]);
    let caso = b.class("LCaso;", "Ljava/lang/Object;");
    let bomba = b.method_idx("LCaso;", p_void, "bomba");
    // bomba()V: aget em array null → NPE Java materializado
    b.direct(
        caso,
        "bomba",
        "V",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(2, 0, 0, {
            let mut u = op11n(0x12, 0, 0); // const/4 v0, +0 (null)
            u.extend(op23x(0x44, 1, 0, 0)); // aget v1, v0, v0
            u.extend(op11x(0x0E, 0)); // return-void
            u
        })),
    );
    // chama()V: invoke-static bomba()V — frame intermediário do unwind
    b.direct(
        caso,
        "chama",
        "V",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(0, 0, 0, {
            let mut u = op35c(0x71, 0, bomba, [0, 0, 0, 0, 0]); // invoke-static {}
            u.extend(op11x(0x0E, 0));
            u
        })),
    );

    // Default: exceção LIMPA, sem linhas de trace (saídas existentes intactas).
    let dex = rd_dex::Dex::parse(b.finish()).expect("dex");
    let mut e_default = Engine::new(vec![dex.clone()], VmConfig::default());
    let msg_default = match invoke(&mut e_default, "chama", "()V", &[]) {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/NullPointerException;");
            t.message.unwrap_or_default()
        }
        other => panic!("esperava NPE, got {other:?}"),
    };
    assert!(
        !msg_default.contains("at LCaso;"),
        "default NÃO pode anotar trace: {msg_default}"
    );

    // exception_trace ON: cada frame desempilhado anota a si mesmo,
    // innermost primeiro (bomba → chama), como as linhas "at" da JVM.
    let mut e_trace = Engine::new(
        vec![dex],
        VmConfig {
            exception_trace: true,
            ..VmConfig::default()
        },
    );
    let msg_trace = match invoke(&mut e_trace, "chama", "()V", &[]) {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/NullPointerException;");
            t.message.unwrap_or_default()
        }
        other => panic!("esperava NPE (trace), got {other:?}"),
    };
    let bomba_ln = msg_trace
        .find("at LCaso;->bomba()V")
        .expect("frame bomba anotado");
    let chama_ln = msg_trace
        .find("at LCaso;->chama()V")
        .expect("frame chama anotado");
    assert!(
        bomba_ln < chama_ln,
        "innermost primeiro (bomba ~{bomba_ln} < chama ~{chama_ln}): {msg_trace}"
    );
    assert!(
        msg_trace.starts_with("aget em null\n  at "),
        "mensagem original preservada no início: {msg_trace}"
    );

    // Postmortem (AG-09): cobertura de entrada sempre gravada — os dois
    // frames estão no conjunto executado, em saída ordenada/determinística.
    // É o dado que responde "esse método rodou?" após o engine morrer.
    let labels = e_trace.executed_labels();
    assert!(labels.iter().any(|l| l == "LCaso;->bomba()V"));
    assert!(labels.iter().any(|l| l == "LCaso;->chama()V"));
    let mut sorted = labels.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(labels, sorted, "executed_labels ordenado/dedup");
}

#[test]
fn invoke_super_comeca_na_super_da_classe_definidora() {
    // Semântica DEX (Dalvik): invoke-super inicia a busca na SUPER da classe
    // ONDE a instrução está (defining class), NÃO em super(mref.class).
    // LAvo.valor=7 ← LApai.valor=8 ← LAneto.chamaSuper: invoke-super
    // LApai;->valor a partir de LAneto deve resolver para LApai.valor (8).
    // Regra errada (super(mref.class)) resolveria para LAvo.valor (7) —
    // bug real observado no launch do primftpd: FragmentActivity.onCreate
    // pulava androidx/activity/ComponentActivity.onCreate direto para
    // androidx/core/app/ComponentActivity.onCreate (issue de M8, 36º gap).
    let mut b = DexBuilder::new();
    let avo = b.class("LLAvo;", "Ljava/lang/Object;");
    b.r#virtual(
        avo,
        "valor",
        "I",
        vec![],
        ACC_PUBLIC,
        Some(b.code(2, 1, 0, {
            let mut u = op21s(0x13, 0, 7);
            u.extend(op11x(0x0F, 0)); // return v0
            u
        })),
    );
    let pai = b.class("LLPai;", "LLAvo;");
    b.r#virtual(
        pai,
        "valor",
        "I",
        vec![],
        ACC_PUBLIC,
        Some(b.code(2, 1, 0, {
            let mut u = op21s(0x13, 0, 8);
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    let neto = b.class("LLNeto;", "LLPai;");
    b.r#virtual(
        neto,
        "valor",
        "I",
        vec![],
        ACC_PUBLIC,
        Some(b.code(2, 1, 0, {
            let mut u = op21s(0x13, 0, 9);
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    let p_i = b.proto_idx("I", vec![]);
    let valor_pai = b.method_idx("LLPai;", p_i, "valor");
    b.r#virtual(
        neto,
        "chamaSuper",
        "I",
        vec![],
        ACC_PUBLIC,
        Some(b.code(2, 1, 1, {
            // regs: v0 scratch, v1=this(p0); invoke-super {v1} LLPai;->valor()I
            let mut u = op35c(0x6F, 1, valor_pai, [1, 0, 0, 0, 0]);
            u.extend(op11x(0x0A, 0)); // move-result v0
            u.extend(op11x(0x0F, 0)); // return v0
            u
        })),
    );
    let neto_tidx = b.type_idx("LLNeto;");
    let chama_super = b.method_idx("LLNeto;", p_i, "chamaSuper");
    let caso = b.class("LCaso;", "Ljava/lang/Object;");
    b.direct(
        caso,
        "despacha",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(2, 0, 1, {
            let mut u = op21c(0x22, 0, neto_tidx); // new-instance v0, LLNeto (resultado já em v0)
            u.extend(op35c(0x6E, 1, chama_super, [0, 0, 0, 0, 0])); // invoke-virtual
            u.extend(op11x(0x0A, 1)); // move-result v1
            u.extend(op11x(0x0F, 1)); // return v1
            u
        })),
    );
    let dex = rd_dex::Dex::parse(b.finish()).expect("dex");
    let mut e = Engine::new(vec![dex], VmConfig::default());
    let r = invoke(&mut e, "despacha", "()I", &[])
        .expect("despacha")
        .as_int()
        .expect("int");
    assert_eq!(
        r, 8,
        "invoke-super DEX-corrto: LApai.valor (8), não LAvo.valor (7)"
    );
}

#[test]
fn invoke_super_object_equals_uses_reference_identity() {
    let mut b = DexBuilder::new();
    let base = b.class("LIdentityEqualsBase;", "Ljava/lang/Object;");
    let equals_proto = b.proto_idx("Z", vec!["Ljava/lang/Object;".into()]);
    let object_equals = b.method_idx("Ljava/lang/Object;", equals_proto, "equals");
    b.r#virtual(
        base,
        "equals",
        "Z",
        vec!["Ljava/lang/Object;"],
        ACC_PUBLIC,
        Some(b.code(2, 2, 2, {
            let mut code = op35c(0x6F, 2, object_equals, [0, 1, 0, 0, 0]);
            code.extend(op11x(0x0A, 0)); // move-result v0
            code.extend(op11x(0x0F, 0)); // return v0
            code
        })),
    );
    let mut e = engine_of(&b);
    let first = e
        .heap
        .alloc_instance("LIdentityEqualsBase;".into(), vec![])
        .unwrap();
    let second = e
        .heap
        .alloc_instance("LIdentityEqualsBase;".into(), vec![])
        .unwrap();
    assert_eq!(
        e.invoke_instance(
            first,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(first)],
        )
        .unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        e.invoke_instance(
            first,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(second)],
        )
        .unwrap(),
        Value::Int(0)
    );
}

#[test]
fn subclasse_dex_de_abstractlist_chama_super_init_noop() {
    // M8 (37º gap, real no launch do primftpd pós-fix do invoke-super):
    // classe DEX estendendo DIRETAMENTE java.util.AbstractList chama
    // super() via invoke-direct → <init>()V host no-op (antes: NOT_FOUND
    // "método Ljava/util/AbstractList;-><init>()V não resolvido").
    let mut b = DexBuilder::new();
    let p_void = b.proto_idx("V", vec![]);
    let p_b = b.proto_idx("Z", vec![]);
    let lista = b.class("LMinha;", "Ljava/util/AbstractList;");
    let init_ab = b.method_idx("Ljava/util/AbstractList;", p_void, "<init>");
    b.direct(
        lista,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, init_ab, [0, 0, 0, 0, 0]); // invoke-direct {v0=this} AbstractList.<init>
            u.extend(op11x(0x0E, 0)); // return-void
            u
        })),
    );
    let minha_tidx = b.type_idx("LMinha;");
    let init_minha = b.method_idx("LMinha;", p_void, "<init>");
    let caso = b.class("LCaso;", "Ljava/lang/Object;"); // assert via retorno

    b.direct(
        caso,
        "main",
        "Z",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(3, 0, 1, {
            let mut u = op21c(0x22, 0, minha_tidx); // new-instance v0, LMinha
            u.extend(op35c(0x70, 1, init_minha, [0, 0, 0, 0, 0])); // invoke-direct {v0} LMinha.<init>
            u.extend(op11n(0x12, 1, 1)); // const/4 v1, +1
            u.extend(op11x(0x0F, 1)); // return v1
            u
        })),
    );
    let _ = p_b;
    let dex = rd_dex::Dex::parse(b.finish()).expect("dex");
    let mut e = Engine::new(vec![dex], VmConfig::default());
    let v = invoke(&mut e, "main", "()Z", &[])
        .expect("main sem NOT_FOUND")
        .as_int()
        .expect("Z");
    assert_eq!(
        v, 1,
        "subclasse DEX de AbstractList instanciou via super() no-op"
    );
}

#[test]
fn dex_subclass_of_copy_on_write_list_gets_host_constructor_and_methods() {
    // Regressão do launch do org.primftpd 8.2: logback TurboFilterList é
    // subclasse DEX de CopyOnWriteArrayList. O invokespecial do construtor
    // host precisa ligar HostObj::List ao receiver DEX; chamadas List.add/size
    // herdadas devem usar esse estado, sem ocultar overrides DEX.
    const COW: &str = "Ljava/util/concurrent/CopyOnWriteArrayList;";
    const SUB: &str = "Lch/qos/logback/classic/spi/TurboFilterList;";

    let mut b = DexBuilder::new();
    let p_void = b.proto_idx("V", vec![]);
    let p_add = b.proto_idx("Z", vec!["Ljava/lang/Object;".into()]);
    let p_size = b.proto_idx("I", vec![]);
    let sub = b.class(SUB, COW);
    let init_cow = b.method_idx(COW, p_void, "<init>");
    b.direct(
        sub,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, init_cow, [0, 0, 0, 0, 0]);
            u.extend(op11x(0x0E, 0)); // return-void
            u
        })),
    );

    let sub_type = b.type_idx(SUB);
    let init_sub = b.method_idx(SUB, p_void, "<init>");
    let add_list = b.method_idx("Ljava/util/List;", p_add, "add");
    let size_list = b.method_idx("Ljava/util/List;", p_size, "size");
    let item = b.intern("host-list-item") as u16;
    let caller = b.class("LCaso;", "Ljava/lang/Object;");
    b.direct(
        caller,
        "exercise",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(4, 0, 2, {
            let mut u = op21c(0x22, 0, sub_type); // new-instance TurboFilterList
            u.extend(op35c(0x70, 1, init_sub, [0, 0, 0, 0, 0])); // subclass ctor
            u.extend(op21c(0x1A, 1, item)); // const-string v1, "host-list-item"
            u.extend(op35c(0x72, 2, add_list, [0, 1, 0, 0, 0])); // List.add(receiver,item)
            u.extend(op11x(0x0A, 2)); // move-result v2 (boolean)
            u.extend(op35c(0x72, 1, size_list, [0, 0, 0, 0, 0])); // List.size(receiver)
            u.extend(op11x(0x0A, 3)); // move-result v3
            u.extend(op11x(0x0F, 3)); // return v3
            u
        })),
    );

    let dex = rd_dex::Dex::parse(b.finish()).expect("DEX sintético parseia");
    let mut e = Engine::new(vec![dex], VmConfig::default());
    assert_eq!(
        invoke(&mut e, "exercise", "()I", &[])
            .expect("super COW constructor + métodos List herdados")
            .as_int()
            .unwrap(),
        1
    );
}

#[test]
fn access_controller_runs_dex_privileged_actions_inline() {
    const ACTION: &str = "Ltest/PropertyAction;";
    const ACCESS_CONTROLLER: &str = "Ljava/security/AccessController;";

    let mut b = DexBuilder::new();
    let action = b.class(ACTION, "Ljava/lang/Object;");
    let result = b.intern("privileged-result") as u16;
    b.r#virtual(
        action,
        "run",
        "Ljava/lang/Object;",
        vec![],
        ACC_PUBLIC,
        Some(b.code(2, 1, 0, {
            let mut u = op21c(0x1A, 0, result); // const-string v0, result
            u.extend(op11x(0x11, 0)); // return-object v0
            u
        })),
    );

    let dex = rd_dex::Dex::parse(b.finish()).expect("DEX de action parseia");
    let mut vm = Engine::new(vec![dex], VmConfig::default());
    let action_ref = vm
        .heap
        .alloc_instance(ACTION.to_string(), Vec::new())
        .expect("aloca PrivilegedAction DEX");

    // Os três overloads usuais de cada interface ignoram o contexto de
    // segurança, ausente neste modelo, mas preservam o callback e o retorno.
    let signatures = [
        "(Ljava/security/PrivilegedAction;)Ljava/lang/Object;",
        "(Ljava/security/PrivilegedAction;Ljava/security/AccessControlContext;)Ljava/lang/Object;",
        "(Ljava/security/PrivilegedAction;Ljava/security/AccessControlContext;[Ljava/security/Permission;)Ljava/lang/Object;",
        "(Ljava/security/PrivilegedExceptionAction;)Ljava/lang/Object;",
        "(Ljava/security/PrivilegedExceptionAction;Ljava/security/AccessControlContext;)Ljava/lang/Object;",
        "(Ljava/security/PrivilegedExceptionAction;Ljava/security/AccessControlContext;[Ljava/security/Permission;)Ljava/lang/Object;",
    ];
    for sig in signatures {
        let mut args = vec![Value::Obj(action_ref)];
        if sig.contains("AccessControlContext") {
            args.push(Value::Null);
        }
        if sig.contains("[Ljava/security/Permission;") {
            args.push(Value::Null);
        }
        let value = vm
            .invoke_static(ACCESS_CONTROLLER, "doPrivileged", sig, &args)
            .expect("doPrivileged executa run() e devolve seu resultado");
        let Value::Obj(string) = value else {
            panic!("resultado não é String: {value:?}");
        };
        assert_eq!(vm.heap.as_str(string).unwrap(), "privileged-result");
    }

    match vm.invoke_static(
        ACCESS_CONTROLLER,
        "doPrivileged",
        "(Ljava/security/PrivilegedAction;)Ljava/lang/Object;",
        &[Value::Null],
    ) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Ljava/lang/NullPointerException;"),
        other => panic!("action nula deveria lançar NullPointerException, got {other:?}"),
    }
}

#[test]
fn runtime_permission_constructor_and_access_controller_check_permission_work() {
    const RUNTIME_PERMISSION: &str = "Ljava/lang/RuntimePermission;";

    let mut b = DexBuilder::new();
    let runtime_permission = b.type_idx(RUNTIME_PERMISSION);
    let name = b.intern("getClassLoader") as u16;
    let p_string = b.proto_idx("V", vec!["Ljava/lang/String;".into()]);
    let init = b.method_idx(RUNTIME_PERMISSION, p_string, "<init>");
    let p_get_name = b.proto_idx("Ljava/lang/String;", vec![]);
    let get_name = b.method_idx("Ljava/security/Permission;", p_get_name, "getName");
    let p_check = b.proto_idx("V", vec!["Ljava/security/Permission;".into()]);
    let check = b.method_idx(
        "Ljava/security/AccessController;",
        p_check,
        "checkPermission",
    );
    let caller = b.class("LCaso;", "Ljava/lang/Object;");
    b.direct(
        caller,
        "permissionName",
        "Ljava/lang/String;",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(3, 0, 2, {
            let mut u = op21c(0x22, 0, runtime_permission);
            u.extend(op21c(0x1A, 1, name)); // const-string v1, "getClassLoader"
            u.extend(op35c(0x70, 2, init, [0, 1, 0, 0, 0])); // RuntimePermission.<init>
            u.extend(op35c(0x6E, 1, get_name, [0, 0, 0, 0, 0])); // Permission.getName
            u.extend(op11x(0x0A, 1)); // move-result-object v1
            u.extend(op35c(0x71, 1, check, [0, 0, 0, 0, 0])); // checkPermission
            u.extend(op11x(0x11, 1)); // return-object v1
            u
        })),
    );

    let dex = rd_dex::Dex::parse(b.finish()).expect("DEX de Permission parseia");
    let mut vm = Engine::new(vec![dex], VmConfig::default());
    let value = invoke(&mut vm, "permissionName", "()Ljava/lang/String;", &[])
        .expect("RuntimePermission é construído e checkPermission é no-op");
    let Value::Obj(string) = value else {
        panic!("Permission.getName não devolveu String: {value:?}");
    };
    assert_eq!(vm.heap.as_str(string).unwrap(), "getClassLoader");
}

#[test]
fn invoke_range_direct_ao_declarado_e_virtual_com_dispatch() {
    // M8 (37º gap, real do launch do primftpd): as variantes /range de
    // invoke-virtual (0x74) e invoke-direct (0x76) estavam TROCADAS no
    // do_invoke. Sob o bug:
    //   (a) invoke-direct/range despachava VIRTUALMENTE → ctor Kotlin de
    //       lambda (FunctionReferenceImpl.<init> → super FunctionReference)
    //       re-executava o próprio <init> em recursão infinita;
    //   (b) invoke-virtual/range NÃO despachava virtualmente → resolvia na
    //       classe DECLARADA em vez da classe do receiver.
    // Formato 3rc do range invoke: [op | count<<8] [midx] [start_reg].
    let range_invoke = |op: u8, count: u8, idx: u16, start: u8| -> Vec<u16> {
        vec![(op as u16) | ((count as u16) << 8), idx, start as u16]
    };
    let mut b = DexBuilder::new();
    let p_void = b.proto_idx("V", vec![]);
    let p_iv = b.proto_idx("V", vec!["I".to_string()]);
    let init_object = b.method_idx("Ljava/lang/Object;", p_void, "<init>");

    // (a) LAvo.<init>(I)V guarda o arg no campo x (iput host-checável).
    let avo = b.class("LAvo;", "Ljava/lang/Object;");
    b.instance_field(avo, "x", "I");
    let fx = b.field_idx("LAvo;", "I", "x");
    b.direct(
        avo,
        "<init>",
        "V",
        vec!["I"],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(2, 2, 1, {
            // regs=2 ins=2 → v0=p0(this), v1=p1(arg)
            let mut u = op35c(0x70, 1, init_object, [0, 0, 0, 0, 0]); // Object.<init> {v0}
            u.extend(op22c(0x59, 1, 0, fx)); // iput v1(arg), v0(this), x
            u.extend(op10x(0x0E));
            u
        })),
    );

    // LSub.<init>()V: chama o ctor do PAI via invoke-direct/range (0x76)
    // com v1=42 — sob o bug re-executaria LSub.<init> (dispatch virtual
    // por engano) em recursão infinita; correto: roda LAvo.<init>(42).
    let sub = b.class("LSub;", "LAvo;");
    let init_avo = b.method_idx("LAvo;", p_iv, "<init>");
    b.direct(
        sub,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(4, 1, 2, {
            // regs=4 ins=1 → v3=p0(this); v0,v1 livres: copia this → v0
            let mut u = op12x(0x07, 0, 3); // move-object v0, v3 (this)
            u.extend(op21s(0x13, 1, 42)); // const/16 v1, 42 (arg)
            u.extend(range_invoke(0x76, 2, init_avo, 0)); // invoke-direct/range {v0..v1} = {this, 42}
            u.extend(op10x(0x0E));
            u
        })),
    );

    // (b) LBaseF.f()I = 7; LSubF sobrescreve com 9 — invoke-virtual/range
    // (0x74) TEM que despachar para a override (9), não para a declarada (7).
    let basef = b.class("LBaseF;", "Ljava/lang/Object;");
    b.r#virtual(
        basef,
        "f",
        "I",
        vec![],
        ACC_PUBLIC,
        Some(b.code(2, 1, 0, {
            let mut u = op21s(0x13, 0, 7);
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    let subf = b.class("LSubF;", "LBaseF;");
    b.r#virtual(
        subf,
        "f",
        "I",
        vec![],
        ACC_PUBLIC,
        Some(b.code(2, 1, 0, {
            let mut u = op21s(0x13, 0, 9);
            u.extend(op11x(0x0F, 0));
            u
        })),
    );

    let p_i = b.proto_idx("I", vec![]);
    let f_base = b.method_idx("LBaseF;", p_i, "f");
    let sub_tidx = b.type_idx("LSub;");
    let init_sub = b.method_idx("LSub;", p_void, "<init>");
    let subf_tidx = b.type_idx("LSubF;");
    let caso = b.class("LCaso;", "Ljava/lang/Object;");
    // le_x()I: new LSub; iget x → esperado 42
    b.direct(
        caso,
        "le_x",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(2, 0, 2, {
            let mut u = op21c(0x22, 0, sub_tidx);
            u.extend(op35c(0x70, 1, init_sub, [0, 0, 0, 0, 0]));
            u.extend(op22c(0x52, 1, 0, fx)); // iget v1, v0, x
            u.extend(op11x(0x0F, 1));
            u
        })),
    );
    // chama_f()I: new LSubF; invoke-virtual/range {v0} LBaseF.f → esperado 9
    b.direct(
        caso,
        "chama_f",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(2, 0, 1, {
            let mut u = op21c(0x22, 0, subf_tidx);
            u.extend(op35c(0x70, 1, init_object, [0, 0, 0, 0, 0])); // Object.<init> (no-op)
            u.extend(range_invoke(0x74, 1, f_base, 0)); // invoke-virtual/range {v0}
            u.extend(op11x(0x0A, 1)); // move-result v1
            u.extend(op11x(0x0F, 1));
            u
        })),
    );
    let dex = rd_dex::Dex::parse(b.finish()).expect("dex");
    let mut e = Engine::new(vec![dex], VmConfig::default());
    let x = invoke(&mut e, "le_x", "()I", &[])
        .expect("le_x")
        .as_int()
        .expect("int");
    assert_eq!(
        x, 42,
        "invoke-direct/range (0x76) resolveu na classe declarada"
    );
    let f = invoke(&mut e, "chama_f", "()I", &[])
        .expect("chama_f")
        .as_int()
        .expect("int");
    assert_eq!(f, 9, "invoke-virtual/range (0x74) despachou virtualmente");
}

#[test]
fn get_application_devolve_application_canonica_do_manifest() {
    // M8 (38º gap, real do launch do primftpd): MainTabsActivity.onCreate
    // chamava getApplication() e o método não resolvia (mref pela própria
    // classe do app — não Landroid/). Agora: launch cria a Application do
    // android:name, roda o onCreate dela ANTES do da activity e
    // getApplication() devolve SEMPRE a mesma instância (canônica).
    let mut b = DexBuilder::new();
    let _p_void = b.proto_idx("V", vec![]);
    // LApp: Application custom com marcador estático escrito no onCreate
    let app = b.class("LApp;", "Landroid/app/Application;");
    b.static_field(app, "ordem", "I", SVal::Int(0));
    let ordem = b.field_idx("LApp;", "I", "ordem");
    b.direct(
        app,
        "onCreate",
        "V",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 0, {
            let mut u = op21s(0x13, 0, 41); // const/16 v0, 41
            u.extend(op21c(0x67, 0, ordem)); // sput v0, LApp;->ordem:I
            u.extend(op10x(0x0E));
            u
        })),
    );
    let p_i2 = b.proto_idx("I", vec![]);
    b.direct(
        app,
        "le_ordem",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(1, 0, 0, {
            let mut u = op21c(0x60, 0, ordem); // sget v0, ordem
            u.extend(op11x(0x0F, 0)); // return v0
            u
        })),
    );
    let _ = p_i2;
    // LMain: activity — onCreate escreve ordem+1 (provando que roda DEPOIS)
    let main = b.class("LMain;", "Landroid/app/Activity;");
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(2, 2, 1, {
            let mut u = op21c(0x60, 0, ordem); // sget v0, ordem
            u.extend(op11n(0x12, 1, 1)); // const/4 v1, +1
            u.extend(op23x(0x90, 0, 0, 1)); // add-int v0, v0, v1 → 42
            u.extend(op21c(0x67, 0, ordem)); // sput v0, ordem
            u.extend(op10x(0x0E));
            u
        })),
    );
    let dex = rd_dex::Dex::parse(b.finish()).expect("dex");
    let mut e = Engine::new(vec![dex], VmConfig::default());
    e.set_application_class("LApp;".to_string());
    let act = e
        .launch_app("LMain;", "com.ex.app", Vec::new())
        .expect("launch roda Application+Activity");
    // Ordem Android: LApp.onCreate (41) → LMain.onCreate (+1 = 42)
    let ai = e
        .invoke_static("LApp;", "le_ordem", "()I", &[])
        .expect("le_ordem")
        .as_int()
        .expect("int");
    assert_eq!(ai, 42, "Application.onCreate rodou ANTES do da activity");

    // getApplication via classe do app (rota que falhava) — canônica.
    let a1 = rd_vm::intrinsics::call_instance_intrinsic_mref(
        &mut e,
        "LMain;",
        "LMain;",
        "getApplication",
        "()Landroid/app/Application;",
        act,
        &[],
    )
    .expect("arm")
    .expect("getApplication devolve objeto");
    let a2 = rd_vm::intrinsics::call_instance_intrinsic_mref(
        &mut e,
        "LMain;",
        "LMain;",
        "getApplication",
        "()Landroid/app/Application;",
        act,
        &[],
    )
    .expect("arm")
    .expect("getApplication devolve objeto");
    assert_eq!(a1, a2, "canônica: mesma ObjRef nas duas chamadas");

    // 39º gap: getApplicationContext devolve a MESMA Application (Android).
    let c1 = rd_vm::intrinsics::call_instance_intrinsic_mref(
        &mut e,
        "LMain;",
        "LMain;",
        "getApplicationContext",
        "()Landroid/content/Context;",
        act,
        &[],
    )
    .expect("arm")
    .expect("getApplicationContext devolve objeto");
    assert_eq!(c1, a1, "getApplicationContext ≡ getApplication na activity");
    let rd_vm::Value::Obj(ar) = a1 else {
        panic!("esperava Obj")
    };
    assert_eq!(
        e.heap.class_of(ar).expect("class_of"),
        "LApp;",
        "instância da classe do manifest, não da plataforma"
    );
}

#[test]
fn unmodifiable_map_snapshot_le_e_bloqueia_mutacoes() {
    // M8 (40º gap, real do launch: PftpdApp.onCreate → LogController.<clinit>):
    // view de leitura; snapshot na criação; mutações → UOE (JDK); null → NPE.
    use rd_vm::intrinsics::{call_instance_intrinsic, call_static_intrinsic};
    const HM: &str = "Ljava/util/HashMap;";
    const UM: &str = "Ljava/util/Collections$UnmodifiableMap;";
    let b = DexBuilder::new();
    let mut e = engine_of(&b);

    // mapa-fonte host: {"k"→"v", "k2"→"v2"}
    let mk = |e: &mut Engine, s: &str| {
        e.heap
            .alloc_string(s.to_string())
            .map(rd_vm::value::Value::Obj)
            .expect("alloc_string")
    };
    let src = e.heap.alloc_instance(HM.to_string(), vec![]).unwrap();
    call_instance_intrinsic(&mut e, HM, "<init>", "()V", src, &[]).unwrap();
    let k = mk(&mut e, "k");
    let v = mk(&mut e, "v");
    let k2 = mk(&mut e, "k2");
    let v2 = mk(&mut e, "v2");
    call_instance_intrinsic(
        &mut e,
        HM,
        "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        src,
        &[k.clone(), v.clone()],
    )
    .unwrap();
    call_instance_intrinsic(
        &mut e,
        HM,
        "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        src,
        &[k2.clone(), v2.clone()],
    )
    .unwrap();

    // view: objeto de OUTRA classe-marcadora com as MESMAS entradas (snapshot)
    let view = call_static_intrinsic(
        &mut e,
        "Ljava/util/Collections;",
        "unmodifiableMap",
        "(Ljava/util/Map;)Ljava/util/Map;",
        &[rd_vm::value::Value::Obj(src)],
    )
    .expect("arm")
    .expect("unmodifiableMap devolve objeto");
    let rd_vm::value::Value::Obj(vr) = view else {
        panic!("esperava Obj")
    };
    assert_ne!(vr, src, "view é objeto novo, não identidade");
    assert_eq!(e.heap.class_of(vr).unwrap(), UM, "classe-marcadora host");
    let sz = call_instance_intrinsic(&mut e, UM, "size", "()I", vr, &[]).expect("size");
    assert_eq!(
        sz,
        Some(rd_vm::value::Value::Int(2)),
        "leituras delegam (size)"
    );
    let got = call_instance_intrinsic(
        &mut e,
        UM,
        "get",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        vr,
        std::slice::from_ref(&k2),
    )
    .unwrap()
    .expect("get devolve valor");
    assert_eq!(got, v2, "leitura k2 → v2");

    // mutações pela view → UnsupportedOperationException (JDK)
    for (n, s, a) in [
        (
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            vec![k.clone(), v.clone()],
        ),
        ("remove", "(Ljava/lang/Object;)Ljava/lang/Object;", vec![k]),
        ("clear", "()V", vec![]),
    ] {
        let r = call_instance_intrinsic(&mut e, UM, n, s, vr, &a);
        assert!(
            matches!(
                r,
                Err(rd_vm::VmExit::Exception(ref t)) if t.class == "Ljava/lang/UnsupportedOperationException;"
            ),
            "{n} pela view → UOE (got {r:?})"
        );
    }

    // null → NPE
    let npe = call_static_intrinsic(
        &mut e,
        "Ljava/util/Collections;",
        "unmodifiableMap",
        "(Ljava/util/Map;)Ljava/util/Map;",
        &[rd_vm::value::Value::Null],
    );
    assert!(
        matches!(npe, Err(rd_vm::VmExit::Exception(ref t)) if t.class == "Ljava/lang/NullPointerException;"),
        "null → NPE"
    );
}

#[test]
fn singleton_list_um_elemento_le_e_bloqueia_mutacoes() {
    // M8 (41º gap, real do launch: LogController.readPrefs): lista imutável
    // de 1 elemento; leituras delegam; mutações → UOE; null é aceito (JDK).
    use rd_vm::intrinsics::{call_instance_intrinsic, call_static_intrinsic};
    const SL: &str = "Ljava/util/Collections$SingletonList;";
    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let elem = e
        .heap
        .alloc_string("uno".to_string())
        .map(rd_vm::value::Value::Obj)
        .expect("alloc_string");
    let sl = call_static_intrinsic(
        &mut e,
        "Ljava/util/Collections;",
        "singletonList",
        "(Ljava/lang/Object;)Ljava/util/List;",
        std::slice::from_ref(&elem),
    )
    .expect("arm")
    .expect("singletonList devolve objeto");
    let rd_vm::value::Value::Obj(sr) = sl else {
        panic!("esperava Obj")
    };
    assert_eq!(e.heap.class_of(sr).unwrap(), SL, "classe-marcadora host");
    let sz = call_instance_intrinsic(&mut e, SL, "size", "()I", sr, &[]).expect("size");
    assert_eq!(sz, Some(rd_vm::value::Value::Int(1)), "size 1");
    let g0 = call_instance_intrinsic(
        &mut e,
        SL,
        "get",
        "(I)Ljava/lang/Object;",
        sr,
        &[rd_vm::value::Value::Int(0)],
    )
    .expect("get")
    .expect("elemento");
    assert_eq!(g0, elem, "get(0) devolve o elemento");
    let iterator =
        call_instance_intrinsic(&mut e, SL, "iterator", "()Ljava/util/Iterator;", sr, &[])
            .expect("iterator")
            .expect("iterator object");
    let rd_vm::value::Value::Obj(iterator) = iterator else {
        panic!("expected iterator object")
    };
    assert_eq!(
        call_instance_intrinsic(
            &mut e,
            "Ljava/util/Iterator;",
            "hasNext",
            "()Z",
            iterator,
            &[],
        )
        .expect("hasNext"),
        Some(rd_vm::value::Value::Int(1))
    );
    assert_eq!(
        call_instance_intrinsic(
            &mut e,
            "Ljava/util/Iterator;",
            "next",
            "()Ljava/lang/Object;",
            iterator,
            &[],
        )
        .expect("next"),
        Some(elem.clone())
    );
    let has = call_instance_intrinsic(
        &mut e,
        SL,
        "contains",
        "(Ljava/lang/Object;)Z",
        sr,
        std::slice::from_ref(&elem),
    )
    .expect("contains");
    assert_eq!(has, Some(rd_vm::value::Value::Int(1)), "contains");

    for (n, sig, a) in [
        ("add", "(Ljava/lang/Object;)Z", vec![elem.clone()]),
        (
            "set",
            "(ILjava/lang/Object;)Ljava/lang/Object;",
            vec![rd_vm::value::Value::Int(0), elem.clone()],
        ),
        (
            "remove",
            "(I)Ljava/lang/Object;",
            vec![rd_vm::value::Value::Int(0)],
        ),
        ("clear", "()V", vec![]),
    ] {
        let r = call_instance_intrinsic(&mut e, SL, n, sig, sr, &a);
        assert!(
            matches!(
                r,
                Err(rd_vm::VmExit::Exception(ref t)) if t.class == "Ljava/lang/UnsupportedOperationException;"
            ),
            "{n} pela view → UOE"
        );
    }

    // null é aceito (JDK): singletonList(null) = [null], get(0) = Null
    let nul = call_static_intrinsic(
        &mut e,
        "Ljava/util/Collections;",
        "singletonList",
        "(Ljava/lang/Object;)Ljava/util/List;",
        &[rd_vm::value::Value::Null],
    )
    .expect("arm")
    .expect("singletonList(null)");
    let rd_vm::value::Value::Obj(nr) = nul else {
        panic!("esperava Obj")
    };
    let g = call_instance_intrinsic(
        &mut e,
        SL,
        "get",
        "(I)Ljava/lang/Object;",
        nr,
        &[rd_vm::value::Value::Int(0)],
    )
    .expect("get")
    .expect("get devolve Null");
    assert_eq!(
        g,
        rd_vm::value::Value::Null,
        "singletonList(null) contém null"
    );
}

#[test]
fn hashset_init_collection_snapshot_dedup_encontrado() {
    // M8 (42º gap, real do launch: LogController.readPrefs → new
    // HashSet<>(String[]→List)): snapshot da fonte COM dedup em ordem;
    // null → NPE; fonte do app → VM_TYPE_ERROR tipado.
    use rd_vm::intrinsics::call_instance_intrinsic;
    const AL: &str = "Ljava/util/ArrayList;";
    const HS: &str = "Ljava/util/HashSet;";
    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let mk = |e: &mut Engine, s: &str| {
        e.heap
            .alloc_string(s.to_string())
            .map(rd_vm::value::Value::Obj)
            .expect("alloc_string")
    };

    // fonte: lista com dup ["a", "b", "a"] (posições 0,1,2)
    let al = e.heap.alloc_instance(AL.to_string(), vec![]).unwrap();
    call_instance_intrinsic(&mut e, AL, "<init>", "()V", al, &[]).unwrap();
    for s in ["a", "b", "a"] {
        let v = mk(&mut e, s);
        call_instance_intrinsic(&mut e, AL, "add", "(Ljava/lang/Object;)Z", al, &[v]).unwrap();
    }

    // ctor com coleção → snapshot dedup ["a", "b"]
    let hs = e.heap.alloc_instance(HS.to_string(), vec![]).unwrap();
    call_instance_intrinsic(
        &mut e,
        HS,
        "<init>",
        "(Ljava/util/Collection;)V",
        hs,
        &[rd_vm::value::Value::Obj(al)],
    )
    .expect("HashSet.<init>(Collection) coberto");
    let sz = call_instance_intrinsic(&mut e, HS, "size", "()I", hs, &[]).expect("size");
    assert_eq!(sz, Some(rd_vm::value::Value::Int(2)), "dedup da fonte");
    let vb = mk(&mut e, "b");
    let has = call_instance_intrinsic(&mut e, HS, "contains", "(Ljava/lang/Object;)Z", hs, &[vb])
        .expect("contains");
    assert_eq!(has, Some(rd_vm::value::Value::Int(1)), "contains b");

    // add de elemento da fonte agora → false (já presente)
    let va = mk(&mut e, "a");
    let dup = call_instance_intrinsic(&mut e, HS, "add", "(Ljava/lang/Object;)Z", hs, &[va])
        .expect("add");
    assert_eq!(dup, Some(rd_vm::value::Value::Int(0)), "add dup → false");

    // Collection null → NPE
    let hs2 = e.heap.alloc_instance(HS.to_string(), vec![]).unwrap();
    let npe = call_instance_intrinsic(
        &mut e,
        HS,
        "<init>",
        "(Ljava/util/Collection;)V",
        hs2,
        &[rd_vm::value::Value::Null],
    );
    assert!(
        matches!(npe, Err(rd_vm::VmExit::Exception(ref t)) if t.class == "Ljava/lang/NullPointerException;"),
        "fonte null → NPE"
    );
}

#[test]
fn unmodifiable_set_snapshot_set_le_e_bloqueia_mutacoes() {
    // M8 (43º gap, real do launch: LogController.readPrefs): view dedup de
    // leitura (snapshot); mutações → UOE; null → NPE.
    use rd_vm::intrinsics::{call_instance_intrinsic, call_static_intrinsic};
    const HS: &str = "Ljava/util/HashSet;";
    const US: &str = "Ljava/util/Collections$UnmodifiableSet;";
    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let mk = |e: &mut Engine, s: &str| {
        e.heap
            .alloc_string(s.to_string())
            .map(rd_vm::value::Value::Obj)
            .expect("alloc_string")
    };

    let src = e.heap.alloc_instance(HS.to_string(), vec![]).unwrap();
    call_instance_intrinsic(&mut e, HS, "<init>", "()V", src, &[]).unwrap();
    let va = mk(&mut e, "a");
    let vb2 = mk(&mut e, "b");
    call_instance_intrinsic(
        &mut e,
        HS,
        "add",
        "(Ljava/lang/Object;)Z",
        src,
        std::slice::from_ref(&va),
    )
    .unwrap();
    call_instance_intrinsic(&mut e, HS, "add", "(Ljava/lang/Object;)Z", src, &[vb2]).unwrap();

    let view = call_static_intrinsic(
        &mut e,
        "Ljava/util/Collections;",
        "unmodifiableSet",
        "(Ljava/util/Set;)Ljava/util/Set;",
        &[rd_vm::value::Value::Obj(src)],
    )
    .expect("arm")
    .expect("unmodifiableSet devolve objeto");
    let rd_vm::value::Value::Obj(vr) = view else {
        panic!("esperava Obj")
    };
    assert_ne!(vr, src, "view é objeto novo");
    assert_eq!(e.heap.class_of(vr).unwrap(), US, "classe-marcadora host");
    let sz = call_instance_intrinsic(&mut e, US, "size", "()I", vr, &[]).expect("size");
    assert_eq!(
        sz,
        Some(rd_vm::value::Value::Int(2)),
        "leituras delegam (size 2)"
    );
    let has = call_instance_intrinsic(
        &mut e,
        US,
        "contains",
        "(Ljava/lang/Object;)Z",
        vr,
        std::slice::from_ref(&va),
    )
    .expect("contains");
    assert_eq!(has, Some(rd_vm::value::Value::Int(1)), "contains a");

    for (n, sig, a) in [
        ("add", "(Ljava/lang/Object;)Z", vec![va.clone()]),
        ("remove", "(Ljava/lang/Object;)Z", vec![va]),
        ("clear", "()V", vec![]),
    ] {
        let r = call_instance_intrinsic(&mut e, US, n, sig, vr, &a);
        assert!(
            matches!(
                r,
                Err(rd_vm::VmExit::Exception(ref t)) if t.class == "Ljava/lang/UnsupportedOperationException;"
            ),
            "{n} pela view → UOE"
        );
    }

    let npe = call_static_intrinsic(
        &mut e,
        "Ljava/util/Collections;",
        "unmodifiableSet",
        "(Ljava/util/Set;)Ljava/util/Set;",
        &[rd_vm::value::Value::Null],
    );
    assert!(
        matches!(npe, Err(rd_vm::VmExit::Exception(ref t)) if t.class == "Ljava/lang/NullPointerException;"),
        "null → NPE"
    );
}

#[test]
fn preference_manager_prefs_padrao_comecam_vazias_devolvem_default() {
    // M8 (44º gap, real do launch: PftpdApp.onCreate → LogController.readPrefs
    // → PreferenceManager.getDefaultSharedPreferences): objeto canônico host;
    // prefs vazias → getXxx devolve o default da chamada; key null → NPE.
    use rd_vm::framework::{call_host_instance, call_host_static};
    const PM: &str = "Landroid/preference/PreferenceManager;";
    const SP: &str = "Landroid/content/SharedPreferences;";
    const SP_IMPL: &str = "Landroid/content/SharedPreferencesImpl;";
    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let ctx = rd_vm::value::Value::Obj(
        e.heap
            .alloc_instance("Landroid/app/Activity;".to_string(), Vec::new())
            .unwrap(),
    );
    let c1 = call_host_static(
        &mut e,
        PM,
        "getDefaultSharedPreferences",
        "(Landroid/content/Context;)Landroid/content/SharedPreferences;",
        std::slice::from_ref(&ctx),
    )
    .expect("arm")
    .expect("prefs objeto");
    let rd_vm::value::Value::Obj(pr) = c1 else {
        panic!("esperava Obj")
    };
    assert_eq!(e.heap.class_of(pr).unwrap(), SP_IMPL, "classe impl (AOSP)");
    let c2 = call_host_static(
        &mut e,
        PM,
        "getDefaultSharedPreferences",
        "(Landroid/content/Context;)Landroid/content/SharedPreferences;",
        std::slice::from_ref(&ctx),
    )
    .expect("arm");
    assert_eq!(
        c2,
        Some(rd_vm::value::Value::Obj(pr)),
        "canônico (objeto da VM, igual)"
    );

    let mk = |e: &mut Engine, s: &str| {
        e.heap
            .alloc_string(s.to_string())
            .map(rd_vm::value::Value::Obj)
            .expect("alloc_string")
    };
    let key = mk(&mut e, "xfer_mode");
    let def = mk(&mut e, "active");

    // getString → default
    let got = call_host_instance(
        &mut e,
        SP,
        "getString",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        pr,
        &[key.clone(), def.clone()],
    )
    .expect("getString")
    .expect("valor");
    assert_eq!(got, def.clone(), "prefs vazias → default");

    // getBoolean com default true → true (prefs vazias) — regra do próprio
    // default (o app espera enabled=true como no state inicial); getInt 41.
    let gb = call_host_instance(
        &mut e,
        SP,
        "getBoolean",
        "(Ljava/lang/String;Z)Z",
        pr,
        &[key.clone(), rd_vm::value::Value::Int(1)],
    )
    .expect("getBoolean");
    assert_eq!(gb, Some(rd_vm::value::Value::Int(1)), "default booleano");
    let gi = call_host_instance(
        &mut e,
        SP,
        "getInt",
        "(Ljava/lang/String;I)I",
        pr,
        &[key.clone(), rd_vm::value::Value::Int(41)],
    )
    .expect("getInt");
    assert_eq!(gi, Some(rd_vm::value::Value::Int(41)), "default inteiro");

    // contains → false (prefs vazias)
    let ct = call_host_instance(
        &mut e,
        SP,
        "contains",
        "(Ljava/lang/String;)Z",
        pr,
        std::slice::from_ref(&key),
    )
    .expect("contains");
    assert_eq!(ct, Some(rd_vm::value::Value::Int(0)), "contains false");

    // Deep audit (cycle 6): SharedPreferencesImpl has no null-key checks —
    // a null key misses in the HashMap and yields the caller's default.
    let null_key = call_host_instance(
        &mut e,
        SP,
        "getString",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        pr,
        &[rd_vm::value::Value::Null, def.clone()],
    );
    assert_eq!(
        null_key.expect("null-key get").expect("value"),
        def,
        "key null → default (sem NPE)"
    );
    // Context null, por outro lado, é dereferenciado → NPE.
    let null_ctx = call_host_static(
        &mut e,
        PM,
        "getDefaultSharedPreferences",
        "(Landroid/content/Context;)Landroid/content/SharedPreferences;",
        &[rd_vm::value::Value::Null],
    );
    assert!(
        matches!(null_ctx, Err(rd_vm::VmExit::Exception(ref t)) if t.class == "Ljava/lang/NullPointerException;"),
        "context null → NPE"
    );
}

#[test]
fn activity_component_name_uses_package_and_runtime_class() {
    use rd_vm::value::Value;
    let mut b = DexBuilder::new();
    let activity_class = b.class("Ltest/MainActivity;", "Landroid/app/Activity;");
    let _ = activity_class;
    let mut e = engine_of(&b);
    e.fw.package_name = "org.test".to_string();
    let activity = e
        .heap
        .alloc_instance("Ltest/MainActivity;".to_string(), vec![])
        .unwrap();
    let component = match e
        .invoke_instance(
            activity,
            "getComponentName",
            "()Landroid/content/ComponentName;",
            &[],
        )
        .unwrap()
    {
        Value::Obj(component) => component,
        other => panic!("Activity.getComponentName should return ComponentName: {other:?}"),
    };
    let package = e
        .invoke_instance(component, "getPackageName", "()Ljava/lang/String;", &[])
        .unwrap();
    let Value::Obj(package) = package else {
        panic!("ComponentName.getPackageName should return String")
    };
    assert_eq!(e.heap.as_str(package).unwrap(), "org.test");

    let class = e
        .invoke_instance(component, "getClassName", "()Ljava/lang/String;", &[])
        .unwrap();
    let Value::Obj(class) = class else {
        panic!("ComponentName.getClassName should return String")
    };
    assert_eq!(e.heap.as_str(class).unwrap(), "test.MainActivity");

    let title = e
        .invoke_instance(activity, "getTitle", "()Ljava/lang/CharSequence;", &[])
        .unwrap();
    let Value::Obj(title) = title else {
        panic!("Activity.getTitle should return its application title")
    };
    assert_eq!(e.heap.as_str(title).unwrap(), "org.test");
    e.invoke_instance(
        activity,
        "setTitle",
        "(Ljava/lang/CharSequence;)V",
        &[Value::StrPlaceholder("Custom title".to_string())],
    )
    .unwrap();
    let Value::Obj(title) = e
        .invoke_instance(activity, "getTitle", "()Ljava/lang/CharSequence;", &[])
        .unwrap()
    else {
        panic!("Activity.getTitle should return a set title")
    };
    assert_eq!(e.heap.as_str(title).unwrap(), "Custom title");
}

#[test]
fn window_callback_defaults_to_activity_and_can_be_replaced() {
    use rd_vm::value::Value;
    const ACTIVITY: &str = "Landroid/app/Activity;";

    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let activity = e.heap.alloc_instance(ACTIVITY.to_string(), vec![]).unwrap();
    let window = match e
        .invoke_instance(activity, "getWindow", "()Landroid/view/Window;", &[])
        .unwrap()
    {
        Value::Obj(window) => window,
        other => panic!("Activity.getWindow should return Window: {other:?}"),
    };
    assert_eq!(
        e.invoke_instance(
            window,
            "getCallback",
            "()Landroid/view/Window$Callback;",
            &[],
        )
        .unwrap(),
        Value::Obj(activity),
        "the Activity owns the default PhoneWindow callback"
    );

    let replacement = e
        .heap
        .alloc_instance("Ltest/WindowCallback;".to_string(), vec![])
        .unwrap();
    e.invoke_instance(
        window,
        "setCallback",
        "(Landroid/view/Window$Callback;)V",
        &[Value::Obj(replacement)],
    )
    .unwrap();
    assert_eq!(
        e.invoke_instance(
            window,
            "getCallback",
            "()Landroid/view/Window$Callback;",
            &[],
        )
        .unwrap(),
        Value::Obj(replacement)
    );

    let content = e
        .heap
        .alloc_instance("Landroid/widget/FrameLayout;".to_string(), vec![])
        .unwrap();
    e.attach_host_state("Landroid/widget/FrameLayout;", content)
        .unwrap();
    e.invoke_instance(
        window,
        "setContentView",
        "(Landroid/view/View;)V",
        &[Value::Obj(content)],
    )
    .unwrap();
    assert!(matches!(
        e.fw.get(window),
        Some(rd_vm::framework::HostObj::Window { content: Some(c) }) if *c == content
    ));
    let decor = match e
        .invoke_instance(window, "getDecorView", "()Landroid/view/View;", &[])
        .unwrap()
    {
        Value::Obj(decor) => decor,
        other => panic!("Window.getDecorView should return View: {other:?}"),
    };
    let frame = match e.fw.get(decor).unwrap() {
        rd_vm::framework::HostObj::View { children, .. } => children[0],
        _ => panic!("expected decor ViewGroup"),
    };
    assert!(matches!(
        e.fw.get(frame),
        Some(rd_vm::framework::HostObj::View { children, .. }) if children == &vec![content]
    ));
}

#[test]
fn fork_join_common_pool_is_canonical_and_executes_inline() {
    const FORK_JOIN_POOL: &str = "Ljava/util/concurrent/ForkJoinPool;";

    let mut b = DexBuilder::new();
    let runnable = b.class("LTestForkJoinTask;", "Ljava/lang/Object;");
    b.r#virtual(
        runnable,
        "run",
        "V",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 0, op10x(0x0E))),
    );
    let mut e = engine_of(&b);
    let pool = match e
        .invoke_static(
            FORK_JOIN_POOL,
            "commonPool",
            "()Ljava/util/concurrent/ForkJoinPool;",
            &[],
        )
        .unwrap()
    {
        Value::Obj(pool) => pool,
        other => panic!("commonPool should return ForkJoinPool: {other:?}"),
    };
    assert_eq!(e.heap.class_of(pool).unwrap(), FORK_JOIN_POOL);
    assert_eq!(
        e.invoke_static(
            FORK_JOIN_POOL,
            "commonPool",
            "()Ljava/util/concurrent/ForkJoinPool;",
            &[],
        )
        .unwrap(),
        Value::Obj(pool),
        "commonPool must return a stable singleton"
    );

    let task = e
        .heap
        .alloc_instance("LTestForkJoinTask;".into(), Vec::new())
        .unwrap();
    e.invoke_instance(
        pool,
        "execute",
        "(Ljava/lang/Runnable;)V",
        &[Value::Obj(task)],
    )
    .unwrap();
}

#[test]
fn context_open_file_streams_use_the_app_private_vfs() {
    use rd_vm::intrinsics::{alloc_string, call_instance_intrinsic};

    let mut e = engine_of(&DexBuilder::new());
    e.fw.package_name = "com.example.contextfiles".into();
    let context = e
        .heap
        .alloc_instance("Landroid/content/Context;".to_string(), Vec::new())
        .unwrap();
    let file_name = Value::Obj(alloc_string(&mut e, "state.bin".to_string()).unwrap());

    let missing = call_instance_intrinsic(
        &mut e,
        "Landroid/content/Context;",
        "openFileInput",
        "(Ljava/lang/String;)Ljava/io/FileInputStream;",
        context,
        std::slice::from_ref(&file_name),
    );
    assert!(matches!(
        missing,
        Err(VmExit::Exception(ref t)) if t.class == "Ljava/io/FileNotFoundException;"
    ));

    let output = call_instance_intrinsic(
        &mut e,
        "Landroid/content/Context;",
        "openFileOutput",
        "(Ljava/lang/String;I)Ljava/io/FileOutputStream;",
        context,
        &[file_name.clone(), Value::Int(0)],
    )
    .unwrap()
    .expect("openFileOutput should return a stream");
    let Value::Obj(output) = output else {
        panic!("openFileOutput must return FileOutputStream")
    };
    call_instance_intrinsic(
        &mut e,
        "Ljava/io/FileOutputStream;",
        "write",
        "(I)V",
        output,
        &[Value::Int(0x5A)],
    )
    .unwrap();
    call_instance_intrinsic(
        &mut e,
        "Ljava/io/FileOutputStream;",
        "close",
        "()V",
        output,
        &[],
    )
    .unwrap();

    let input = call_instance_intrinsic(
        &mut e,
        "Landroid/content/Context;",
        "openFileInput",
        "(Ljava/lang/String;)Ljava/io/FileInputStream;",
        context,
        std::slice::from_ref(&file_name),
    )
    .unwrap()
    .expect("the file should now exist");
    let Value::Obj(input) = input else {
        panic!("openFileInput must return FileInputStream")
    };
    assert_eq!(
        call_instance_intrinsic(
            &mut e,
            "Ljava/io/FileInputStream;",
            "read",
            "()I",
            input,
            &[],
        )
        .unwrap(),
        Some(Value::Int(0x5A))
    );
    assert_eq!(
        call_instance_intrinsic(
            &mut e,
            "Ljava/io/FileInputStream;",
            "read",
            "()I",
            input,
            &[],
        )
        .unwrap(),
        Some(Value::Int(-1))
    );

    let unsafe_name = Value::Obj(alloc_string(&mut e, "../outside.bin".to_string()).unwrap());
    assert!(matches!(
        call_instance_intrinsic(
            &mut e,
            "Landroid/content/Context;",
            "openFileOutput",
            "(Ljava/lang/String;I)Ljava/io/FileOutputStream;",
            context,
            &[unsafe_name, Value::Int(0)],
        ),
        Err(VmExit::Exception(ref t)) if t.class == "Ljava/lang/IllegalArgumentException;"
    ));
}

#[test]
fn print_stream_println_writes_to_its_output_stream() {
    let mut e = engine_of(&DexBuilder::new());
    let output = e
        .heap
        .alloc_instance("Ljava/io/ByteArrayOutputStream;".into(), vec![])
        .unwrap();
    e.invoke_instance(output, "<init>", "()V", &[]).unwrap();

    let stream = e
        .heap
        .alloc_instance("Ljava/io/PrintStream;".into(), vec![])
        .unwrap();
    e.invoke_instance(
        stream,
        "<init>",
        "(Ljava/io/OutputStream;)V",
        &[Value::Obj(output)],
    )
    .unwrap();
    let text = Value::Obj(
        e.heap
            .alloc_string("sample authorized keys".into())
            .unwrap(),
    );
    e.invoke_instance(stream, "println", "(Ljava/lang/String;)V", &[text])
        .unwrap();

    let output_text = e
        .invoke_instance(output, "toString", "()Ljava/lang/String;", &[])
        .unwrap();
    let Value::Obj(output_text) = output_text else {
        panic!("ByteArrayOutputStream.toString must return String")
    };
    assert_eq!(
        e.heap.as_str(output_text).unwrap(),
        "sample authorized keys\n"
    );
}

#[test]
fn layout_inflater_from_returns_context_service_and_tracks_factory() {
    use rd_vm::intrinsics::{call_instance_intrinsic, call_static_intrinsic};
    use rd_vm::value::Value;
    const LI: &str = "Landroid/view/LayoutInflater;";
    const FACTORY2: &str = "(Landroid/view/LayoutInflater$Factory2;)V";

    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let context = e
        .heap
        .alloc_instance("Landroid/content/Context;".to_string(), vec![])
        .unwrap();
    let from = |e: &mut Engine, context| {
        call_static_intrinsic(
            e,
            LI,
            "from",
            "(Landroid/content/Context;)Landroid/view/LayoutInflater;",
            &[Value::Obj(context)],
        )
    };
    let inflater = match from(&mut e, context).unwrap() {
        Some(Value::Obj(inflater)) => inflater,
        other => panic!("LayoutInflater.from(context) should return the service: {other:?}"),
    };
    assert_eq!(
        from(&mut e, context).unwrap(),
        Some(Value::Obj(inflater)),
        "the Context service is stable across from() calls"
    );
    assert_eq!(
        call_instance_intrinsic(
            &mut e,
            LI,
            "getFactory",
            "()Landroid/view/LayoutInflater$Factory;",
            inflater,
            &[],
        )
        .unwrap(),
        Some(Value::Null)
    );

    let factory = e
        .heap
        .alloc_instance("Ltest/InflaterFactory;".to_string(), vec![])
        .unwrap();
    call_instance_intrinsic(
        &mut e,
        LI,
        "setFactory2",
        FACTORY2,
        inflater,
        &[Value::Obj(factory)],
    )
    .unwrap();
    assert_eq!(
        call_instance_intrinsic(
            &mut e,
            LI,
            "getFactory",
            "()Landroid/view/LayoutInflater$Factory;",
            inflater,
            &[],
        )
        .unwrap(),
        Some(Value::Obj(factory))
    );
    assert_eq!(
        call_instance_intrinsic(
            &mut e,
            LI,
            "getFactory2",
            "()Landroid/view/LayoutInflater$Factory2;",
            inflater,
            &[],
        )
        .unwrap(),
        Some(Value::Obj(factory))
    );

    let null_context = call_static_intrinsic(
        &mut e,
        LI,
        "from",
        "(Landroid/content/Context;)Landroid/view/LayoutInflater;",
        &[Value::Null],
    );
    assert!(matches!(
        null_context,
        Err(VmExit::Exception(ref t)) if t.class == "Ljava/lang/NullPointerException;"
    ));
}

#[test]
fn service_loader_descobre_provider_de_meta_inf_services() {
    // M8 (45º gap, real do launch do primftpd — slf4j 2.x descobre logback via
    // ServiceLoader): load guarda os providers de META-INF/services/<bin>;
    // iterator() INSTANCIA cada classe impl (ctor roda); provider ausente dos
    // dex → ServiceConfigurationError tipado (JLS), nunca skip silencioso.
    use rd_vm::intrinsics::{call_instance_intrinsic, call_static_intrinsic};
    use rd_vm::value::Value;
    const SL: &str = "Ljava/util/ServiceLoader;";
    const IT: &str = "Ljava/util/Iterator;";
    let _ = IT;
    let mut b = DexBuilder::new();
    let _marker = b.class("Lsvc/Marker;", "Ljava/lang/Object;");
    let prov = b.class("Lsvc/Prov;", "Ljava/lang/Object;");
    b.static_field(prov, "count", "I", SVal::Int(0));
    let count = b.field_idx("Lsvc/Prov;", "I", "count");
    b.direct(
        prov,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 0, {
            let mut u = op21s(0x13, 0, 42); // const/16 v0, 42
            u.extend(op21c(0x67, 0, count)); // sput v0, Lsvc/Prov;->count:I
            u.extend(op10x(0x0E)); // return-void
            u
        })),
    );
    b.direct(
        prov,
        "le_count",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(1, 0, 0, {
            let mut u = op21c(0x60, 0, count); // sget v0, count
            u.extend(op11x(0x0F, 0)); // return v0
            u
        })),
    );
    let mut e = engine_of(&b);

    // APK com provider real + um ausente (comentário # e linha vazia pulados)
    let arsc = common::apkfix::ArscFix::new().build();
    let apk_bytes = common::apkfix::build_apk(
        "svc.t",
        "Lsvc/Marker;",
        arsc,
        vec![(
            "META-INF/services/svc.Marker",
            b"# prov com comentario\nsvc.Prov\n\nsvc.Missing\n".to_vec(),
        )],
    );
    e.set_resources(rd_apk::Apk::from_bytes(apk_bytes).expect("apk"));

    // Class da interface de serviço via getClass (o canônico do host)
    let m = e
        .heap
        .alloc_instance("Lsvc/Marker;".to_string(), vec![])
        .unwrap();
    let service_class = match call_instance_intrinsic(
        &mut e,
        "Lsvc/Marker;",
        "getClass",
        "()Ljava/lang/Class;",
        m,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("service Class: {other:?}"),
    };
    let loaded = call_static_intrinsic(
        &mut e,
        SL,
        "load",
        "(Ljava/lang/Class;)Ljava/util/ServiceLoader;",
        &[Value::Obj(service_class)],
    )
    .expect("arm")
    .expect("loader");
    let Value::Obj(loader) = loaded else {
        panic!("esperava Obj")
    };

    // iterator(): instancia svc.Prov (ctor roda count=42), depois svc.Missing
    // ausente → ServiceConfigurationError.
    let r = call_instance_intrinsic(
        &mut e,
        SL,
        "iterator",
        "()Ljava/util/Iterator;",
        loader,
        &[],
    );
    match r {
        Err(VmExit::Exception(ref t)) => {
            assert_eq!(
                t.class, "Ljava/util/ServiceConfigurationError;",
                "provider ausente dos dex → ServiceConfigurationError (JLS)"
            );
            let msg = t.message.clone().unwrap_or_default();
            assert!(
                msg.contains("Lsvc/Missing;"),
                "mensagem cita o provider ausente: {msg}"
            );
        }
        other => panic!("esperava ServiceConfigurationError, got {other:?}"),
    }

    // O primeiro provider chegou a instanciar ANTES do erro do ausente:
    // count=42 prova que o ctor DEX rodou (instantiate eager ordenado).
    let got = e
        .invoke_static("Lsvc/Prov;", "le_count", "()I", &[])
        .expect("le_count")
        .as_int()
        .expect("int");
    assert_eq!(
        got, 42,
        "ctor do provider rodou antes do provider ausente falhar"
    );
}
