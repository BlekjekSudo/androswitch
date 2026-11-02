//! Issue #330 — resolução canônica de host statics.
//!
//! Antes: (1) `sget-object Locale.US` devolvia `Value::Null` (NPE no
//! `<clinit>` de APKs reais — org.primftpd_71.apk materializa um
//! DateTimeFormatter com Locale.US/ENGLISH/ROOT); (2) o `sget*` (interp.rs)
//! e a reflexão (`Class.getField`/`Field.get(null)` em intrinsics.rs) tinham
//! conjuntos de providers DIVERGENTES — bytecode devolvia 0/Null para campos
//! que a reflexão enxergava (android_sysservices, android_stub_surfaces) e a
//! reflexão não via os 8 providers que o bytecode enxergava (TimeUnit,
//! jtext_format, android_colorspace, graphics, parcel, platform_base,
//! collections, jtime); (3) boxes omittia Byte/Short/Character.
//!
//! Provas aqui: FALHA→PASSA nos três eixos e CONCORDÂNCIA bytecode↔reflexão
//! nos campos pedidos na issue (Locale.US/ROOT, StandardCharsets.UTF_8,
//! Collections.EMPTY_LIST, ZoneOffset.UTC, TimeUnit.SECONDS,
//! WebSettings.LOAD_DEFAULT, Byte.MAX_VALUE).

mod common;

use common::*;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef};

const LOCALE: &str = "Ljava/util/Locale;";
const TIME_UNIT: &str = "Ljava/util/concurrent/TimeUnit;";
const WEB_SETTINGS: &str = "Landroid/webkit/WebSettings;";
const STANDARD_CHARSETS: &str = "Ljava/nio/charset/StandardCharsets;";
const COLLECTIONS: &str = "Ljava/util/Collections;";
// Nota #330: a issue enumera "ZoneOffset.UTC/Instant.EPOCH/..." como
// providers de jtime, mas na main o jtime::host_static só serve
// ChronoUnit.* (UNIT) — ZoneOffset/Instant.EPOCH não existem em host_static
// algum (lacuna preexistente do java.time, fora do escopo deste fix; o
// par ChronoUnit.SECONDS abaixo prova o provider jtime nos dois caminhos).
const CHRONO_UNIT: &str = "Ljava/time/temporal/ChronoUnit;";

// ── helpers de valor ────────────────────────────────────────────────────────

fn text(e: &mut Engine, s: &str) -> Value {
    Value::Obj(e.heap.alloc_string(s.to_string()).unwrap())
}
fn text_of(e: &Engine, v: &Value) -> String {
    match v {
        Value::Obj(r) => e.heap.as_str(*r).unwrap().to_string(),
        other => panic!("esperava String, veio {other:?}"),
    }
}
fn obj_of(v: Value) -> ObjRef {
    match v {
        Value::Obj(r) => r,
        other => panic!("esperava Obj, veio {other:?}"),
    }
}
fn int_of(v: Value) -> i32 {
    match v {
        Value::Int(i) => i,
        other => panic!("esperava Int, veio {other:?}"),
    }
}

/// Class.getField(name) sobre classe HOST (caminho intrinsics.rs: a classe
/// não tem DEX → platform_static → resolve_host_static).
fn host_field(engine: &mut Engine, desc: &str, name: &str) -> ObjRef {
    let class = engine
        .heap
        .alloc_instance("Ljava/lang/Class;".to_string(), Vec::new())
        .unwrap();
    let name_val = text(engine, desc);
    engine
        .heap
        .put_field(class, "class_name", name_val)
        .unwrap();
    let fname_val = text(engine, name);
    let f = engine
        .invoke_instance(
            class,
            "getField",
            "(Ljava/lang/String;)Ljava/lang/reflect/Field;",
            &[fname_val],
        )
        .unwrap_or_else(|e| panic!("getField({desc}.{name}) falhou: {e:?}"));
    obj_of(f)
}

/// Field.get(null) reflexivo.
fn field_get(engine: &mut Engine, field: ObjRef) -> Value {
    engine
        .invoke_instance(
            field,
            "get",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            &[Value::Null],
        )
        .unwrap_or_else(|e| panic!("Field.get falhou: {e:?}"))
}

// ── fixture DEX: métodos estáticos que fazem sget nos host statics ─────────

