//! disasm.rs — disassembler smali (M1-A2), convenções de saída baksmali 2.5.2.
//!
//! Renderiza uma classe DEX em texto smali consumível pelo montador smali:
//! layout `.class`/`.super`/`.source`/`.implements`, seções `# static fields`,
//! `# instance fields`, `# direct methods`, `# virtual methods`, blocos
//! `.method` com `.registers`, `.param`, `.prologue`/`.epilogue`/`.line`/
//! `.local` (debug info), `.annotation` (todos os níveis, com subannotations),
//! `.catch`/`.catchall` posicionados no endereço do `try_end` e payloads como
//! blocos `.packed-switch`/`.sparse-switch`/`.array-data`.
//!
//! Determinismo (Lei 3 do disassembler): toda iteração dependente de ordem usa
//! `BTreeMap`/ordem do arquivo — nada de permutação por hash. Labels
//! (`:cond_N`, `:goto_N`, `:pswitch_N`, `:sswitch_N`, `:pswitch_data_N`,
//! `:sswitch_data_N`, `:array_data_N`, `:try_start_N`, `:try_end_N`,
//! `:catch_N`, `:catchall_N`) são numeradas por ordem crescente de endereço,
//! por tipo, reiniciando a numeração a cada método.
//!
//! Registradores: `.registers N` (default do baksmali); registradores de
//! entrada usam a notação `pK` quando `reg >= registers_size - ins_size`
//! (`p0` = `this` em métodos não-estáticos), `vK` caso contrário.
//!
//! Opcodes "unused" (0x3e–0x43, 0x73, 0x79–0x7a, 0xe3–0xf9) são renderizados
//! como `nop // unused 0xXX`; `nop` real (0x00) vira `nop` puro. 0xd8–0xe2 são
//! os `*int/lit8` válidos — NÃO fazem parte do intervalo unused (issue #13).

use std::collections::BTreeMap;
use std::rc::Rc;

use crate::annotations::{
    parse_annotation_item, parse_annotation_set_item, parse_annotation_set_ref_list,
    parse_annotations_directory, AnnotationItem, EncodedAnnotation, EncodedValue,
};
use crate::classes::ClassDef;
use crate::code::{CodeItem, Insn, Kind, Payload};
use crate::debug::{DebugEvent, DebugInfo, LocalEvent};
use crate::dex::Dex;
use crate::error::{RdError, RdResult};
use crate::fields::{
    format_flags, FieldId, ACC_STATIC, CLASS_FLAG_ORDER, FIELD_FLAG_ORDER, METHOD_FLAG_ORDER,
};
use crate::mutf8;
use crate::opcode::info;
use crate::strings::NO_INDEX;

/// Indentação base de bloco (estilo baksmali: 4 espaços).
const IND: &str = "    ";

// prefixes de label (convenção baksmali; `:array_data_N` segue a spec da tarefa)
const COND: &str = ":cond_";
const GOTO: &str = ":goto_";
const PSWITCH: &str = ":pswitch_";
const SSWITCH: &str = ":sswitch_";
const PSWITCH_DATA: &str = ":pswitch_data_";
const SSWITCH_DATA: &str = ":sswitch_data_";
const ARRAY_DATA: &str = ":array_data_";
const TRY_START: &str = ":try_start_";
const TRY_END: &str = ":try_end_";
const CATCH: &str = ":catch_";
const CATCHALL: &str = ":catchall_";

/// Opaco em usize::MAX — marcador de "offset ausente" nos builders de teste.
#[cfg(test)]
const SENTINEL: u32 = u32::MAX;

// ── orçamento de renderização (anti-DoS / anti-DAG #285) ────────────────────

/// Teto absoluto de texto smali produzido por uma chamada a `render_class` (`16 MiB`).
const MAX_RENDER_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
/// Teto absoluto de passos estruturais visitados em `render_class`.
const MAX_RENDER_STEPS: usize = 250_000;
/// Profundidade máxima ao renderizar `EncodedValue` recursivo (`Array`/`Annotation`).
const MAX_RENDER_VALUE_DEPTH: usize = 64;

struct RenderWriter {
    buf: String,
    max_bytes: usize,
    steps: usize,
    max_steps: usize,
    ann_set_cache: BTreeMap<u32, Rc<[u32]>>,
    ann_item_cache: BTreeMap<u32, Rc<AnnotationItem>>,
    code_cache: BTreeMap<u32, Rc<CodeItem>>,
    debug_cache: BTreeMap<u32, Rc<Option<DebugInfo>>>,
}

impl RenderWriter {
    fn new(dex_len: usize) -> Self {
        let max_bytes = dex_len
            .saturating_mul(32)
            .clamp(256 * 1024, MAX_RENDER_OUTPUT_BYTES);
        let max_steps = dex_len.saturating_mul(8).clamp(16_384, MAX_RENDER_STEPS);
        Self {
            buf: String::with_capacity(8192),
            max_bytes,
            steps: 0,
            max_steps,
            ann_set_cache: BTreeMap::new(),
            ann_item_cache: BTreeMap::new(),
            code_cache: BTreeMap::new(),
            debug_cache: BTreeMap::new(),
        }
    }

    #[inline]
    fn step(&mut self, n: usize) -> RdResult<()> {
        self.steps = self.steps.saturating_add(n);
        if self.steps > self.max_steps {
            return Err(RdError::parse(format!(
                "disasm: orçamento de trabalho excedido ({} > {} itens)",
                self.steps, self.max_steps
            )));
        }
        Ok(())
    }

    #[inline]
    fn remaining_bytes(&self) -> usize {
        self.max_bytes.saturating_sub(self.buf.len())
    }

    #[inline]
    fn push_str(&mut self, s: &str) -> RdResult<()> {
        let next = self.buf.len().saturating_add(s.len());
        if next > self.max_bytes {
            return Err(RdError::parse(format!(
                "disasm: saída smali excede orçamento ({} > {} bytes)",
                next, self.max_bytes
            )));
        }
        self.buf.push_str(s);
        Ok(())
    }

    #[inline]
    fn push(&mut self, ch: char) -> RdResult<()> {
        let next = self.buf.len().saturating_add(ch.len_utf8());
        if next > self.max_bytes {
            return Err(RdError::parse(format!(
                "disasm: saída smali excede orçamento ({} > {} bytes)",
                next, self.max_bytes
            )));
        }
        self.buf.push(ch);
        Ok(())
    }

    #[inline]
    fn ends_with(&self, pat: &str) -> bool {
        self.buf.ends_with(pat)
    }

    fn push_escaped_string(&mut self, raw: &str) -> RdResult<()> {
        // Todo caractere consome >= 1 byte no literal smali (+ 2 aspas).
        if raw.len().saturating_add(2) > self.remaining_bytes() {
            return Err(RdError::parse(format!(
                "disasm: literal de string ({} bytes) excede orçamento restante ({} bytes)",
                raw.len(),
                self.remaining_bytes()
            )));
        }
        let escaped = mutf8::escape_string(raw);
        self.push_str(&escaped)
    }

    fn escaped_string(&self, raw: &str) -> RdResult<String> {
        if raw.len().saturating_add(2) > self.remaining_bytes() {
            return Err(RdError::parse(format!(
                "disasm: literal de string ({} bytes) excede orçamento restante ({} bytes)",
                raw.len(),
                self.remaining_bytes()
            )));
        }
        Ok(mutf8::escape_string(raw))
    }

    fn annotation_set(&mut self, data: &[u8], off: u32) -> RdResult<Rc<[u32]>> {
        if let Some(cached) = self.ann_set_cache.get(&off) {
            let rc = Rc::clone(cached);
            self.step(rc.len().max(1))?;
            return Ok(rc);
        }
        let parsed = parse_annotation_set_item(data, off)?;
        self.step(parsed.len().max(1))?;
        let rc: Rc<[u32]> = parsed.into();
        self.ann_set_cache.insert(off, Rc::clone(&rc));
        Ok(rc)
    }

    fn annotation_item(&mut self, data: &[u8], off: u32) -> RdResult<Rc<AnnotationItem>> {
        if let Some(cached) = self.ann_item_cache.get(&off) {
            let rc = Rc::clone(cached);
            self.step(rc.annotation.elements.len().max(1))?;
            return Ok(rc);
        }
        let parsed = parse_annotation_item(data, off)?;
        self.step(parsed.annotation.elements.len().max(1))?;
        let rc = Rc::new(parsed);
        self.ann_item_cache.insert(off, Rc::clone(&rc));
        Ok(rc)
    }

    fn code_item(&mut self, dex: &Dex, off: u32) -> RdResult<Rc<CodeItem>> {
        if let Some(cached) = self.code_cache.get(&off) {
            let rc = Rc::clone(cached);
            self.step(rc.instructions.len().max(1))?;
            return Ok(rc);
        }
        let parsed = dex
            .code(off)?
            .ok_or_else(|| RdError::parse(format!("code_item ausente no offset 0x{off:x}")))?;
        self.step(parsed.instructions.len().max(1))?;
        let rc = Rc::new(parsed);
        self.code_cache.insert(off, Rc::clone(&rc));
        Ok(rc)
    }

    fn debug_item(&mut self, dex: &Dex, off: u32) -> RdResult<Rc<Option<DebugInfo>>> {
        if let Some(cached) = self.debug_cache.get(&off) {
            let rc = Rc::clone(cached);
            if let Some(dbg) = rc.as_ref() {
                self.step(dbg.entries.len().max(1))?;
            }
            return Ok(rc);
        }
        let parsed = dex.debug_info(off)?;
        if let Some(dbg) = &parsed {
            self.step(dbg.entries.len().max(1))?;
        }
        let rc = Rc::new(parsed);
        self.debug_cache.insert(off, Rc::clone(&rc));
        Ok(rc)
    }
}

// ── API pública ─────────────────────────────────────────────────────────────

/// Renderiza uma classe no formato smali (baksmali-compatible).
pub fn render_class(dex: &Dex, class_index: usize) -> RdResult<String> {
    let def = dex.class_defs.get(class_index).ok_or_else(|| {
        RdError::missing_entry(format!(
            "class #{class_index} (o dex tem {} classes)",
            dex.class_defs.len()
        ))
        .with_suggestion("list classes with `rd dex disasm <path>` (without --class)")
    })?;
    let mut out = RenderWriter::new(dex.data.len());
    render_class_into(dex, def, &mut out)?;
    Ok(out.buf)
}

/// Renderiza apenas o bloco `.method` do método `method_name` da classe.
/// Sobrecarga (mesmo nome) → primeiro método (direct antes de virtual).
pub fn render_method(dex: &Dex, class_index: usize, method_name: &str) -> RdResult<String> {
    let text = render_class(dex, class_index)?;
    extract_method_block(&text, method_name).ok_or_else(|| {
        RdError::missing_entry(format!("method {method_name:?} na classe #{class_index}"))
            .with_suggestion("check the method name in the `.method` header of the class smali")
    })
}

/// Mapeia descritor → nome de arquivo smali (`Lcom/x/Y;` → `Lcom/x/Y.smali`).
pub fn smali_file_name(descriptor: &str) -> String {
    let base = descriptor.strip_suffix(';').unwrap_or(descriptor);
    format!("{base}.smali")
}

// ── labels ──────────────────────────────────────────────────────────────────

/// Tabelas de labels por tipo — endereço → índice. Numeração por ordem
/// crescente de endereço, atribuída após a coleta completa (fase 2).
#[derive(Debug, Default)]
struct Labels {
    cond: BTreeMap<u32, usize>,
    goto: BTreeMap<u32, usize>,
    pswitch: BTreeMap<u32, usize>,
    sswitch: BTreeMap<u32, usize>,
    pswitch_data: BTreeMap<u32, usize>,
    sswitch_data: BTreeMap<u32, usize>,
    array_data: BTreeMap<u32, usize>,
    try_start: BTreeMap<u32, usize>,
    try_end: BTreeMap<u32, usize>,
    catch: BTreeMap<u32, usize>,
    catchall: BTreeMap<u32, usize>,
}

