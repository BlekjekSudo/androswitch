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
            let mut u = op11n(0x12, 0, 7);
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
            let mut u = op11n(0x12, 0, 7); // const/4 v0, 7
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