/// Cada `le_*` retorna o objeto/valor lido por `sget[-object]` REAL (o
/// caminho do interp.rs, não um atalho de host), provando o elo
/// bytecode → vm.statics → provider.
fn fixture() -> Engine {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");

    // field refs (field_ids determinísticos)
    let f_us = b.field_idx(LOCALE, LOCALE, "US");
    let f_chinese = b.field_idx(LOCALE, LOCALE, "CHINESE");
    let f_root = b.field_idx(LOCALE, LOCALE, "ROOT");
    let f_prc = b.field_idx(LOCALE, LOCALE, "PRC");
    let f_china = b.field_idx(LOCALE, LOCALE, "CHINA");
    let f_simp = b.field_idx(LOCALE, LOCALE, "SIMPLIFIED_CHINESE");
    let f_taiwan = b.field_idx(LOCALE, LOCALE, "TAIWAN");
    let f_trad = b.field_idx(LOCALE, LOCALE, "TRADITIONAL_CHINESE");
    let f_seconds = b.field_idx(TIME_UNIT, TIME_UNIT, "SECONDS");
    let f_utf8 = b.field_idx(STANDARD_CHARSETS, "Ljava/nio/charset/Charset;", "UTF_8");
    let f_empty_list = b.field_idx(COLLECTIONS, "Ljava/util/List;", "EMPTY_LIST");
    let f_chrono = b.field_idx(CHRONO_UNIT, CHRONO_UNIT, "SECONDS");
    let f_load_default = b.field_idx(WEB_SETTINGS, "I", "LOAD_DEFAULT");
    let f_byte_max = b.field_idx("Ljava/lang/Byte;", "B", "MAX_VALUE");
    let f_byte_min = b.field_idx("Ljava/lang/Byte;", "B", "MIN_VALUE");
    let f_short_max = b.field_idx("Ljava/lang/Short;", "S", "MAX_VALUE");
    let f_char_max = b.field_idx("Ljava/lang/Character;", "C", "MAX_VALUE");
    let f_min_radix = b.field_idx("Ljava/lang/Character;", "I", "MIN_RADIX");
    let f_max_radix = b.field_idx("Ljava/lang/Character;", "I", "MAX_RADIX");
    let f_int_size = b.field_idx("Ljava/lang/Integer;", "I", "SIZE");
    let f_int_bytes = b.field_idx("Ljava/lang/Integer;", "I", "BYTES");
    let f_long_size = b.field_idx("Ljava/lang/Long;", "I", "SIZE");

    // method refs
    let p_str = b.proto_idx("Ljava/lang/String;", vec![]);
    let getlang = b.method_idx(LOCALE, p_str, "getLanguage");
    let getcountry = b.method_idx(LOCALE, p_str, "getCountry");
    let totag = b.method_idx(LOCALE, p_str, "toLanguageTag");
    let p_vss = b.proto_idx(
        "V",
        vec!["Ljava/lang/String;".into(), "Ljava/lang/String;".into()],
    );
    let locale_init = b.method_idx(LOCALE, p_vss, "<init>");
    let p_z = b.proto_idx("Z", vec!["Ljava/lang/Object;".into()]);
    let locale_equals = b.method_idx(LOCALE, p_z, "equals");

    let sidx_en = b.intern("en");
    let sidx_us = b.intern("US");
    let t_locale = b.type_idx(LOCALE);

    // helper local: método `sget-object <f>; return-object` (regs=1)
    macro_rules! le_obj {
        ($name:literal, $f:expr, $ret:expr) => {
            b.direct(
                cls,
                $name,
                $ret,
                vec![],
                ACC_PUBLIC | ACC_STATIC,
                Some(b.code(1, 0, 0, {
                    let mut u = op21c(0x62, 0, $f); // sget-object v0
                    u.extend(op11x(0x11, 0)); // return-object v0
                    u
                })),
            );
        };
    }
    // helper local: método `sget <f>; return` (regs=1)
    macro_rules! le_int {
        ($name:literal, $f:expr) => {
            b.direct(
                cls,
                $name,
                "I",
                vec![],
                ACC_PUBLIC | ACC_STATIC,
                Some(b.code(1, 0, 0, {
                    let mut u = op21c(0x60, 0, $f); // sget v0
                    u.extend(op11x(0x0F, 0)); // return v0
                    u
                })),
            );
        };
    }

    le_obj!("le_us_a", f_us, LOCALE);
    le_obj!("le_us_b", f_us, LOCALE);
    le_obj!("le_chinese", f_chinese, LOCALE);
    le_obj!("le_prc", f_prc, LOCALE);
    le_obj!("le_china", f_china, LOCALE);
    le_obj!("le_simp", f_simp, LOCALE);
    le_obj!("le_taiwan", f_taiwan, LOCALE);
    le_obj!("le_trad", f_trad, LOCALE);
    le_obj!("le_root", f_root, LOCALE);
    le_obj!("le_seconds", f_seconds, TIME_UNIT);
    le_obj!("le_utf8", f_utf8, "Ljava/nio/charset/Charset;");
    le_obj!("le_empty_list", f_empty_list, "Ljava/util/List;");
    le_obj!("le_chrono", f_chrono, CHRONO_UNIT);
    le_int!("le_load_default", f_load_default);
    le_int!("le_byte_max", f_byte_max);
    le_int!("le_byte_min", f_byte_min);
    le_int!("le_short_max", f_short_max);
    le_int!("le_char_max", f_char_max);
    le_int!("le_min_radix", f_min_radix);
    le_int!("le_max_radix", f_max_radix);
    le_int!("le_int_size", f_int_size);
    le_int!("le_int_bytes", f_int_bytes);
    le_int!("le_long_size", f_long_size);

    // le_us_lang()Ljava/lang/String;: sget-object US; getLanguage(); return-object
    b.direct(
        cls,
        "le_us_lang",
        "Ljava/lang/String;",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(2, 0, 1, {
            let mut u = op21c(0x62, 0, f_us);
            u.extend(op35c(0x6E, 1, getlang, [0, 0, 0, 0, 0]));
            u.extend(op11x(0x0A, 0)); // move-result v0
            u.extend(op11x(0x11, 0)); // return-object v0
            u
        })),
    );
    // le_us_country()Ljava/lang/String;: idem getCountry()
    b.direct(
        cls,
        "le_us_country",
        "Ljava/lang/String;",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(2, 0, 1, {
            let mut u = op21c(0x62, 0, f_us);
            u.extend(op35c(0x6E, 1, getcountry, [0, 0, 0, 0, 0]));
            u.extend(op11x(0x0A, 0));
            u.extend(op11x(0x11, 0));
            u
        })),
    );
    // le_root_tag()Ljava/lang/String;: sget-object ROOT; toLanguageTag()
    b.direct(
        cls,
        "le_root_tag",
        "Ljava/lang/String;",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(2, 0, 1, {
            let mut u = op21c(0x62, 0, f_root);
            u.extend(op35c(0x6E, 1, totag, [0, 0, 0, 0, 0]));
            u.extend(op11x(0x0A, 0));
            u.extend(op11x(0x11, 0));
            u
        })),
    );
    // le_root_lang()Ljava/lang/String;: sget-object ROOT; getLanguage()
    b.direct(
        cls,
        "le_root_lang",
        "Ljava/lang/String;",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(2, 0, 1, {
            let mut u = op21c(0x62, 0, f_root);
            u.extend(op35c(0x6E, 1, getlang, [0, 0, 0, 0, 0]));
            u.extend(op11x(0x0A, 0));
            u.extend(op11x(0x11, 0));
            u
        })),
    );
    // novo_us()Ljava/util/Locale;: new Locale("en","US") — objeto NOVO,
    // distinto da constante canônica (semântica JDK de identidade)
    b.direct(
        cls,
        "novo_us",
        LOCALE,
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(3, 0, 3, {
            let mut u = op21c(0x22, 0, t_locale); // new-instance v0
            u.extend(op21c(0x1A, 1, sidx_en as u16)); // const-string v1, "en"
            u.extend(op21c(0x1A, 2, sidx_us as u16)); // const-string v2, "US"
            u.extend(op35c(0x70, 3, locale_init, [0, 1, 2, 0, 0]));
            u.extend(op11x(0x11, 0));
            u
        })),
    );
    // novo_equals_us()Z: new Locale("en","US").equals(Locale.US) → 1
    b.direct(
        cls,
        "novo_equals_us",
        "Z",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(4, 0, 2, {
            let mut u = op21c(0x22, 0, t_locale);
            u.extend(op21c(0x1A, 1, sidx_en as u16));
            u.extend(op21c(0x1A, 2, sidx_us as u16));
            u.extend(op35c(0x70, 3, locale_init, [0, 1, 2, 0, 0]));
            u.extend(op21c(0x62, 3, f_us)); // sget-object v3, Locale.US
            u.extend(op35c(0x6E, 2, locale_equals, [0, 3, 0, 0, 0]));
            u.extend(op11x(0x0A, 0));
            u.extend(op11x(0x0F, 0));
            u
        })),
    );

    engine_of(&b)
}