impl Labels {
    fn number_all(&mut self) {
        for m in [
            &mut self.cond,
            &mut self.goto,
            &mut self.pswitch,
            &mut self.sswitch,
            &mut self.pswitch_data,
            &mut self.sswitch_data,
            &mut self.array_data,
            &mut self.try_start,
            &mut self.try_end,
            &mut self.catch,
            &mut self.catchall,
        ] {
            for (i, v) in m.values_mut().enumerate() {
                *v = i;
            }
        }
    }
}

/// Registra alvo de branch se for endereço válido (>= 0; alvos negativos são
/// stream malformada — ignorados, nunca pânico).
fn ins_target(m: &mut BTreeMap<u32, usize>, addr: usize, off: i64) {
    if let Ok(t) = u32::try_from(addr as i64 + off) {
        m.insert(t, 0);
    }
}

fn lab(m: &BTreeMap<u32, usize>, prefix: &str, addr: u32) -> String {
    let i = m.get(&addr).copied().unwrap_or(0);
    format!("{prefix}{i}")
}

fn reg_name(n: usize, reg_base: usize) -> String {
    if n >= reg_base {
        format!("p{}", n - reg_base)
    } else {
        format!("v{n}")
    }
}

/// Literal de array-data: SIGNED com sufixo baksmali (t/s/L).
fn fmt_array_lit(v: i64, suffix: &str) -> String {
    let base = long_lit(v);
    match suffix {
        "" => base,
        "L" => base,
        sfx => {
            // byte/short: sufixo sempre presente
            if v < 0 {
                format!("-0x{:x}{sfx}", v.unsigned_abs())
            } else {
                format!("0x{v:x}{sfx}")
            }
        }
    }
}

/// Literal long: sufixo `L` apenas quando NÃO cabe em i32 (regra baksmali).
fn long_lit(v: i64) -> String {
    let base = signed_hex(v);
    if (i32::MIN as i64..=i32::MAX as i64).contains(&v) {
        base
    } else {
        format!("{base}L")
    }
}

fn signed_hex(v: i64) -> String {
    if v < 0 {
        format!("-0x{:x}", v.unsigned_abs())
    } else {
        format!("0x{v:x}")
    }
}

fn indent(n: usize) -> String {
    " ".repeat(n)
}

fn fmt_f32(v: f32) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v < 0.0 {
            "-Infinity".into()
        } else {
            "Infinity".into()
        };
    }
    ensure_dot(format!("{v}"))
}

fn fmt_f64(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v < 0.0 {
            "-Infinity".into()
        } else {
            "Infinity".into()
        };
    }
    ensure_dot(format!("{v}"))
}

fn ensure_dot(s: String) -> String {
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{s}.0")
    }
}

// ── renderização da classe ──────────────────────────────────────────────────

fn render_class_into(dex: &Dex, def: &ClassDef, out: &mut RenderWriter) -> RdResult<()> {
    out.step(1)?;
    let this = dex.type_str(def.class_idx);
    let flags = format_flags(def.access_flags, CLASS_FLAG_ORDER);
    if flags.is_empty() {
        out.push_str(&format!(".class {this}\n"))?;
    } else {
        out.push_str(&format!(".class {flags} {this}\n"))?;
    }
    if def.superclass_idx != NO_INDEX {
        out.push_str(&format!(".super {}\n", dex.type_str(def.superclass_idx)))?;
    }
    if def.source_file_idx != NO_INDEX {
        out.push_str(".source ")?;
        out.push_escaped_string(dex.string(def.source_file_idx))?;
        out.push('\n')?;
    }

    let dir = if def.annotations_off != 0 {
        Some(parse_annotations_directory(&dex.data, def.annotations_off)?)
    } else {
        None
    };

    if let Some(d) = &dir {
        if d.class_annotations_off != 0 {
            out.push_str("\n# annotations\n")?;
            let set = out.annotation_set(&dex.data, d.class_annotations_off)?;
            for &off in set.iter() {
                let item = out.annotation_item(&dex.data, off)?;
                write_annotation_block(dex, out, &item, 0)?;
            }
        }
    }

    let interfaces = dex.interfaces(def)?;
    if !interfaces.is_empty() {
        out.step(interfaces.len())?;
        out.push_str("\n# interfaces\n")?;
        for &t in &interfaces {
            out.push_str(&format!(".implements {}\n", dex.type_str(t)))?;
        }
    }

    let Some(cd) = dex.class_data(def)? else {
        return Ok(());
    };

    // diretórios de anotação → mapas ordenados por índice (determinismo)
    // spec §annotations_directory_item exige índices únicos e ordenados; rejeitar
    // duplicatas impede amplificação quadrática por entradas repetidas (#285).
    let mut field_anns: BTreeMap<u32, Rc<[u32]>> = BTreeMap::new();
    let mut method_anns: BTreeMap<u32, Rc<[u32]>> = BTreeMap::new();
    let mut param_anns: BTreeMap<u32, Vec<Option<u32>>> = BTreeMap::new();
    if let Some(d) = &dir {
        for (idx, off) in &d.field_annotations {
            if field_anns.contains_key(idx) {
                return Err(RdError::invalid_format(format!(
                    "annotations_directory: field_idx {idx} duplicado"
                )));
            }
            let set = out.annotation_set(&dex.data, *off)?;
            field_anns.insert(*idx, set);
        }
        for (idx, off) in &d.method_annotations {
            if method_anns.contains_key(idx) {
                return Err(RdError::invalid_format(format!(
                    "annotations_directory: method_idx {idx} duplicado"
                )));
            }
            let set = out.annotation_set(&dex.data, *off)?;
            method_anns.insert(*idx, set);
        }
        for (idx, off) in &d.parameter_annotations {
            if param_anns.contains_key(idx) {
                return Err(RdError::invalid_format(format!(
                    "annotations_directory: parameter method_idx {idx} duplicado"
                )));
            }
            let refs = parse_annotation_set_ref_list(&dex.data, *off)?;
            out.step(refs.len().max(1))?;
            param_anns.insert(
                *idx,
                refs.into_iter()
                    .map(|o| if o == 0 { None } else { Some(o) })
                    .collect(),
            );
        }
    }

    let svals = dex.static_values(def)?;

    if !cd.static_fields.is_empty() {
        out.push_str("\n# static fields\n")?;
        for (i, f) in cd.static_fields.iter().enumerate() {
            let fid = dex.field(f.field_idx)?;
            write_field(
                dex,
                out,
                &fid,
                f.access_flags,
                svals.get(i),
                field_anns.get(&f.field_idx).map(Rc::as_ref),
            )?;
        }
    }
    if !cd.instance_fields.is_empty() {
        out.push_str("\n# instance fields\n")?;
        for f in &cd.instance_fields {
            let fid = dex.field(f.field_idx)?;
            write_field(dex, out, &fid, f.access_flags, None, None)?;
        }
    }
    if !cd.direct_methods.is_empty() {
        out.push_str("\n# direct methods\n")?;
        for m in &cd.direct_methods {
            write_method(
                dex,
                out,
                m,
                method_anns.get(&m.method_idx).map(Rc::as_ref),
                param_anns.get(&m.method_idx),
            )?;
        }
    }
    if !cd.virtual_methods.is_empty() {
        out.push_str("\n# virtual methods\n")?;
        for m in &cd.virtual_methods {
            write_method(
                dex,
                out,
                m,
                method_anns.get(&m.method_idx).map(Rc::as_ref),
                param_anns.get(&m.method_idx),
            )?;
        }
    }
    Ok(())
}

fn write_field(
    dex: &Dex,
    out: &mut RenderWriter,
    fid: &FieldId,
    flags: u32,
    value: Option<&EncodedValue>,
    anns: Option<&[u32]>,
) -> RdResult<()> {
    out.step(1)?;
    out.push_str(".field")?;
    let fl = format_flags(flags, FIELD_FLAG_ORDER);
    if !fl.is_empty() {
        out.push(' ')?;
        out.push_str(&fl)?;
    }
    out.push(' ')?;
    out.push_str(dex.string(fid.name_idx))?;
    out.push(':')?;
    out.push_str(dex.type_str(u32::from(fid.type_idx)))?;
    if let Some(v) = value {
        out.push_str(" = ")?;
        write_encoded_value(dex, out, v, 0, 0)?;
    }
    out.push('\n')?;
    if let Some(list) = anns {
        if !list.is_empty() {
            for &off in list {
                let item = out.annotation_item(&dex.data, off)?;
                write_annotation_block(dex, out, &item, 4)?;
            }
            out.push_str(".end field\n")?;
        }
    }
    Ok(())
}

fn write_method(
    dex: &Dex,
    out: &mut RenderWriter,
    em: &crate::methods::EncodedMethod,
    method_anns: Option<&[u32]>,
    param_anns: Option<&Vec<Option<u32>>>,
) -> RdResult<()> {
    out.step(1)?;
    let mid = dex.method(em.method_idx)?;
    let proto = dex.proto(mid.proto_idx)?;
    let params = dex.proto_params(mid.proto_idx)?;
    out.step(params.len())?;
    let mut sig = String::from("(");
    for &p in &params {
        sig.push_str(dex.type_str(p));
    }
    sig.push(')');
    sig.push_str(dex.type_str(proto.return_type_idx));

    let fl = format_flags(em.access_flags, METHOD_FLAG_ORDER);
    // separador em branco entre métodos (estilo baksmali)
    if !out.ends_with("\n\n") {
        out.push('\n')?;
    }
    if fl.is_empty() {
        out.push_str(&format!(".method {}{sig}\n", dex.string(mid.name_idx)))?;
    } else {
        out.push_str(&format!(".method {fl} {}{sig}\n", dex.string(mid.name_idx)))?;
    }

    // anotações do método (inclusive em métodos sem corpo)
    let has_anns = method_anns.is_some_and(|l| !l.is_empty());
    if em.code_off == 0 {
        if has_anns {
            out.push('\n')?;
            for &off in method_anns.unwrap_or(&[]) {
                let item = out.annotation_item(&dex.data, off)?;
                write_annotation_block(dex, out, &item, 4)?;
            }
        }
        out.push_str(".end method\n")?;
        return Ok(());
    }

    let code = out.code_item(dex, em.code_off)?;
    let debug = if code.debug_info_off != 0 {
        out.debug_item(dex, code.debug_info_off)?
    } else {
        Rc::new(None)
    };
    let is_static = em.access_flags & ACC_STATIC != 0;

    out.push_str(&format!("    .registers {}\n\n", code.registers_size))?;
    write_params(
        dex,
        out,
        &params,
        debug.as_ref().as_ref(),
        param_anns,
        is_static,
    )?;

    if has_anns {
        out.push('\n')?;
        for &off in method_anns.unwrap_or(&[]) {
            let item = out.annotation_item(&dex.data, off)?;
            write_annotation_block(dex, out, &item, 4)?;
        }
        out.push('\n')?;
    }

    let md = MethodRender::build(dex, &code, debug.as_ref().as_ref(), out)?;
    md.write_body(out)?;

    out.push_str(".end method\n")?;
    Ok(())
}

/// Largura de registrador de um parâmetro (J/D = wide = 2 registradores).
fn param_width(desc: &str) -> usize {
    if desc.starts_with('J') || desc.starts_with('D') {
        2
    } else {
        1
    }
}

/// Diretivas `.param` (nomes do debug info) + anotações de parâmetro.
/// Numeração pN: p0 = this (não-estático); parâmetro wide consome 2 registradores.
fn write_params(
    dex: &Dex,
    out: &mut RenderWriter,
    params: &[u32],
    debug: Option<&DebugInfo>,
    param_anns: Option<&Vec<Option<u32>>>,
    is_static: bool,
) -> RdResult<()> {
    let names = debug.map(|d| &d.parameter_names);
    let mut preg = if is_static { 0usize } else { 1usize };
    let mut any = false;
    for (k, &t) in params.iter().enumerate() {
        out.step(1)?;
        let tdesc = dex.type_str(t);
        let name = names
            .and_then(|v| v.get(k))
            .copied()
            .flatten()
            .filter(|&i| i != NO_INDEX)
            .map(|i| dex.string(i));
        let ann_off = param_anns.and_then(|v| v.get(k)).copied().flatten();
        if let Some(n) = name {
            let esc = out.escaped_string(n)?;
            out.push_str(&format!("    .param p{preg}, {esc}    # {tdesc}\n"))?;
            any = true;
        }
        if let Some(off) = ann_off {
            if name.is_none() {
                out.push_str(&format!("    .param p{preg}\n"))?;
                any = true;
            }
            let set = out.annotation_set(&dex.data, off)?;
            for &aoff in set.iter() {
                let item = out.annotation_item(&dex.data, aoff)?;
                write_annotation_block(dex, out, &item, 8)?;
            }
            out.push_str("    .end param\n")?;
        }
        preg += param_width(tdesc);
    }
    if any {
        out.push('\n')?;
    }
    Ok(())
}