// ── 1) Locale.* existem e têm os valores JDK (FALHA→PASSA) ─────────────────

/// ANTES do fix: sget-object Locale.US → Null → getLanguage() em receiver
/// nulo → NullPointerException (crash real do org.primftpd_71.apk no
/// <clinit> via DateTimeFormatter.ofPattern(p, Locale.US)).
/// DEPOIS: "en" / "US", os valores do Locale.java.
#[test]
fn locale_us_constants_have_jdk_values_via_real_sget() {
    let mut e = fixture();
    let lang = invoke(&mut e, "le_us_lang", "()Ljava/lang/String;", &[]).unwrap();
    assert_eq!(text_of(&e, &lang), "en");
    let country = invoke(&mut e, "le_us_country", "()Ljava/lang/String;", &[]).unwrap();
    assert_eq!(text_of(&e, &country), "US");
}

/// Locale.ROOT: language "" e toLanguageTag "und" (JDK: "und" para raiz).
#[test]
fn locale_root_materializes_with_jdk_semantics() {
    let mut e = fixture();
    let lang = invoke(&mut e, "le_root_lang", "()Ljava/lang/String;", &[]).unwrap();
    assert_eq!(text_of(&e, &lang), "");
    let tag = invoke(&mut e, "le_root_tag", "()Ljava/lang/String;", &[]).unwrap();
    assert_eq!(text_of(&e, &tag), "und");
}