// ── corpo de método (instruções, labels, tries, debug) ──────────────────────

struct MethodRender<'a> {
    dex: &'a Dex,
    code: &'a CodeItem,
    labels: Labels,
    /// `.catch`/`.catchall` por endereço de try_end (ordem do arquivo).
    catches: BTreeMap<u32, Vec<String>>,
    /// diretivas de debug por endereço (`.prologue`/`.epilogue`/`.line`/`.local`).
    directives: BTreeMap<u32, Vec<String>>,
    /// endereço do payload → endereço da instrução switch/fill que o referencia.
    payload_switch: BTreeMap<u32, u32>,
    reg_base: usize,
}

impl<'a> MethodRender<'a> {
    fn build(
        dex: &'a Dex,
        code: &'a CodeItem,
        debug: Option<&DebugInfo>,
        out: &mut RenderWriter,
    ) -> RdResult<MethodRender<'a>> {
        out.step(code.instructions.len().saturating_add(code.tries.len()))?;
        // issue #20: saturating_sub defensivo — o CodeItem::parse agora rejeita
        // ins_size > registers_size, mas o render não deve wrapar mesmo se
        // chamado com um CodeItem construído fora do parse
        let reg_base = code.registers_size.saturating_sub(code.ins_size) as usize;
        let mut labels = Labels::default();
        let mut payload_switch: BTreeMap<u32, u32> = BTreeMap::new();

        // payloads por endereço (para resolver os alvos dos switches)
        let mut payloads: BTreeMap<u32, &Payload> = BTreeMap::new();
        for (addr, insn) in &code.instructions {
            if let Kind::Payload(p) = &insn.kind {
                payloads.insert(*addr as u32, p);
            }
        }

        for (addr, insn) in &code.instructions {
            match &insn.kind {
                Kind::Branch8(o) => ins_target(&mut labels.goto, *addr, *o as i64),
                Kind::Branch16(o) => ins_target(&mut labels.goto, *addr, *o as i64),
                Kind::Branch32(o) => ins_target(&mut labels.goto, *addr, *o as i64),
                Kind::RegBranch16(_, o) => ins_target(&mut labels.cond, *addr, *o as i64),
                Kind::RegRegBranch16(_, _, o) => ins_target(&mut labels.cond, *addr, *o as i64),
                Kind::RegBranch32(_, o) => {
                    // 31t: fill-array-data / packed-switch / sparse-switch
                    let t = *addr as i64 + *o as i64;
                    match insn.opcode {
                        0x26 => ins_target(&mut labels.array_data, *addr, *o as i64),
                        0x2b => ins_target(&mut labels.pswitch_data, *addr, *o as i64),
                        0x2c => ins_target(&mut labels.sswitch_data, *addr, *o as i64),
                        _ => {}
                    }
                    if let Ok(t) = u32::try_from(t) {
                        payload_switch.insert(t, *addr as u32);
                    }
                }
                _ => {}
            }
        }

        // alvos dos payloads (relativos ao endereço da instrução switch)
        for (&paddr, &saddr) in &payload_switch {
            if let Some(p) = payloads.get(&paddr) {
                match p {
                    Payload::PackedSwitch { targets, .. } => {
                        out.step(targets.len())?;
                        for t in targets {
                            ins_target(&mut labels.pswitch, saddr as usize, *t as i64);
                        }
                    }
                    Payload::SparseSwitch { targets, .. } => {
                        out.step(targets.len())?;
                        for t in targets {
                            ins_target(&mut labels.sswitch, saddr as usize, *t as i64);
                        }
                    }
                    Payload::ArrayData { .. } => {}
                }
            }
        }

        // tries + handlers → labels e linhas .catch
        for t in &code.tries {
            labels.try_start.insert(t.start_addr, 0);
            labels.try_end.insert(t.start_addr + t.insn_count as u32, 0);
        }
        for h in &code.handlers {
            for (_, addr) in &h.typed {
                labels.catch.insert(*addr, 0);
            }
            if let Some(addr) = h.catch_all {
                labels.catchall.insert(addr, 0);
            }
        }
        labels.number_all();

        let mut catches: BTreeMap<u32, Vec<String>> = BTreeMap::new();
        for (t, h) in code.tries.iter().zip(&code.handlers) {
            out.step(
                h.typed
                    .len()
                    .saturating_add(usize::from(h.catch_all.is_some())),
            )?;
            let end = t.start_addr + t.insn_count as u32;
            let entry = catches.entry(end).or_default();
            for (ty, addr) in &h.typed {
                entry.push(format!(
                    ".catch {} {{{} .. {}}} {}",
                    dex.type_str(*ty),
                    lab(&labels.try_start, TRY_START, t.start_addr),
                    lab(&labels.try_end, TRY_END, end),
                    lab(&labels.catch, CATCH, *addr)
                ));
            }
            if let Some(addr) = h.catch_all {
                entry.push(format!(
                    ".catchall {{{} .. {}}} {}",
                    lab(&labels.try_start, TRY_START, t.start_addr),
                    lab(&labels.try_end, TRY_END, end),
                    lab(&labels.catchall, CATCHALL, addr)
                ));
            }
        }

        // diretivas de debug: .prologue/.epilogue, .line (dedup), .local
        let mut directives: BTreeMap<u32, Vec<String>> = BTreeMap::new();
        let mut last_line: Option<u32> = None;
        if let Some(dbg) = debug {
            out.step(dbg.entries.len())?;
            for e in &dbg.entries {
                out.step(e.events.len())?;
                let entry = directives.entry(e.addr).or_default();
                for ev in &e.events {
                    match ev {
                        DebugEvent::SetPrologueEnd => entry.push(".prologue".into()),
                        DebugEvent::SetEpilogueBegin => entry.push(".epilogue".into()),
                        DebugEvent::SetFile(_) => {}
                        DebugEvent::Local(_) => {}
                    }
                }
                if last_line != Some(e.line) {
                    entry.push(format!(".line {}", e.line));
                    last_line = Some(e.line);
                }
                for ev in &e.events {
                    if let DebugEvent::Local(l) = ev {
                        entry.push(render_local(dex, l, reg_base, out)?);
                    }
                }
            }
        }