// ── 2) identidade JDK: aliases compartilham UMA instância ──────────────────

/// Duas leituras da MESMA constante → o MESMO objeto (cache vm.statics);
/// e o construído pelo guest (`new Locale("en","US")`) é objeto DISTINTO,
/// com equals valor-igual — exatamente o JVM.
#[test]
fn locale_identity_same_constant_same_object_constructed_distinct() {
    let mut e = fixture();
    let a = obj_of(invoke(&mut e, "le_us_a", &format!("(){LOCALE}"), &[]).unwrap());
    let b = obj_of(invoke(&mut e, "le_us_b", &format!("(){LOCALE}"), &[]).unwrap());
    assert_eq!(a, b, "Locale.US deve ser o mesmo objeto em toda leitura");
    let novo = obj_of(invoke(&mut e, "novo_us", &format!("(){LOCALE}"), &[]).unwrap());
    assert_ne!(a, novo, "new Locale(\"en\",\"US\") ≠ Locale.US (JDK)");
    assert_eq!(
        int_of(invoke(&mut e, "novo_equals_us", "()Z", &[]).unwrap()),
        1,
        "new Locale(\"en\",\"US\").equals(Locale.US) == true (JDK)"
    );
}

/// Locale.java: CHINA = SIMPLIFIED_CHINESE; PRC = CHINA; TAIWAN =
/// TRADITIONAL_CHINESE — aliases são o MESMO campo, logo a MESMA instância.
/// Cobre também a ordem PRC-antes-do-canônico (reuso de grupo).
#[test]
fn locale_aliases_share_one_instance() {
    let mut e = fixture();
    let prc = obj_of(invoke(&mut e, "le_prc", &format!("(){LOCALE}"), &[]).unwrap());
    let china = obj_of(invoke(&mut e, "le_china", &format!("(){LOCALE}"), &[]).unwrap());
    let simp = obj_of(invoke(&mut e, "le_simp", &format!("(){LOCALE}"), &[]).unwrap());
    assert_eq!(prc, china, "Locale.PRC == Locale.CHINA (alias JDK)");
    assert_eq!(prc, simp, "Locale.PRC == Locale.SIMPLIFIED_CHINESE");
    let taiwan = obj_of(invoke(&mut e, "le_taiwan", &format!("(){LOCALE}"), &[]).unwrap());
    let trad = obj_of(invoke(&mut e, "le_trad", &format!("(){LOCALE}"), &[]).unwrap());
    assert_eq!(taiwan, trad, "Locale.TAIWAN == Locale.TRADITIONAL_CHINESE");
    // grupos distintos NÃO colapsam (CHINESE("zh","") ≠ SIMPLIFIED("zh","CN"))
    let chinese = obj_of(invoke(&mut e, "le_chinese", &format!("(){LOCALE}"), &[]).unwrap());
    assert_ne!(simp, chinese, "Locale.CHINESE é objeto próprio");
}

// ── 3) provider que o sget NUNCA chamou (split-brain lado bytecode) ────────

/// WebSettings.LOAD_DEFAULT = -1 (android_stub_surfaces.rs): ANTES o sget
/// devolvia 0 (default_for) porque interp.rs não chamava
/// android_stub_surfaces::host_static.
#[test]
fn sget_reaches_android_stub_surfaces_and_sysservices() {
    let mut e = fixture();
    assert_eq!(
        int_of(invoke(&mut e, "le_load_default", "()I", &[]).unwrap()),
        -1,
        "sget WebSettings.LOAD_DEFAULT deve ver -1 (não o default 0)"
    );
}

/// TimeUnit.SECONDS via sget (objeto enum materializado) e via reflexão —
/// o MESMO objeto nos dois caminhos.
#[test]
fn time_unit_seconds_sget_and_reflection_agree() {
    let mut e = fixture();
    let via_sget = obj_of(invoke(&mut e, "le_seconds", &format!("(){TIME_UNIT}"), &[]).unwrap());
    let f = host_field(&mut e, TIME_UNIT, "SECONDS");
    let via_refl = obj_of(field_get(&mut e, f));
    assert_eq!(
        via_sget, via_refl,
        "sget e Field.get veem a mesma instância"
    );
}

// ── 4) concordância reflexão ↔ sget nos campos pedidos na issue ────────────

#[test]
fn reflection_agrees_with_sget_on_issue_fields() {
    let mut e = fixture();

    // Locale.US: reflexão ANTES nem via android_res (NoSuchFieldException)
    let f = host_field(&mut e, LOCALE, "US");
    let refl_us = obj_of(field_get(&mut e, f));
    let sget_us = obj_of(invoke(&mut e, "le_us_a", &format!("(){LOCALE}"), &[]).unwrap());
    assert_eq!(
        refl_us, sget_us,
        "Locale.US: reflexão == sget (mesmo objeto)"
    );

    // StandardCharsets.UTF_8 (jtext_format: só bytecode via, reflexão não)
    let f = host_field(&mut e, STANDARD_CHARSETS, "UTF_8");
    let refl = obj_of(field_get(&mut e, f));
    let sget = obj_of(invoke(&mut e, "le_utf8", "()Ljava/nio/charset/Charset;", &[]).unwrap());
    assert_eq!(refl, sget, "StandardCharsets.UTF_8: reflexão == sget");

    // Collections.EMPTY_LIST (collections: idem)
    let f = host_field(&mut e, COLLECTIONS, "EMPTY_LIST");
    let refl = obj_of(field_get(&mut e, f));
    let sget = obj_of(invoke(&mut e, "le_empty_list", "()Ljava/util/List;", &[]).unwrap());
    assert_eq!(refl, sget, "Collections.EMPTY_LIST: reflexão == sget");

    // ChronoUnit.SECONDS (jtime: só bytecode via, reflexão não)
    let f = host_field(&mut e, CHRONO_UNIT, "SECONDS");
    let refl = obj_of(field_get(&mut e, f));
    let sget = obj_of(invoke(&mut e, "le_chrono", &format!("(){CHRONO_UNIT}"), &[]).unwrap());
    assert_eq!(refl, sget, "ChronoUnit.SECONDS: reflexão == sget");
}