        Ok(MethodRender {
            dex,
            code,
            labels,
            catches,
            directives,
            payload_switch,
            reg_base,
        })
    }

    fn r(&self, n: impl Into<u32>) -> String {
        reg_name(n.into() as usize, self.reg_base)
    }

    fn field_ref(&self, idx: u32) -> RdResult<String> {
        let f = self.dex.field(idx)?;
        Ok(format!(
            "{}->{}:{}",
            self.dex.type_str(f.class_idx as u32),
            self.dex.string(f.name_idx),
            self.dex.type_str(f.type_idx as u32)
        ))
    }

    fn method_ref(&self, idx: u32) -> RdResult<String> {
        let m = self.dex.method(idx)?;
        Ok(format!(
            "{}->{}{}",
            self.dex.type_str(m.class_idx as u32),
            self.dex.string(m.name_idx),
            proto_sig(self.dex, m.proto_idx)?
        ))
    }

    /// Operando de índice para formatos 21c (opcode → string/tipo/campo).
    fn index_ref_21c(&self, opcode: u8, idx: u16, out: &RenderWriter) -> RdResult<String> {
        match opcode {
            0x1a | 0x1b => out.escaped_string(self.dex.string(idx as u32)),
            0x1c | 0x1f | 0x22 => Ok(self.dex.type_str(idx as u32).to_string()),
            0x60..=0x6d => self.field_ref(idx as u32),
            _ => Ok(format!("index@{idx}")),
        }
    }

    /// Operando de índice para formatos 22c (tipo ou campo).
    fn index_ref_22c(&self, opcode: u8, idx: u16) -> RdResult<String> {
        match opcode {
            0x20 | 0x23 => Ok(self.dex.type_str(idx as u32).to_string()),
            0x52..=0x5e => self.field_ref(idx as u32),
            _ => Ok(format!("index@{idx}")),
        }
    }

    /// Lista de registradores `{vA, vB, …}` de um invoke 35c.
    fn reg_list_35c(&self, count: u8, regs: &[u8; 5]) -> String {
        let mut list = String::from("{");
        for (i, reg) in regs.iter().enumerate().take(count as usize) {
            if i > 0 {
                list.push_str(", ");
            }
            list.push_str(&self.r(*reg));
        }
        list.push('}');
        list
    }

    /// Faixa de registradores `{vC .. vN}` de um invoke /range (3rc/4rcc).
    fn reg_range_3rc(&self, count: u8, start: u16) -> String {
        if count == 0 {
            return "{}".into();
        }
        let first = self.r(start as u32);
        let last = self.r(start as u32 + count as u32 - 1);
        format!("{{{} .. {}}}", first, last)
    }

    fn render_insn(&self, addr: usize, insn: &Insn, out: &RenderWriter) -> RdResult<String> {
        let o = info(insn.opcode);
        if o.is_unused() {
            return Ok(format!("nop // unused 0x{:02x}", insn.opcode));
        }
        let name = o.name;
        Ok(match &insn.kind {
            Kind::Plain => name.to_string(),
            Kind::Regs(a, b) => format!("{name} {}, {}", self.r(*a), self.r(*b)),
            Kind::RegLit4(a, lit) => {
                format!("{name} {}, {}", self.r(*a), signed_hex(*lit as i64))
            }
            Kind::Reg(a) => format!("{name} {}", self.r(*a)),
            Kind::Branch8(off) => {
                let t = (addr as i64 + *off as i64) as u32;
                format!("{name} {}", lab(&self.labels.goto, GOTO, t))
            }
            Kind::Branch16(off) => {
                let t = (addr as i64 + *off as i64) as u32;
                format!("{name} {}", lab(&self.labels.goto, GOTO, t))
            }
            Kind::Branch32(off) => {
                let t = (addr as i64 + *off as i64) as u32;
                format!("{name} {}", lab(&self.labels.goto, GOTO, t))
            }
            Kind::RegReg16(a, b) => format!("{name} {}, {}", self.r(*a), self.r(*b)),
            Kind::RegBranch16(a, off) => {
                let t = (addr as i64 + *off as i64) as u32;
                format!("{name} {}, {}", self.r(*a), lab(&self.labels.cond, COND, t))
            }
            Kind::RegLit16(a, lit) => {
                format!("{name} {}, {}", self.r(*a), signed_hex(*lit as i64))
            }
            Kind::RegHigh16(a, raw) => {
                let lit = match insn.opcode {
                    0x15 => {
                        let bits = (*raw as u32) << 16;
                        let f = f32::from_bits(bits);
                        format!("0x{bits:08x}    # {}f", fmt_f32(f))
                    }
                    0x19 => {
                        let bits = (*raw as u64) << 48;
                        let d = f64::from_bits(bits);
                        format!("0x{bits:016x}L    # {}", fmt_f64(d))
                    }
                    _ => format!("0x{raw:04x}"),
                };
                format!("{name} {}, {lit}", self.r(*a))
            }
            Kind::RegIndex(a, idx) => {
                let target = self.index_ref_21c(insn.opcode, *idx, out)?;
                format!("{name} {}, {target}", self.r(*a))
            }
            Kind::RegRegReg(a, b, c) => {
                format!("{name} {}, {}, {}", self.r(*a), self.r(*b), self.r(*c))
            }
            Kind::RegRegLit8(a, b, lit) => {
                format!(
                    "{name} {}, {}, {}",
                    self.r(*a),
                    self.r(*b),
                    signed_hex(*lit as i64)
                )
            }
            Kind::RegRegBranch16(a, b, off) => {
                let t = (addr as i64 + *off as i64) as u32;
                format!(
                    "{name} {}, {}, {}",
                    self.r(*a),
                    self.r(*b),
                    lab(&self.labels.cond, COND, t)
                )
            }
            Kind::RegRegLit16(a, b, lit) => format!(
                "{name} {}, {}, {}",
                self.r(*a),
                self.r(*b),
                signed_hex(*lit as i64)
            ),
            Kind::RegRegIndex(a, b, idx) => {
                let target = self.index_ref_22c(insn.opcode, *idx)?;
                format!("{name} {}, {}, {target}", self.r(*a), self.r(*b))
            }
            Kind::RegLit32(a, lit) => {
                format!("{name} {}, {}", self.r(*a), signed_hex(*lit as i64))
            }
            Kind::RegBranch32(a, off) => {
                let t = (addr as i64 + *off as i64) as u32;
                let l = match insn.opcode {
                    0x26 => lab(&self.labels.array_data, ARRAY_DATA, t),
                    0x2b => lab(&self.labels.pswitch_data, PSWITCH_DATA, t),
                    0x2c => lab(&self.labels.sswitch_data, SSWITCH_DATA, t),
                    _ => format!("branch@{t:x}"),
                };
                format!("{name} {}, {l}", self.r(*a))
            }
            Kind::RegIndex32(a, idx) => format!(
                "{name} {}, {}",
                self.r(*a),
                out.escaped_string(self.dex.string(*idx))?
            ),
            Kind::Lit64(a, lit) => {
                format!("{name} {}, {}", self.r(*a), long_lit(*lit))
            }
            Kind::Invoke35c { count, idx, regs } => {
                let operand = match insn.opcode {
                    0x24 => self.dex.type_str(*idx as u32).to_string(),
                    0xfc => format!("call_site@{idx}"),
                    _ => self.method_ref(*idx as u32)?,
                };
                let list = self.reg_list_35c(*count, regs);
                format!("{name} {list}, {operand}")
            }
            Kind::InvokeRange3rc { count, idx, start } => {
                let operand = match insn.opcode {
                    0x25 => self.dex.type_str(*idx as u32).to_string(),
                    0xfd => format!("call_site@{idx}"),
                    _ => self.method_ref(*idx as u32)?,
                };
                let list = self.reg_range_3rc(*count, *start);
                format!("{name} {list}, {operand}")
            }
            Kind::Invoke45cc {
                count,
                idx,
                regs,
                proto,
            } => {
                let list = self.reg_list_35c(*count, regs);
                format!(
                    "{name} {list}, {}, {}",
                    self.method_ref(*idx as u32)?,
                    proto_sig(self.dex, *proto)?
                )
            }
            Kind::InvokeRange4rcc {
                count,
                idx,
                start,
                proto,
            } => {
                let list = self.reg_range_3rc(*count, *start);
                format!(
                    "{name} {list}, {}, {}",
                    self.method_ref(*idx as u32)?,
                    proto_sig(self.dex, *proto)?
                )
            }
            Kind::Payload(_) => "nop // payload".to_string(),
        })
    }

    fn emit_label(
        out: &mut RenderWriter,
        m: &BTreeMap<u32, usize>,
        prefix: &str,
        a: u32,
    ) -> RdResult<()> {
        if let Some(&i) = m.get(&a) {
            out.push_str(&format!("{prefix}{i}\n"))?;
        }
        Ok(())
    }

    fn emit_labels_at(&self, out: &mut RenderWriter, a: u32) -> RdResult<()> {
        Self::emit_label(out, &self.labels.try_start, TRY_START, a)?;
        Self::emit_label(out, &self.labels.try_end, TRY_END, a)?;
        if let Some(lines) = self.catches.get(&a) {
            for l in lines {
                out.step(1)?;
                out.push_str(l)?;
                out.push('\n')?;
            }
        }
        Self::emit_label(out, &self.labels.cond, COND, a)?;
        Self::emit_label(out, &self.labels.goto, GOTO, a)?;
        Self::emit_label(out, &self.labels.pswitch, PSWITCH, a)?;
        Self::emit_label(out, &self.labels.sswitch, SSWITCH, a)?;
        Self::emit_label(out, &self.labels.pswitch_data, PSWITCH_DATA, a)?;
        Self::emit_label(out, &self.labels.sswitch_data, SSWITCH_DATA, a)?;
        Self::emit_label(out, &self.labels.array_data, ARRAY_DATA, a)?;
        Ok(())
    }

    fn emit_directives(&self, out: &mut RenderWriter, a: u32) -> RdResult<()> {
        if let Some(dirs) = self.directives.get(&a) {
            for d in dirs {
                out.step(1)?;
                out.push_str(IND)?;
                out.push_str(d)?;
                out.push('\n')?;
            }
        }
        Ok(())
    }

    fn write_payload(&self, out: &mut RenderWriter, addr: u32, p: &Payload) -> RdResult<()> {
        match p {
            Payload::PackedSwitch { first_key, targets } => {
                out.push_str(&format!(
                    "{}\n",
                    lab(&self.labels.pswitch_data, PSWITCH_DATA, addr)
                ))?;
                out.push_str(&format!(
                    ".packed-switch {}\n",
                    signed_hex(*first_key as i64)
                ))?;
                let sa = self.payload_switch.get(&addr).copied().unwrap_or(addr);
                for t in targets {
                    out.step(1)?;
                    let target = (sa as i64 + *t as i64) as u32;
                    out.push_str(&format!(
                        "{IND}{}\n",
                        lab(&self.labels.pswitch, PSWITCH, target)
                    ))?;
                }
                out.push_str(".end packed-switch\n")?;
            }
            Payload::SparseSwitch { keys, targets } => {
                out.push_str(&format!(
                    "{}\n",
                    lab(&self.labels.sswitch_data, SSWITCH_DATA, addr)
                ))?;
                out.push_str(".sparse-switch\n")?;
                let sa = self.payload_switch.get(&addr).copied().unwrap_or(addr);
                for (k, t) in keys.iter().zip(targets.iter()) {
                    out.step(1)?;
                    let target = (sa as i64 + *t as i64) as u32;
                    out.push_str(&format!(
                        "{IND}{} -> {}\n",
                        signed_hex(*k as i64),
                        lab(&self.labels.sswitch, SSWITCH, target)
                    ))?;
                }
                out.push_str(".end sparse-switch\n")?;
            }
            Payload::ArrayData {
                element_width,
                element_count,
                data,
            } => {
                out.push_str(&format!(
                    "{}\n",
                    lab(&self.labels.array_data, ARRAY_DATA, addr)
                ))?;
                out.push_str(&format!(".array-data {element_width}\n"))?;
                let w = *element_width as usize;
                let total = (*element_count as usize).saturating_mul(w).min(data.len());
                if w > 0 {
                    let mut i = 0usize;
                    while i + w <= total {
                        out.step(1)?;
                        let b = &data[i..i + w];
                        // literais SIGNED (convenção baksmali: -0x1t, -0x40800000, …L)
                        let s = match w {
                            1 => fmt_array_lit(b[0] as i8 as i64, "t"),
                            2 => fmt_array_lit(i16::from_le_bytes([b[0], b[1]]) as i64, "s"),
                            4 => fmt_array_lit(
                                i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as i64,
                                "",
                            ),
                            8 => fmt_array_lit(
                                i64::from_le_bytes([
                                    b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
                                ]),
                                "L",
                            ),
                            _ => {
                                // largura incomum — bytes brutos (tolerante)
                                let hex: String = b.iter().fold(String::new(), |mut acc, x| {
                                    use std::fmt::Write as _;
                                    let _ = write!(acc, "{x:02x}");
                                    acc
                                });
                                format!("0x{hex}")
                            }
                        };
                        out.push_str(&format!("{IND}{s}\n"))?;
                        i += w;
                    }
                }
                out.push_str(".end array-data\n")?;
            }
        }
        Ok(())
    }

    fn write_body(&self, out: &mut RenderWriter) -> RdResult<()> {
        let last_addr = self.code.instructions.last().map(|(a, _)| *a as u32);
        for (addr, insn) in &self.code.instructions {
            out.step(1)?;
            let a = *addr as u32;
            self.emit_labels_at(out, a)?;
            self.emit_directives(out, a)?;
            match &insn.kind {
                Kind::Payload(p) => self.write_payload(out, a, p)?,
                _ => {
                    let rendered = self.render_insn(*addr, insn, out)?;
                    out.push_str(IND)?;
                    out.push_str(&rendered)?;
                    out.push('\n')?;
                }
            }
        }
        // cauda: labels/.catch além da última instrução (try_end no fim do corpo)
        if let Some(last) = last_addr {
            let mut tail: Vec<u32> = Vec::new();
            for m in [
                &self.labels.try_start,
                &self.labels.try_end,
                &self.labels.cond,
                &self.labels.goto,
                &self.labels.pswitch,
                &self.labels.sswitch,
                &self.labels.catch,
                &self.labels.catchall,
            ] {
                for &k in m.keys().skip_while(|&&k| k <= last) {
                    if k > last {
                        tail.push(k);
                    }
                }
            }
            tail.sort_unstable();
            tail.dedup();
            for k in tail {
                self.emit_labels_at(out, k)?;
            }
        }
        Ok(())
    }
}

/// Assinatura de um proto: `(params)ret`.
fn proto_sig(dex: &Dex, idx: u16) -> RdResult<String> {
    let p = dex.proto(idx)?;
    let params = dex.proto_params(idx)?;
    let mut sig = String::from("(");
    for &t in &params {
        sig.push_str(dex.type_str(t));
    }
    sig.push(')');
    sig.push_str(dex.type_str(p.return_type_idx));
    Ok(sig)
}

/// Diretiva `.local` de um evento de variável local (debug info).
fn render_local(
    dex: &Dex,
    l: &LocalEvent,
    reg_base: usize,
    out: &RenderWriter,
) -> RdResult<String> {
    Ok(match l {
        LocalEvent::Start {
            reg,
            name,
            typ,
            sig,
        } => {
            let r = reg_name(*reg as usize, reg_base);
            match (
                name.filter(|&i| i != NO_INDEX),
                typ.filter(|&i| i != NO_INDEX),
            ) {
                (Some(n), Some(t)) => {
                    let esc = out.escaped_string(dex.string(n))?;
                    let mut s = format!(".local {r}, {esc}:{}", dex.type_str(t));
                    if let Some(g) = sig.filter(|&i| i != NO_INDEX) {
                        let gs = dex.string(g);
                        if gs.len() > out.remaining_bytes() {
                            return Err(RdError::parse(
                                "disasm: assinatura .local excede orçamento restante",
                            ));
                        }
                        s.push_str(&format!("    # {gs}"));
                    }
                    s
                }
                _ => format!(".local {r}"),
            }
        }
        LocalEvent::End { reg } => format!(".end local {}", reg_name(*reg as usize, reg_base)),
        LocalEvent::Restart { reg } => {
            format!(".restart local {}", reg_name(*reg as usize, reg_base))
        }
    })
}

// ── anotações ───────────────────────────────────────────────────────────────

fn write_annotation_block(
    dex: &Dex,
    out: &mut RenderWriter,
    item: &AnnotationItem,
    ind: usize,
) -> RdResult<()> {
    write_annotation_inner(dex, out, item.visibility.name(), &item.annotation, ind, 0)
}

fn write_annotation_inner(
    dex: &Dex,
    out: &mut RenderWriter,
    vis: &str,
    ann: &EncodedAnnotation,
    ind: usize,
    depth: usize,
) -> RdResult<()> {
    if depth > MAX_RENDER_VALUE_DEPTH {
        return Err(RdError::parse(format!(
            "disasm: aninhamento de anotação excede {MAX_RENDER_VALUE_DEPTH} níveis"
        )));
    }
    out.step(1)?;
    let pad = indent(ind);
    out.push_str(&format!(
        "{pad}.annotation {vis} {}\n",
        dex.type_str(ann.type_idx)
    ))?;
    for el in &ann.elements {
        out.step(1)?;
        out.push_str(&format!("{pad}    {} = ", dex.string(el.name_idx)))?;
        write_encoded_value(dex, out, &el.value, ind + 4, depth + 1)?;
        out.push('\n')?;
    }
    out.push_str(&format!("{pad}.end annotation\n"))?;
    Ok(())
}

/// Valor `encoded_value` no literal smali escrito diretamente em `out`.
/// `ind` só é usado por `.subannotation` (multiline). Arrays: `{ a, b }` numa linha.
fn write_encoded_value(
    dex: &Dex,
    out: &mut RenderWriter,
    v: &EncodedValue,
    ind: usize,
    depth: usize,
) -> RdResult<()> {
    if depth > MAX_RENDER_VALUE_DEPTH {
        return Err(RdError::parse(format!(
            "disasm: aninhamento de encoded_value excede {MAX_RENDER_VALUE_DEPTH} níveis"
        )));
    }
    out.step(1)?;
    match v {
        EncodedValue::Byte(b) => {
            out.push_str(&signed_hex(*b as i64))?;
            out.push('t')?;
        }
        EncodedValue::Short(s) => {
            out.push_str(&signed_hex(*s as i64))?;
            out.push('s')?;
        }
        EncodedValue::Char(c) => out.push_str(&mutf8::escape_char(*c))?,
        EncodedValue::Int(i) => out.push_str(&signed_hex(*i as i64))?,
        EncodedValue::Long(l) => {
            out.push_str(&signed_hex(*l))?;
            out.push('L')?;
        }
        EncodedValue::Float(f) => out.push_str(&format!("{}f", fmt_f32(*f)))?,
        EncodedValue::Double(d) => out.push_str(&fmt_f64(*d))?,
        EncodedValue::MethodType(idx) => out.push_str(&proto_sig(dex, *idx as u16)?)?,
        EncodedValue::MethodHandle(idx) => match dex.method_handles.get(*idx as usize) {
            Some(h) if crate::methods::method_handle_type::is_invoke(h.method_handle_type) => {
                let m = dex.method(h.field_or_method_idx)?;
                out.push_str(&format!(
                    "{}@{}->{}{}",
                    crate::methods::method_handle_type::name(h.method_handle_type),
                    dex.type_str(m.class_idx as u32),
                    dex.string(m.name_idx),
                    proto_sig(dex, m.proto_idx)?
                ))?;
            }
            Some(h) => {
                let f = dex.field(h.field_or_method_idx)?;
                out.push_str(&format!(
                    "{}@{}->{}:{}",
                    crate::methods::method_handle_type::name(h.method_handle_type),
                    dex.type_str(f.class_idx as u32),
                    dex.string(f.name_idx),
                    dex.type_str(f.type_idx as u32)
                ))?;
            }
            None => out.push_str(&format!("method_handle@{idx}"))?,
        },
        EncodedValue::String(idx) => out.push_escaped_string(dex.string(*idx))?,
        EncodedValue::Type(idx) => out.push_str(dex.type_str(*idx))?,
        EncodedValue::Field(idx) | EncodedValue::Enum(idx) => {
            let f = dex.field(*idx)?;
            out.push_str(&format!(
                "{}->{}:{}",
                dex.type_str(f.class_idx as u32),
                dex.string(f.name_idx),
                dex.type_str(f.type_idx as u32)
            ))?;
        }
        EncodedValue::Method(idx) => {
            let m = dex.method(*idx)?;
            out.push_str(&format!(
                "{}->{}{}",
                dex.type_str(m.class_idx as u32),
                dex.string(m.name_idx),
                proto_sig(dex, m.proto_idx)?
            ))?;
        }
        EncodedValue::Array(elems) => {
            if elems.is_empty() {
                out.push_str("{}")?;
            } else {
                out.push_str("{ ")?;
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ")?;
                    }
                    write_encoded_value(dex, out, e, ind, depth + 1)?;
                }
                out.push_str(" }")?;
            }
        }
        EncodedValue::Annotation(a) => {
            out.push_str(&format!(".subannotation {}\n", dex.type_str(a.type_idx)))?;
            let elem_pad = indent(ind + 4);
            for el in &a.elements {
                out.step(1)?;
                out.push_str(&elem_pad)?;
                out.push_str(dex.string(el.name_idx))?;
                out.push_str(" = ")?;
                write_encoded_value(dex, out, &el.value, ind + 4, depth + 1)?;
                out.push('\n')?;
            }
            out.push_str(&indent(ind))?;
            out.push_str(".end subannotation")?;
        }
        EncodedValue::Null => out.push_str("null")?,
        EncodedValue::Boolean(b) => out.push_str(if *b { "true" } else { "false" })?,
    }
    Ok(())
}

// ── utilidades ──────────────────────────────────────────────────────────────