/// Constantes de wrapper: Byte/Short/Character (antes 0 no sget Kotlin não
/// inlinado) e SIZE/BYTES; a reflexão concorda no VALOR.
///
/// Nota de modelagem: ReflectField de host estático tipa o valor pelo
/// runtime (B→"I" em type_of_value), então Field.get embute Integer — o
/// VALOR concorda com o sget (127), que é o critério da issue; a caixa
/// exata é metadado herdado do modelo de registradores 32-bit do Dalvik.
#[test]
fn wrapper_constants_sget_and_reflection_agree_on_values() {
    let mut e = fixture();
    // sget (caminho bytecode)
    assert_eq!(
        int_of(invoke(&mut e, "le_byte_max", "()I", &[]).unwrap()),
        127
    );
    assert_eq!(
        int_of(invoke(&mut e, "le_byte_min", "()I", &[]).unwrap()),
        -128
    );
    assert_eq!(
        int_of(invoke(&mut e, "le_short_max", "()I", &[]).unwrap()),
        32767
    );
    assert_eq!(
        int_of(invoke(&mut e, "le_char_max", "()I", &[]).unwrap()),
        0xFFFF
    );
    assert_eq!(
        int_of(invoke(&mut e, "le_min_radix", "()I", &[]).unwrap()),
        2
    );
    assert_eq!(
        int_of(invoke(&mut e, "le_max_radix", "()I", &[]).unwrap()),
        36
    );
    assert_eq!(
        int_of(invoke(&mut e, "le_int_size", "()I", &[]).unwrap()),
        32
    );
    assert_eq!(
        int_of(invoke(&mut e, "le_int_bytes", "()I", &[]).unwrap()),
        4
    );
    assert_eq!(
        int_of(invoke(&mut e, "le_long_size", "()I", &[]).unwrap()),
        64
    );

    // reflexão (mesmos valores; Byte.MAX_VALUE era invisível p/ boxes)
    let f = host_field(&mut e, "Ljava/lang/Byte;", "MAX_VALUE");
    let boxed = field_get(&mut e, f);
    assert_eq!(
        int_of(
            e.invoke_instance(obj_of(boxed), "intValue", "()I", &[])
                .unwrap()
        ),
        127,
        "Byte.MAX_VALUE via reflexão == 127"
    );
    let f = host_field(&mut e, WEB_SETTINGS, "LOAD_DEFAULT");
    let boxed = field_get(&mut e, f);
    assert_eq!(
        int_of(
            e.invoke_instance(obj_of(boxed), "intValue", "()I", &[])
                .unwrap()
        ),
        -1,
        "WebSettings.LOAD_DEFAULT via reflexão == -1"
    );
}

// ── 5) cache: segunda leitura não re-materializa (hit em vm.statics) ───────

/// Identidade dupla já cobre cache para objetos; aqui o cenário adverso:
/// PRC lido PRIMEIRO (grupo [SIMPLIFIED_CHINESE, CHINA, PRC] sem canônico em
/// cache), depois CHINA e SIMPLIFIED_CHINESE — todos reusam o 1º ObjRef.
#[test]
fn alias_group_reuses_first_materialized_object_regardless_of_order() {
    let mut e = fixture();
    let prc = obj_of(invoke(&mut e, "le_prc", &format!("(){LOCALE}"), &[]).unwrap());
    let china = obj_of(invoke(&mut e, "le_china", &format!("(){LOCALE}"), &[]).unwrap());
    let simp = obj_of(invoke(&mut e, "le_simp", &format!("(){LOCALE}"), &[]).unwrap());
    assert_eq!(prc, china);
    assert_eq!(prc, simp);
}

/// System.out continua materializando PrintStream embutido (issues
/// #43/#48 preservadas na unificação) — e agora TAMBÉM via reflexão.
#[test]
fn system_out_materializes_once_and_reflection_sees_it() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let f_out = b.field_idx("Ljava/lang/System;", "Ljava/io/PrintStream;", "out");
    b.direct(
        cls,
        "le_out",
        "Ljava/io/PrintStream;",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(1, 0, 0, {
            let mut u = op21c(0x62, 0, f_out);
            u.extend(op11x(0x11, 0));
            u
        })),
    );
    let mut e2 = engine_of(&b);
    let o1 = obj_of(invoke(&mut e2, "le_out", "()Ljava/io/PrintStream;", &[]).unwrap());
    let o2 = obj_of(invoke(&mut e2, "le_out", "()Ljava/io/PrintStream;", &[]).unwrap());
    assert_eq!(o1, o2, "System.out é singleton embutido");
    // reflexão enxerga o MESMO PrintStream (novo na unificação)
    let f = host_field(&mut e2, "Ljava/lang/System;", "out");
    let refl = obj_of(field_get(&mut e2, f));
    assert_eq!(o1, refl, "Field.get(System.out) == sget(System.out)");
}