/// Extrai o bloco `.method … .end method` de um texto smali pelo nome do método.
fn extract_method_block(text: &str, method_name: &str) -> Option<String> {
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        if !line.starts_with(".method ") {
            continue;
        }
        let sig = line.split_whitespace().next_back().unwrap_or("");
        let name = match sig.find('(') {
            Some(p) => &sig[..p],
            None => "",
        };
        if name == method_name {
            let mut block = String::from(line);
            block.push('\n');
            for l in lines.by_ref() {
                block.push_str(l);
                block.push('\n');
                if l == ".end method" {
                    break;
                }
            }
            return Some(block);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dex::Dex;

    // ── construtor mínimo de DEX sintético (fixtures de teste) ─────────────

    struct Db {
        buf: Vec<u8>,
        /// (pos no buf de um u32 absoluto, alvo relativo ao buf)
        patches: Vec<(usize, usize)>,
        /// (pos no buf de um uleb128 de 5 bytes absoluto, alvo relativo ao buf)
        uleb_patches: Vec<(usize, u32)>,
        strings: Vec<String>,
        str_offsets: Vec<u32>,
        types: Vec<u32>,
        protos: Vec<(u32, u32, u32)>,
        fields: Vec<(u16, u16, u32)>,
        methods: Vec<(u16, u16, u32)>,
        classes: Vec<ClassRec>,
    }

    struct ClassRec {
        class_idx: u32,
        access: u32,
        sup: u32,
        source: u32,
        ifaces: u32,
        anns: u32,
        data: u32,
        static_vals: u32,
    }

    impl Db {
        fn new() -> Self {
            Db {
                buf: Vec::new(),
                patches: Vec::new(),
                uleb_patches: Vec::new(),
                strings: Vec::new(),
                str_offsets: Vec::new(),
                types: Vec::new(),
                protos: Vec::new(),
                fields: Vec::new(),
                methods: Vec::new(),
                classes: Vec::new(),
            }
        }

        fn uleb_into(v: u32, out: &mut Vec<u8>) {
            let mut v = v;
            loop {
                let b = (v & 0x7F) as u8;
                v >>= 7;
                if v == 0 {
                    out.push(b);
                    break;
                }
                out.push(b | 0x80);
            }
        }

        fn uleb(&mut self, v: u32) {
            let mut tmp = Vec::new();
            Self::uleb_into(v, &mut tmp);
            self.buf.extend_from_slice(&tmp);
        }

        /// uleb128 de 5 bytes SEMPRE (padding redundante é uleb válido) —
        /// permite remendar o valor absoluto no finish() sem mudar o tamanho.
        fn uleb5_patch(&mut self, v: u32) {
            let pos = self.buf.len();
            for i in 0..4 {
                self.buf.push((((v >> (7 * i)) & 0x7F) | 0x80) as u8);
            }
            self.buf.push(((v >> 28) & 0x0F) as u8);
            self.uleb_patches.push((pos, v));
        }

        fn push(&mut self, b: &[u8]) -> usize {
            let p = self.buf.len();
            self.buf.extend_from_slice(b);
            p
        }

        fn s(&mut self, s: &str) -> u32 {
            if let Some(i) = self.strings.iter().position(|x| x == s) {
                return i as u32;
            }
            let mut tmp = Vec::new();
            Self::uleb_into(s.encode_utf16().count() as u32, &mut tmp);
            let off = self.push(&tmp);
            self.push(&mutf8::encode(s));
            self.push(&[0]);
            self.strings.push(s.to_string());
            self.str_offsets.push(off as u32);
            (self.strings.len() - 1) as u32
        }

        fn t(&mut self, desc: &str) -> u32 {
            let si = self.s(desc);
            if let Some(i) = self.types.iter().position(|&x| x == si) {
                return i as u32;
            }
            self.types.push(si);
            (self.types.len() - 1) as u32
        }

        fn proto(&mut self, params: &[u32], ret: u32) -> u16 {
            let mut shorty = String::new();
            shorty.push(self.shorty_char(ret));
            for &p in params {
                shorty.push(self.shorty_char(p));
            }
            let si = self.s(&shorty);
            let poff = if params.is_empty() {
                SENTINEL
            } else {
                let mut tl = Vec::new();
                tl.extend_from_slice(&(params.len() as u32).to_le_bytes());
                for &p in params {
                    tl.extend_from_slice(&(p as u16).to_le_bytes());
                }
                self.push(&tl) as u32
            };
            self.protos.push((si, ret, poff));
            (self.protos.len() - 1) as u16
        }

        fn shorty_char(&self, type_idx: u32) -> char {
            let si = self.types[type_idx as usize];
            self.strings[si as usize].chars().next().unwrap_or('L')
        }

        fn field(&mut self, class: u16, name: &str, typ: u16) -> u32 {
            let ni = self.s(name);
            self.fields.push((class, typ, ni));
            (self.fields.len() - 1) as u32
        }

        fn method(&mut self, class: u16, name: &str, proto: u16) -> u32 {
            let ni = self.s(name);
            self.methods.push((class, proto, ni));
            (self.methods.len() - 1) as u32
        }

        fn debug(&mut self, line_start: u32, params: &[i64], body: &[u8]) -> u32 {
            let off = self.buf.len() as u32;
            self.uleb(line_start);
            self.uleb(params.len() as u32);
            for &p in params {
                self.uleb((p + 1) as u32);
            }
            self.push(body);
            self.buf.push(0x00);
            off
        }

        #[allow(clippy::too_many_arguments)]
        fn code(
            &mut self,
            registers: u16,
            ins: u16,
            outs: u16,
            insns: &[u16],
            tries: &[(u32, u16)],
            handlers: &[Vec<u8>],
            debug_off: Option<u32>,
        ) -> u32 {
            let off = self.buf.len() as u32;
            self.buf.extend_from_slice(&registers.to_le_bytes());
            self.buf.extend_from_slice(&ins.to_le_bytes());
            self.buf.extend_from_slice(&outs.to_le_bytes());
            self.buf
                .extend_from_slice(&(tries.len() as u16).to_le_bytes());
            let dbg_pos = self.buf.len();
            self.buf.extend_from_slice(&0u32.to_le_bytes());
            if let Some(d) = debug_off {
                self.patches.push((dbg_pos, d as usize));
            }
            self.buf
                .extend_from_slice(&(insns.len() as u32).to_le_bytes());
            for u in insns {
                self.buf.extend_from_slice(&u.to_le_bytes());
            }
            if insns.len() % 2 == 1 {
                self.buf.extend_from_slice(&0u16.to_le_bytes());
            }
            if !tries.is_empty() {
                // spec: encoded_catch_handler_list = size ULEB128 + handlers;
                // handler_off é relativo ao INÍCIO da lista (inclui o size).
                let list_pos = self.buf.len() + 8 * tries.len();
                let mut hoffs = Vec::with_capacity(handlers.len());
                let mut hp = list_pos + 1; // após o byte do size (uleb)
                for h in handlers {
                    hoffs.push(hp);
                    hp += h.len();
                }
                for (i, (start, count)) in tries.iter().enumerate() {
                    self.buf.extend_from_slice(&start.to_le_bytes());
                    self.buf.extend_from_slice(&count.to_le_bytes());
                    self.buf
                        .extend_from_slice(&((hoffs[i] - list_pos) as u16).to_le_bytes());
                }
                self.buf.push(handlers.len() as u8); // size uleb128
                for h in handlers {
                    self.push(h);
                }
            }
            off
        }

        fn class_data(
            &mut self,
            sf: &[(u32, u32)],
            inf: &[(u32, u32)],
            dm: &[(u32, u32, u32)],
            vm: &[(u32, u32, u32)],
        ) -> u32 {
            let off = self.buf.len() as u32;
            self.uleb(sf.len() as u32);
            self.uleb(inf.len() as u32);
            self.uleb(dm.len() as u32);
            self.uleb(vm.len() as u32);
            let mut prev = 0u32;
            for (idx, flags) in sf {
                self.uleb(idx - prev);
                self.uleb(*flags);
                prev = *idx;
            }
            prev = 0;
            for (idx, flags) in inf {
                self.uleb(idx - prev);
                self.uleb(*flags);
                prev = *idx;
            }
            prev = 0;
            for (idx, flags, code) in dm {
                self.uleb(idx - prev);
                self.uleb(*flags);
                self.uleb5_patch(*code);
                prev = *idx;
            }
            prev = 0;
            for (idx, flags, code) in vm {
                self.uleb(idx - prev);
                self.uleb(*flags);
                self.uleb5_patch(*code);
                prev = *idx;
            }
            off
        }

        #[allow(clippy::too_many_arguments)]
        fn class(
            &mut self,
            class_idx: u32,
            access: u32,
            sup: u32,
            source: Option<u32>,
            ifaces: &[u32],
            anns: Option<u32>,
            data: Option<u32>,
            static_vals: Option<u32>,
        ) -> usize {
            let ioff = if ifaces.is_empty() {
                SENTINEL
            } else {
                let mut tl = Vec::new();
                tl.extend_from_slice(&(ifaces.len() as u32).to_le_bytes());
                for &t in ifaces {
                    tl.extend_from_slice(&(t as u16).to_le_bytes());
                }
                self.push(&tl) as u32
            };
            self.classes.push(ClassRec {
                class_idx,
                access,
                sup,
                source: source.unwrap_or(NO_INDEX),
                ifaces: ioff,
                anns: anns.unwrap_or(0),
                data: data.unwrap_or(0),
                static_vals: static_vals.unwrap_or(0),
            });
            self.classes.len() - 1
        }

        /// annotation_set_item (offsets → patch).
        fn annotation_set(&mut self, items: &[u32]) -> u32 {
            let off = self.buf.len() as u32;
            self.buf
                .extend_from_slice(&(items.len() as u32).to_le_bytes());
            for it in items {
                let p = self.buf.len();
                self.buf.extend_from_slice(&0u32.to_le_bytes());
                self.patches.push((p, *it as usize));
            }
            off
        }

        /// annotation_item bruto: visibility + uleb(type) + uleb(count) + elems.
        fn annotation(&mut self, vis: u8, type_idx: u32, count: u32, elems: &[u8]) -> u32 {
            let off = self.buf.len() as u32;
            self.buf.push(vis);
            self.uleb(type_idx);
            self.uleb(count);
            self.push(elems);
            off
        }

        /// annotations_directory_item.
        fn annotations_directory(
            &mut self,
            class_set: Option<u32>,
            fields: &[(u32, u32)],
            methods: &[(u32, u32)],
            params: &[(u32, u32)],
        ) -> u32 {
            let off = self.buf.len() as u32;
            let put_patch = |db: &mut Db, target: Option<u32>| {
                let p = db.buf.len();
                db.buf.extend_from_slice(&0u32.to_le_bytes());
                if let Some(t) = target {
                    db.patches.push((p, t as usize));
                }
            };
            put_patch(self, class_set.filter(|&x| x != 0));
            self.buf
                .extend_from_slice(&(fields.len() as u32).to_le_bytes());
            self.buf
                .extend_from_slice(&(methods.len() as u32).to_le_bytes());
            self.buf
                .extend_from_slice(&(params.len() as u32).to_le_bytes());
            for (i, o) in fields {
                self.buf.extend_from_slice(&i.to_le_bytes());
                put_patch(self, Some(*o));
            }
            for (i, o) in methods {
                self.buf.extend_from_slice(&i.to_le_bytes());
                put_patch(self, Some(*o));
            }
            for (i, o) in params {
                self.buf.extend_from_slice(&i.to_le_bytes());
                put_patch(self, Some(*o));
            }
            off
        }

        fn finish(mut self) -> Vec<u8> {
            let (ns, nt, np, nf, nm, nc) = (
                self.strings.len(),
                self.types.len(),
                self.protos.len(),
                self.fields.len(),
                self.methods.len(),
                self.classes.len(),
            );
            let data_base = 112 + 4 * ns + 4 * nt + 12 * np + 8 * nf + 8 * nm + 32 * nc;
            let type_off = 112 + 4 * ns;
            let proto_off = type_off + 4 * nt;
            let field_off = proto_off + 12 * np;
            let method_off = field_off + 8 * nf;
            let class_off = method_off + 8 * nm;
            let data_off = class_off + 32 * nc;
            let mut d = vec![0u8; data_off.max(112)];
            d[0..8].copy_from_slice(b"dex\n035\0");
            let put32 = |d: &mut Vec<u8>, off: usize, v: u32| {
                d[off..off + 4].copy_from_slice(&v.to_le_bytes());
            };
            put32(&mut d, 36, 112);
            put32(&mut d, 40, 0x1234_5678);
            put32(&mut d, 56, ns as u32);
            put32(&mut d, 60, if ns > 0 { 112 } else { 0 });
            put32(&mut d, 64, nt as u32);
            put32(&mut d, 68, if nt > 0 { type_off as u32 } else { 0 });
            put32(&mut d, 72, np as u32);
            put32(&mut d, 76, if np > 0 { proto_off as u32 } else { 0 });
            put32(&mut d, 80, nf as u32);
            put32(&mut d, 84, if nf > 0 { field_off as u32 } else { 0 });
            put32(&mut d, 88, nm as u32);
            put32(&mut d, 92, if nm > 0 { method_off as u32 } else { 0 });
            put32(&mut d, 96, nc as u32);
            put32(&mut d, 100, if nc > 0 { class_off as u32 } else { 0 });
            put32(&mut d, 104, self.buf.len() as u32);
            put32(&mut d, 108, data_off as u32);

            for i in 0..ns {
                put32(&mut d, 112 + i * 4, data_base as u32 + self.str_offsets[i]);
            }
            for i in 0..nt {
                put32(&mut d, type_off + i * 4, self.types[i]);
            }
            for i in 0..np {
                let (si, ret, poff) = self.protos[i];
                put32(&mut d, proto_off + i * 12, si);
                put32(&mut d, proto_off + i * 12 + 4, ret);
                put32(
                    &mut d,
                    proto_off + i * 12 + 8,
                    if poff == SENTINEL {
                        0
                    } else {
                        data_base as u32 + poff
                    },
                );
            }
            for i in 0..nf {
                let (c, t, n) = self.fields[i];
                let base = field_off + i * 8;
                d[base..base + 2].copy_from_slice(&c.to_le_bytes());
                d[base + 2..base + 4].copy_from_slice(&t.to_le_bytes());
                put32(&mut d, base + 4, n);
            }
            for i in 0..nm {
                let (c, p, n) = self.methods[i];
                let base = method_off + i * 8;
                d[base..base + 2].copy_from_slice(&c.to_le_bytes());
                d[base + 2..base + 4].copy_from_slice(&p.to_le_bytes());
                put32(&mut d, base + 4, n);
            }
            for (i, c) in self.classes.iter().enumerate() {
                let base = class_off + i * 32;
                put32(&mut d, base, c.class_idx);
                put32(&mut d, base + 4, c.access);
                put32(&mut d, base + 8, c.sup);
                put32(
                    &mut d,
                    base + 12,
                    if c.ifaces == SENTINEL {
                        0
                    } else {
                        data_base as u32 + c.ifaces
                    },
                );
                put32(&mut d, base + 16, c.source);
                put32(
                    &mut d,
                    base + 20,
                    if c.anns == 0 {
                        0
                    } else {
                        data_base as u32 + c.anns
                    },
                );
                put32(
                    &mut d,
                    base + 24,
                    if c.data == 0 {
                        0
                    } else {
                        data_base as u32 + c.data
                    },
                );
                put32(
                    &mut d,
                    base + 28,
                    if c.static_vals == 0 {
                        0
                    } else {
                        data_base as u32 + c.static_vals
                    },
                );
            }

            d.extend_from_slice(&self.buf);
            let patches = std::mem::take(&mut self.patches);
            for (pos, target) in patches {
                let at = data_base + pos;
                d[at..at + 4].copy_from_slice(&((data_base + target) as u32).to_le_bytes());
            }
            let uleb_patches = std::mem::take(&mut self.uleb_patches);
            for (pos, target) in uleb_patches {
                let at = data_base + pos;
                let abs = (data_base + target as usize) as u32;
                for i in 0..4 {
                    d[at + i] = (((abs >> (7 * i)) & 0x7F) | 0x80) as u8;
                }
                d[at + 4] = ((abs >> 28) & 0x0F) as u8;
            }
            let file_size = d.len() as u32;
            put32(&mut d, 32, file_size);
            d
        }
    }

    // helpers de encoded_value (encoders dos fixtures)
    fn uleb(v: u32) -> Vec<u8> {
        let mut o = Vec::new();
        Db::uleb_into(v, &mut o);
        o
    }

    fn ev_int(v: i32) -> Vec<u8> {
        let bytes = if (-128..128).contains(&v) {
            1
        } else if (-32768..32768).contains(&v) {
            2
        } else {
            4
        };
        let mut out = vec![0x04 | ((bytes as u8 - 1) << 5)];
        for i in 0..bytes {
            out.push((v >> (8 * i)) as u8);
        }
        out
    }

    fn ev_string(idx: u32) -> Vec<u8> {
        let mut out = vec![0x17];
        out.extend(uleb(idx));
        out
    }

    fn ev_bool(b: bool) -> Vec<u8> {
        let arg = if b { 1u8 } else { 0 };
        vec![0x1f | (arg << 5)]
    }

    fn ev_null() -> Vec<u8> {
        vec![0x1e]
    }

    fn ev_char(c: u16) -> Vec<u8> {
        // char: (arg+1) bytes LE, zero-extendido à esquerda — 2 bytes com arg=1.
        let mut out = vec![0x03 | (1 << 5)];
        out.extend_from_slice(&c.to_le_bytes());
        out
    }

    fn ev_float(f: f32) -> Vec<u8> {
        // spec VALUE_FLOAT: size = arg+1 bytes com os bits de ALTA ordem,
        // "zero-extended to the right" → arg = 3 − bytes de zero à direita.
        let bits = f.to_bits();
        let zero_bytes = ((bits.trailing_zeros() / 8) as u8).min(3);
        let arg = 3 - zero_bytes;
        let mut out = vec![0x10 | (arg << 5)];
        let shifted = bits >> (8 * zero_bytes as u32);
        for i in 0..(arg + 1) {
            out.push((shifted >> (8 * i)) as u8);
        }
        out
    }

    fn ev_array(elems: &[Vec<u8>]) -> Vec<u8> {
        // spec: value_arg=0 + size uleb + valores (encoded_array format)
        let mut out = vec![0x1c];
        out.extend(uleb(elems.len() as u32));
        for e in elems {
            out.extend(e);
        }
        out
    }

    fn ev_subannotation(type_idx: u32, elems: &[(u32, Vec<u8>)]) -> Vec<u8> {
        // spec: value_arg=0 + type uleb + size uleb + (name uleb + value)*
        let mut out = vec![0x1d];
        out.extend(uleb(type_idx));
        out.extend(uleb(elems.len() as u32));
        for (n, v) in elems {
            out.extend(uleb(*n));
            out.extend(v);
        }
        out
    }

    /// opcode especial do debug info: avanço de `a` endereços e `dl` linhas.
    fn dbg_sp(a: u32, dl: i32) -> Vec<u8> {
        let adjusted = a * 15 + (dl + 4) as u32;
        vec![0x0a + adjusted as u8]
    }

    fn dbg_start_local(reg: u32, name: i64, typ: i64) -> Vec<u8> {
        let mut o = vec![0x03];
        o.extend(uleb(reg));
        o.extend(uleb((name + 1) as u32));
        o.extend(uleb((typ + 1) as u32));
        o
    }

    fn count_lines(text: &str, prefix: &str) -> usize {
        text.lines().filter(|l| l.starts_with(prefix)).count()
    }

    // ── testes ──────────────────────────────────────────────────────────────

    #[test]
    fn renders_class_layout_fields_and_static_values() {
        let mut b = Db::new();
        let src = b.s("Foo.java");
        let t_obj = b.t("Ljava/lang/Object;");
        let t_foo = b.t("Lcom/test/Foo;");
        let t_str = b.t("Ljava/lang/String;");
        let t_i = b.t("I");
        let t_v = b.t("V");
        let p_void = b.proto(&[], t_v);
        let m_init = b.method(t_foo as u16, "<init>", p_void);
        let m_obj_init = b.method(t_obj as u16, "<init>", p_void);
        let f_tag = b.field(t_foo as u16, "TAG", t_str as u16);
        let f_count = b.field(t_foo as u16, "count", t_i as u16);

        // <init>()V: invoke-direct {p0}, Ljava/lang/Object;-><init>()V; return-void
        let code = b.code(
            1,
            1,
            0,
            &[0x1070, m_obj_init as u16, 0x0000, 0x000e],
            &[],
            &[],
            None,
        );
        let cd = b.class_data(
            &[(f_tag, 0x19)], // public static final
            &[(f_count, 0x2)],
            &[(m_init, 0x1 | 0x1_0000, code)],
            &[],
        );
        // static_values: "hello"
        let s_hello = b.s("hello");
        let mut sv = vec![0x01]; // encoded_array size 1
        sv.extend(ev_string(s_hello));
        let sv_off = b.push(&sv) as u32;
        b.class(
            t_foo,
            0x1,
            t_obj,
            Some(src),
            &[],
            None,
            Some(cd),
            Some(sv_off),
        );

        let dex = Dex::parse(b.finish()).expect("parse fixture");
        let text = render_class(&dex, 0).expect("render");

        assert!(text.starts_with(".class public Lcom/test/Foo;\n"), "{text}");
        assert!(text.contains(".super Ljava/lang/Object;\n"));
        assert!(text.contains(".source \"Foo.java\"\n"));
        assert_eq!(count_lines(&text, ".implements"), 0);
        assert!(text.contains("\n# static fields\n"));
        assert!(
            text.contains(".field public static final TAG:Ljava/lang/String; = \"hello\"\n"),
            "{text}"
        );
        assert!(text.contains("\n# instance fields\n"));
        assert!(text.contains(".field private count:I\n"));
        assert!(text.contains("\n# direct methods\n"));
        assert!(text.contains(".method public constructor <init>()V\n"));
        assert!(text.contains("    .registers 1\n"));
        assert!(
            text.contains("    invoke-direct {p0}, Ljava/lang/Object;-><init>()V\n"),
            "{text}"
        );
        assert!(text.contains("    return-void\n"));
        assert!(text.ends_with(".end method\n"));
        assert!(!text.contains("TODO"));
        assert!(!text.contains("NOT_IMPLEMENTED"));
        assert_eq!(count_lines(&text, ".method "), 1);
        assert_eq!(count_lines(&text, ".end method"), 1);
    }

    #[test]
    fn p_register_math_and_param_directives() {
        let mut b = Db::new();
        let t_obj = b.t("Ljava/lang/Object;");
        let t_foo = b.t("Lcom/test/Foo;");
        let t_i = b.t("I");
        let t_j = b.t("J");
        let t_v = b.t("V");
        let p_ijv = b.proto(&[t_i, t_j], t_v);
        let m_wide = b.method(t_foo as u16, "wide", p_ijv);
        let f_bar = b.field(t_foo as u16, "bar", t_j as u16);
        // não-estático: p0=this(v1), p1=I(v2), p2=J(v3..v4)
        // iput-wide vA=3 (v3→p2), vB=1 (v1→p0), field → unit0 = 0x5a00 | (1 << 12) | (3 << 8)
        let unit0 = 0x5au16 | (3 << 8) | (1 << 12);
        // debug: 2 params nomeados; prologue; line 5
        let n_a = b.s("a");
        let n_b = b.s("b");
        let mut body = vec![0x07]; // SET_PROLOGUE_END
        body.extend(dbg_sp(0, 4)); // .line 5 @ addr 0 (linha inicial = 1)
        let dbg = b.debug(1, &[n_a as i64, n_b as i64], &body);
        let code = b.code(5, 4, 0, &[unit0, f_bar as u16, 0x000e], &[], &[], Some(dbg));
        let cd = b.class_data(&[], &[], &[(m_wide, 0x1, code)], &[]);
        b.class(t_foo, 0x1, t_obj, None, &[], None, Some(cd), None);

        let dex = Dex::parse(b.finish()).expect("parse fixture");
        let text = render_class(&dex, 0).expect("render");
        assert!(text.contains(".method public wide(IJ)V\n"), "{text}");
        assert!(text.contains("    .registers 5\n"));
        assert!(text.contains("    .param p1, \"a\"    # I\n"), "{text}");
        assert!(text.contains("    .param p2, \"b\"    # J\n"), "{text}");
        assert!(text.contains("    .prologue\n"));
        assert!(text.contains("    .line 5\n"));
        assert!(
            text.contains("    iput-wide p2, p0, Lcom/test/Foo;->bar:J\n"),
            "{text}"
        );
    }

    #[test]
    fn packed_switch_payload_and_labels() {
        let mut b = Db::new();
        let t_obj = b.t("Ljava/lang/Object;");
        let t_foo = b.t("Lcom/test/Foo;");
        let t_v = b.t("V");
        let p_void = b.proto(&[], t_v);
        let m = b.method(t_foo as u16, "ps", p_void);
        // 0: packed-switch v0, +3 → payload @3 (8 units)
        // alvos relativos ao switch (addr 0): +11, +12
        // 11: nop; 12: return-void; 13: return-void
        let mut insns: Vec<u16> = vec![0x002b, 0x0003, 0x0000, 0x0100, 2, 0, 0, 11, 0, 12, 0];
        insns.extend([0x0000, 0x000e, 0x000e]);
        let code = b.code(1, 0, 0, &insns, &[], &[], None);
        let cd = b.class_data(&[], &[], &[(m, 0x9, code)], &[]);
        b.class(t_foo, 0x1, t_obj, None, &[], None, Some(cd), None);

        let dex = Dex::parse(b.finish()).expect("parse fixture");
        let text = render_class(&dex, 0).expect("render");
        assert!(
            text.contains("    packed-switch v0, :pswitch_data_0\n"),
            "{text}"
        );
        assert!(text.contains(":pswitch_data_0\n"));
        assert!(text.contains(".packed-switch 0x0\n"));
        assert!(text.contains("    :pswitch_0\n"));
        assert!(text.contains("    :pswitch_1\n"));
        assert!(text.contains(".end packed-switch\n"));
        // ordem dos alvos = endereços ordenados: 11 < 12
        let pd = text.find(":pswitch_data_0").unwrap();
        let p0 = text.find(":pswitch_0\n").unwrap();
        assert!(pd < p0, "payload antes do fim do método: {text}");
    }

    #[test]
    fn sparse_switch_and_array_data_payloads() {
        let mut b = Db::new();
        let t_obj = b.t("Ljava/lang/Object;");
        let t_foo = b.t("Lcom/test/Foo;");
        let t_v = b.t("V");
        let p_void = b.proto(&[], t_v);
        let m = b.method(t_foo as u16, "sp", p_void);
        // sparse: 0: sparse-switch v0, +3 (3 units) → payload @3 (10 units) keys 5,100 alvos +13/+14
        // array: @13: fill-array-data v0, +3 (3 units) → payload @16 (6 units); @22 nop; @23 return-void
        let mut insns: Vec<u16> = vec![
            0x002c, 0x0003, 0x0000, 0x0200, 2, 5, 0, 100, 0, 13, 0, 14, 0, 0x0026, 0x0003, 0x0000,
            0x0300, 1, 3, 0, 0x0201, 0x0003,
        ];
        insns.extend([0x0000, 0x000e]);
        let code = b.code(1, 0, 0, &insns, &[], &[], None);
        let cd = b.class_data(&[], &[], &[(m, 0x9, code)], &[]);
        b.class(t_foo, 0x1, t_obj, None, &[], None, Some(cd), None);

        let dex = Dex::parse(b.finish()).expect("parse fixture");
        let text = render_class(&dex, 0).expect("render");
        assert!(
            text.contains("    sparse-switch v0, :sswitch_data_0\n"),
            "{text}"
        );
        assert!(text.contains(".sparse-switch\n"));
        assert!(text.contains("    0x5 -> :sswitch_0\n"), "{text}");
        assert!(text.contains("    0x64 -> :sswitch_1\n"), "{text}");
        assert!(text.contains(".end sparse-switch\n"));
        assert!(
            text.contains("    fill-array-data v0, :array_data_0\n"),
            "{text}"
        );
        assert!(text.contains(":array_data_0\n"));
        assert!(text.contains(".array-data 1\n"));
        assert!(text.contains("    0x1t\n"));
        assert!(text.contains("    0x2t\n"));
        assert!(text.contains("    0x3t\n"));
        assert!(text.contains(".end array-data\n"));
    }

    #[test]
    fn try_catch_rendering() {
        let mut b = Db::new();
        let t_obj = b.t("Ljava/lang/Object;");
        let t_foo = b.t("Lcom/test/Foo;");
        let t_exc = b.t("Ljava/lang/Exception;");
        let t_v = b.t("V");
        let p_void = b.proto(&[], t_v);
        let m = b.method(t_foo as u16, "tc", p_void);
        // try [0,2): nop, nop; handler: Exception → 3; catchall → 4 (size -1)
        let mut handler = vec![0x7f]; // sleb(-1): 1 typed + catchall
        handler.extend(uleb(t_exc));
        handler.extend(uleb(3));
        handler.extend(uleb(4));
        let insns = [0x0000u16, 0x0000, 0x000e, 0x000d, 0x000e];
        let code = b.code(1, 0, 0, &insns, &[(0, 2)], &[handler], None);
        let cd = b.class_data(&[], &[], &[(m, 0x9, code)], &[]);
        b.class(t_foo, 0x1, t_obj, None, &[], None, Some(cd), None);

        let dex = Dex::parse(b.finish()).expect("parse fixture");
        let text = render_class(&dex, 0).expect("render");
        assert!(text.contains(":try_start_0\n"), "{text}");
        assert!(text.contains(":try_end_0\n"));
        assert!(
            text.contains(".catch Ljava/lang/Exception; {:try_start_0 .. :try_end_0} :catch_0\n"),
            "{text}"
        );
        assert!(
            text.contains(".catchall {:try_start_0 .. :try_end_0} :catchall_0\n"),
            "{text}"
        );
        assert!(text.contains(":catch_0\n"));
        assert!(text.contains("    move-exception v0\n"));
        // try_end label precede as linhas .catch
        let te = text.find(":try_end_0").unwrap();
        let ca = text.find(".catch").unwrap();
        assert!(te < ca, "{text}");
    }

    #[test]
    fn goto_and_cond_labels() {
        let mut b = Db::new();
        let t_obj = b.t("Ljava/lang/Object;");
        let t_foo = b.t("Lcom/test/Foo;");
        let t_v = b.t("V");
        let p_void = b.proto(&[], t_v);
        let m = b.method(t_foo as u16, "br", p_void);
        // 0: goto +2 → 2; 1: nop; 2-3: if-eqz v0, +2 → 4; 4: nop; 5: return-void
        let insns = [0x0228u16, 0x0000, 0x0038, 0x0002, 0x0000, 0x000e];
        let code = b.code(1, 0, 0, &insns, &[], &[], None);
        let cd = b.class_data(&[], &[], &[(m, 0x9, code)], &[]);
        b.class(t_foo, 0x1, t_obj, None, &[], None, Some(cd), None);

        let dex = Dex::parse(b.finish()).expect("parse fixture");
        let text = render_class(&dex, 0).expect("render");
        assert!(text.contains("    goto :goto_0\n"), "{text}");
        assert!(text.contains(":goto_0\n"));
        assert!(text.contains("    if-eqz v0, :cond_0\n"), "{text}");
        assert!(text.contains(":cond_0\n"));
        // :goto_0 (addr 2) vem antes de :cond_0 (addr 4)
        assert!(text.find(":goto_0").unwrap() < text.find(":cond_0").unwrap());
    }

    #[test]
    fn const_family_literals() {
        let mut b = Db::new();
        let t_obj = b.t("Ljava/lang/Object;");
        let t_foo = b.t("Lcom/test/Foo;");
        let t_v = b.t("V");
        let p_void = b.proto(&[], t_v);
        let m = b.method(t_foo as u16, "c", p_void);
        // const/4 v0, -1 | const/16 v1, 0xff | const-wide v2, 0x123456789abcdef
        // const/high16 v4, 1.0f | const-wide/high16 v5, 100.0
        let insns = [
            0xf012u16, 0x0113, 0x00ff, 0x0218, 0xcdef, 0x89ab, 0x4567, 0x0123, 0x0415, 0x3f80,
            0x0519, 0x4059, 0x000e,
        ];
        let code = b.code(7, 0, 0, &insns, &[], &[], None); // 7 regs: const-wide/high16 v5 ocupa o par (v5,v6)
        let cd = b.class_data(&[], &[], &[(m, 0x9, code)], &[]);
        b.class(t_foo, 0x1, t_obj, None, &[], None, Some(cd), None);

        let dex = Dex::parse(b.finish()).expect("parse fixture");
        let text = render_class(&dex, 0).expect("render");
        assert!(text.contains("    const/4 v0, -0x1\n"), "{text}");
        assert!(text.contains("    const/16 v1, 0xff\n"));
        assert!(
            text.contains("    const-wide v2, 0x123456789abcdefL\n"),
            "{text}"
        );
        assert!(
            text.contains("    const/high16 v4, 0x3f800000    # 1.0f\n"),
            "{text}"
        );
        assert!(
            text.contains("    const-wide/high16 v5, 0x4059000000000000L    # 100.0\n"),
            "{text}"
        );
    }

    #[test]
    fn string_escaping_in_const_string() {
        let mut b = Db::new();
        let t_obj = b.t("Ljava/lang/Object;");
        let t_foo = b.t("Lcom/test/Foo;");
        let t_v = b.t("V");
        let p_void = b.proto(&[], t_v);
        let m = b.method(t_foo as u16, "st", p_void);
        let s = b.s("a\"b\\c\né");
        let insns = [0x001au16, s as u16, 0x000e];
        let code = b.code(1, 0, 0, &insns, &[], &[], None);
        let cd = b.class_data(&[], &[], &[(m, 0x9, code)], &[]);
        b.class(t_foo, 0x1, t_obj, None, &[], None, Some(cd), None);

        let dex = Dex::parse(b.finish()).expect("parse fixture");
        let text = render_class(&dex, 0).expect("render");
        assert!(
            text.contains("    const-string v0, \"a\\\"b\\\\c\\n\\u00e9\"\n"),
            "{text}"
        );
    }

    #[test]
    fn annotation_rendering_class_level() {
        let mut b = Db::new();
        let t_obj = b.t("Ljava/lang/Object;");
        let t_foo = b.t("Lcom/test/Foo;");
        let t_anno = b.t("Lcom/test/Anno;");
        let t_baz = b.t("Lcom/test/Baz;");
        let t_v = b.t("V");
        let n_value = b.s("value");
        let n_name = b.s("name");
        let n_flag = b.s("flag");
        let n_nul = b.s("nul");
        let n_arr = b.s("arr");
        let n_ch = b.s("ch");
        let n_fl = b.s("fl");
        let n_sub = b.s("sub");
        let s_hi = b.s("hi");
        let p_void = b.proto(&[], t_v);
        let m = b.method(t_foo as u16, "x", p_void);
        let code = b.code(1, 0, 0, &[0x000e], &[], &[], None);
        let cd = b.class_data(&[], &[], &[(m, 0x9, code)], &[]);

        // elems em ordem de name_idx (criados sequencialmente)
        let mut elems = Vec::new();
        elems.extend(uleb(n_value));
        elems.extend(ev_int(42));
        elems.extend(uleb(n_name));
        elems.extend(ev_string(s_hi));
        elems.extend(uleb(n_flag));
        elems.extend(ev_bool(true));
        elems.extend(uleb(n_nul));
        elems.extend(ev_null());
        elems.extend(uleb(n_arr));
        elems.extend(ev_array(&[ev_int(1), ev_int(2)]));
        elems.extend(uleb(n_ch));
        elems.extend(ev_char(b'x' as u16));
        elems.extend(uleb(n_fl));
        elems.extend(ev_float(1.5));
        elems.extend(uleb(n_sub));
        elems.extend(ev_subannotation(t_baz, &[(n_value, ev_int(7))]));
        let item = b.annotation(1 /* runtime */, t_anno, 8, &elems);
        let set = b.annotation_set(&[item]);
        let dir = b.annotations_directory(Some(set), &[], &[], &[]);
        b.class(t_foo, 0x1, t_obj, None, &[], Some(dir), Some(cd), None);

        let dex = Dex::parse(b.finish()).expect("parse fixture");
        let text = render_class(&dex, 0).expect("render");
        assert!(text.contains("\n# annotations\n"), "{text}");
        assert!(
            text.contains(".annotation runtime Lcom/test/Anno;\n"),
            "{text}"
        );
        assert!(text.contains("    value = 0x2a\n"), "{text}");
        assert!(text.contains("    name = \"hi\"\n"));
        assert!(text.contains("    flag = true\n"));
        assert!(text.contains("    nul = null\n"));
        assert!(text.contains("    arr = { 0x1, 0x2 }\n"), "{text}");
        assert!(text.contains("    ch = 'x'\n"));
        assert!(text.contains("    fl = 1.5f\n"), "{text}");
        assert!(
            text.contains("    sub = .subannotation Lcom/test/Baz;\n"),
            "{text}"
        );
        assert!(text.contains("        value = 0x7\n"));
        assert!(text.contains("    .end subannotation\n"));
        assert!(text.contains(".end annotation\n"));
    }

    #[test]
    fn interfaces_no_source_and_unused_opcode() {
        let mut b = Db::new();
        let t_obj = b.t("Ljava/lang/Object;");
        let t_foo = b.t("Lcom/test/Foo;");
        let t_run = b.t("Ljava/lang/Runnable;");
        let t_close = b.t("Ljava/io/Closeable;");
        let t_v = b.t("V");
        let p_void = b.proto(&[], t_v);
        let m = b.method(t_foo as u16, "u", p_void);
        // 0x3e é opcode unused
        let code = b.code(1, 0, 0, &[0x003e, 0x000e], &[], &[], None);
        let cd = b.class_data(&[], &[], &[(m, 0x9, code)], &[]);
        b.class(
            t_foo,
            0x1 | 0x200,
            t_obj,
            None,
            &[t_run, t_close],
            None,
            Some(cd),
            None,
        );

        let dex = Dex::parse(b.finish()).expect("parse fixture");
        let text = render_class(&dex, 0).expect("render");
        assert!(!text.contains(".source"), "sem source: {text}");
        assert!(text.contains(".implements Ljava/lang/Runnable;\n"));
        assert!(text.contains(".implements Ljava/io/Closeable;\n"));
        assert!(text.contains("    nop // unused 0x3e\n"), "{text}");
        assert!(
            text.contains(".class public interface abstract") || text.contains(".class public ")
        );
    }

    #[test]
    fn render_method_extraction_and_typed_errors() {
        let mut b = Db::new();
        let t_obj = b.t("Ljava/lang/Object;");
        let t_foo = b.t("Lcom/test/Foo;");
        let t_v = b.t("V");
        let p_void = b.proto(&[], t_v);
        let m = b.method(t_foo as u16, "only", p_void);
        let code = b.code(1, 0, 0, &[0x000e], &[], &[], None);
        let cd = b.class_data(&[], &[], &[(m, 0x1, code)], &[]);
        b.class(t_foo, 0x1, t_obj, None, &[], None, Some(cd), None);

        let dex = Dex::parse(b.finish()).expect("parse fixture");
        let block = render_method(&dex, 0, "only").expect("render_method");
        assert!(block.starts_with(".method public only()V\n"));
        assert!(block.ends_with(".end method\n"));

        let e = render_method(&dex, 0, "missing").unwrap_err();
        assert_eq!(e.code, "MISSING_ENTRY");
        let e = render_class(&dex, 9).unwrap_err();
        assert_eq!(e.code, "MISSING_ENTRY");
    }

    #[test]
    fn smali_file_mapping() {
        assert_eq!(smali_file_name("Lcom/x/Y;"), "Lcom/x/Y.smali");
        assert_eq!(smali_file_name("LFoo;"), "LFoo.smali");
    }

    #[test]
    fn local_directives_from_debug_events() {
        let mut b = Db::new();
        let t_obj = b.t("Ljava/lang/Object;");
        let t_foo = b.t("Lcom/test/Foo;");
        let t_i = b.t("I");
        let t_v = b.t("V");
        let p_iv = b.proto(&[t_i], t_v);
        let m = b.method(t_foo as u16, "loc", p_iv);
        let n_x = b.s("x");
        // entry @0: .line 1 + start local v1 (p1) "x":I; entry @1: .line 3 + end local
        let mut body = dbg_sp(0, 0); // posiciona .line 1 @ addr 0
        body.extend(dbg_start_local(1, n_x as i64, t_i as i64));
        body.extend(dbg_sp(1, 2)); // addr+1, line+2 → linha 3 @ addr 1
        body.extend([0x05]); // END_LOCAL reg 1
        body.extend(uleb(1));
        let dbg = b.debug(1, &[n_x as i64], &body);
        let code = b.code(2, 2, 0, &[0x0000, 0x0000, 0x000e], &[], &[], Some(dbg));
        let cd = b.class_data(&[], &[], &[(m, 0x1, code)], &[]);
        b.class(t_foo, 0x1, t_obj, None, &[], None, Some(cd), None);

        let dex = Dex::parse(b.finish()).expect("parse fixture");
        let text = render_class(&dex, 0).expect("render");
        assert!(text.contains("    .local p1, \"x\":I\n"), "{text}");
        assert!(text.contains("    .end local p1\n"), "{text}");
        assert!(text.contains("    .line 1\n"), "{text}");
        assert!(text.contains("    .line 3\n"), "{text}");
        // dedup de .line: linha repetida não é reimpressa
        let n_line1 = text.matches("    .line 1\n").count();
        assert_eq!(n_line1, 1, "{text}");
    }

    #[test]
    fn roundtrip_structural_invariants() {
        let mut b = Db::new();
        let t_obj = b.t("Ljava/lang/Object;");
        let t_foo = b.t("Lcom/test/Foo;");
        let t_v = b.t("V");
        let p_void = b.proto(&[], t_v);
        let m1 = b.method(t_foo as u16, "<init>", p_void);
        let m2 = b.method(t_foo as u16, "a", p_void);
        let m3 = b.method(t_foo as u16, "b", p_void);
        let c1 = b.code(1, 1, 0, &[0x1070, 0, 0x0000, 0x000e], &[], &[], None);
        let c2 = b.code(1, 0, 0, &[0x000e], &[], &[], None);
        let c3 = b.code(1, 0, 0, &[0x000e], &[], &[], None);
        let cd = b.class_data(
            &[],
            &[],
            &[(m1, 0x1 | 0x1_0000, c1)],
            &[(m2, 0x1, c2), (m3, 0x4, c3)],
        );
        b.class(t_foo, 0x1, t_obj, None, &[], None, Some(cd), None);

        let dex = Dex::parse(b.finish()).expect("parse fixture");
        let text = render_class(&dex, 0).expect("render");
        assert!(text.starts_with(".class "), "começa com .class");
        assert_eq!(count_lines(&text, ".method "), 3);
        assert_eq!(text.matches(".end method\n").count(), 3);
        assert!(!text.contains("TODO"));
        assert!(!text.contains("NOT_IMPLEMENTED"));
        for line in text.lines() {
            if line.contains("unused") {
                assert!(
                    line.trim_start().starts_with("nop // unused 0x"),
                    "unused só em opcodes realmente indefinidos: {line}"
                );
            }
        }
    }
}
