//! DexBuilder + helpers de engine compartilhados pelos testes do rd-vm e do
//! rd-cli: DEXs sintéticos construídos byte a byte (mesma política dos
//! fixtures do rd-dex) — nenhum fixture binário commitado.
#![allow(dead_code)]

use rd_vm::engine::{Engine, VmConfig};
use rd_vm::err::VmExit;
use rd_vm::value::Value;

// ── packing de code units (formatos oficiais do dex-format) ─────────────────

pub fn op10x(op: u8) -> Vec<u16> {
    vec![op as u16]
}
pub fn op12x(op: u8, a: u8, b: u8) -> Vec<u16> {
    vec![(op as u16) | ((a as u16) << 8) | ((b as u16) << 12)]
}
pub fn op11n(op: u8, a: u8, lit: i8) -> Vec<u16> {
    vec![(op as u16) | ((a as u16) << 8) | (((lit as u8) as u16) << 12)]
}
pub fn op11x(op: u8, a: u8) -> Vec<u16> {
    vec![(op as u16) | ((a as u16) << 8)]
}
pub fn op10t(op: u8, off: i8) -> Vec<u16> {
    vec![(op as u16) | (((off as u8) as u16) << 8)]
}
pub fn op21c(op: u8, a: u8, idx: u16) -> Vec<u16> {
    vec![(op as u16) | ((a as u16) << 8), idx]
}
pub fn op21s(op: u8, a: u8, lit: i16) -> Vec<u16> {
    vec![(op as u16) | ((a as u16) << 8), lit as u16]
}
pub fn op22b(op: u8, a: u8, b: u8, lit: i8) -> Vec<u16> {
    vec![
        (op as u16) | ((a as u16) << 8),
        (b as u16) | (((lit as u8) as u16) << 8),
    ]
}
pub fn op22s(op: u8, a: u8, b: u8, lit: i16) -> Vec<u16> {
    vec![
        (op as u16) | ((a as u16) << 8) | ((b as u16) << 12),
        lit as u16,
    ]
}
pub fn op22c(op: u8, a: u8, b: u8, idx: u16) -> Vec<u16> {
    vec![(op as u16) | ((a as u16) << 8) | ((b as u16) << 12), idx]
}
pub fn op22t(op: u8, a: u8, b: u8, off: i16) -> Vec<u16> {
    vec![
        (op as u16) | ((a as u16) << 8) | ((b as u16) << 12),
        off as u16,
    ]
}
pub fn op23x(op: u8, a: u8, b: u8, c: u8) -> Vec<u16> {
    vec![
        (op as u16) | ((a as u16) << 8),
        (b as u16) | ((c as u16) << 8),
    ]
}
pub fn op31t(op: u8, a: u8, off: i32) -> Vec<u16> {
    vec![
        (op as u16) | ((a as u16) << 8),
        off as u16,
        (off >> 16) as u16,
    ]
}
pub fn op51l(op: u8, a: u8, lit: i64) -> Vec<u16> {
    vec![
        (op as u16) | ((a as u16) << 8),
        lit as u16,
        (lit >> 16) as u16,
        (lit >> 32) as u16,
        (lit >> 48) as u16,
    ]
}
/// 35c: A (count) no nibble ALTO de byte1, G no baixo; regs {C D E F}
pub fn op35c(op: u8, count: u8, idx: u16, regs: [u8; 5]) -> Vec<u16> {
    vec![
        (op as u16) | ((count as u16) << 12) | ((regs[4] as u16) << 8),
        idx,
        ((regs[3] as u16) << 12)
            | ((regs[2] as u16) << 8)
            | ((regs[1] as u16) << 4)
            | regs[0] as u16,
    ]
}
pub fn packed_payload(first_key: i32, targets: &[i32]) -> Vec<u16> {
    let mut v = vec![
        0x0100u16,
        targets.len() as u16,
        first_key as u16,
        (first_key >> 16) as u16,
    ];
    for t in targets {
        v.push(*t as u16);
        v.push((t >> 16) as u16);
    }
    v
}

// ── builder de DEX ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub enum SVal {
    Int(i32),
}

pub struct CodeBlob {
    pub regs: u16,
    pub ins: u16,
    pub outs: u16,
    pub units: Vec<u16>,
    pub tries: Vec<TryBlob>,
}

pub struct TryBlob {
    pub start: u32,
    pub count: u16,
    pub typed: Vec<(String, u32)>,
    pub catch_all: Option<u32>,
}

pub struct MethodSlot {
    pub name: String,
    pub proto_idx: usize,
    pub access: u32,
    pub code: Option<CodeBlob>,
}

pub struct ProtoE {
    pub shorty: String,
    pub ret: String,
    pub params: Vec<String>,
}

pub struct ClassE {
    pub this: String,
    pub sup: String,
    pub statics: Vec<(String, String)>,
    pub static_values: Vec<SVal>,
    pub instances: Vec<(String, String)>,
    pub directs: Vec<MethodSlot>,
    pub virtuals: Vec<MethodSlot>,
}

#[derive(Default)]
pub struct DexBuilder {
    pub strings: Vec<String>,
    pub types: Vec<String>,
    pub protos: Vec<ProtoE>,
    pub fields: Vec<(String, String, String)>,
    pub methods: Vec<(String, usize, String)>,
    pub classes: Vec<ClassE>,
}

impl DexBuilder {
    pub fn new() -> Self {
        let mut b = DexBuilder::default();
        b.proto_idx("V", vec![]); // slot 0 = ()V
        b
    }

    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(i) = self.strings.iter().position(|x| x == s) {
            return i as u32;
        }
        self.strings.push(s.to_string());
        (self.strings.len() - 1) as u32
    }

    pub fn type_idx(&mut self, desc: &str) -> u16 {
        if let Some(i) = self.types.iter().position(|x| x == desc) {
            return i as u16;
        }
        self.intern(desc);
        self.types.push(desc.to_string());
        (self.types.len() - 1) as u16
    }

    pub fn proto_idx(&mut self, ret: &str, params: Vec<String>) -> usize {
        self.type_idx(ret);
        for p in &params {
            self.type_idx(p);
        }
        if let Some(i) = self
            .protos
            .iter()
            .position(|p| p.ret == ret && p.params == params)
        {
            return i;
        }
        self.intern(&shorty_of(ret, &params));
        self.protos.push(ProtoE {
            shorty: shorty_of(ret, &params),
            ret: ret.to_string(),
            params: params.clone(),
        });
        self.protos.len() - 1
    }

    pub fn method_idx(&mut self, class: &str, proto: usize, name: &str) -> u16 {
        self.type_idx(class);
        if let Some(i) = self
            .methods
            .iter()
            .position(|(c, p, n)| c == class && *p == proto && n == name)
        {
            return i as u16;
        }
        self.intern(name);
        self.methods
            .push((class.to_string(), proto, name.to_string()));
        (self.methods.len() - 1) as u16
    }

    pub fn field_idx(&mut self, class: &str, ftype: &str, name: &str) -> u16 {
        self.type_idx(class);
        self.type_idx(ftype);
        if let Some(i) = self
            .fields
            .iter()
            .position(|(c, t, n)| c == class && t == ftype && n == name)
        {
            return i as u16;
        }
        self.intern(name);
        self.fields
            .push((class.to_string(), ftype.to_string(), name.to_string()));
        (self.fields.len() - 1) as u16
    }

    pub fn class(&mut self, this: &str, sup: &str) -> usize {
        self.type_idx(this);
        self.type_idx(sup);
        self.classes.push(ClassE {
            this: this.to_string(),
            sup: sup.to_string(),
            statics: Vec::new(),
            static_values: Vec::new(),
            instances: Vec::new(),
            directs: Vec::new(),
            virtuals: Vec::new(),
        });
        self.classes.len() - 1
    }

    pub fn static_field(&mut self, cls: usize, name: &str, ftype: &str, init: SVal) {
        self.intern(name);
        self.type_idx(ftype);
        self.classes[cls]
            .statics
            .push((name.to_string(), ftype.to_string()));
        self.classes[cls].static_values.push(init);
    }

    pub fn instance_field(&mut self, cls: usize, name: &str, ftype: &str) {
        self.intern(name);
        self.type_idx(ftype);
        self.classes[cls]
            .instances
            .push((name.to_string(), ftype.to_string()));
    }

    pub fn code(&self, regs: u16, ins: u16, outs: u16, units: Vec<u16>) -> CodeBlob {
        CodeBlob {
            regs,
            ins,
            outs,
            units,
            tries: Vec::new(),
        }
    }

    pub fn direct(
        &mut self,
        cls: usize,
        name: &str,
        ret: &str,
        params: Vec<&str>,
        access: u32,
        code: Option<CodeBlob>,
    ) {
        let proto_idx = self.proto_idx(ret, params.iter().map(|s| s.to_string()).collect());
        let this = self.classes[cls].this.clone();
        self.method_idx(&this, proto_idx, name);
        self.classes[cls].directs.push(MethodSlot {
            name: name.to_string(),
            proto_idx,
            access,
            code,
        });
    }

    #[allow(named_arguments_used_positionally)]
    pub fn r#virtual(
        &mut self,
        cls: usize,
        name: &str,
        ret: &str,
        params: Vec<&str>,
        access: u32,
        code: Option<CodeBlob>,
    ) {
        let proto_idx = self.proto_idx(ret, params.iter().map(|s| s.to_string()).collect());
        let this = self.classes[cls].this.clone();
        self.method_idx(&this, proto_idx, name);
        self.classes[cls].virtuals.push(MethodSlot {
            name: name.to_string(),
            proto_idx,
            access,
            code,
        });
    }

    pub fn finish(&self) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&[0u8; 112]); // header placeholder

        let string_ids_off = out.len() as u32;
        for _ in &self.strings {
            out.extend_from_slice(&0u32.to_le_bytes());
        }
        let type_ids_off = out.len() as u32;
        for _ in &self.types {
            out.extend_from_slice(&0u32.to_le_bytes());
        }
        let proto_ids_off = out.len() as u32;
        for _ in &self.protos {
            out.extend_from_slice(&[0u8; 12]);
        }
        let field_ids_off = out.len() as u32;
        for _ in &self.fields {
            out.extend_from_slice(&[0u8; 8]);
        }
        let method_ids_off = out.len() as u32;
        for _ in &self.methods {
            out.extend_from_slice(&[0u8; 8]);
        }
        let class_defs_off = out.len() as u32;
        for _ in &self.classes {
            out.extend_from_slice(&[0u8; 32]);
        }
        let data_off = out.len() as u32;

        // string_data
        let mut string_data_offsets = vec![0u32; self.strings.len()];
        for (i, s) in self.strings.iter().enumerate() {
            align4(&mut out);
            string_data_offsets[i] = out.len() as u32;
            write_uleb(&mut out, s.encode_utf16().count() as u64);
            // DEX string_data is modified UTF-8 over UTF-16 code units.
            // NUL is C0 80; a supplementary scalar is two three-byte units.
            for unit in s.encode_utf16() {
                match unit {
                    1..=0x7f => out.push(unit as u8),
                    0..=0x7ff => out
                        .extend_from_slice(&[0xc0 | (unit >> 6) as u8, 0x80 | (unit & 0x3f) as u8]),
                    _ => out.extend_from_slice(&[
                        0xe0 | (unit >> 12) as u8,
                        0x80 | ((unit >> 6) & 0x3f) as u8,
                        0x80 | (unit & 0x3f) as u8,
                    ]),
                }
            }
            out.push(0);
        }

        // type_lists
        let mut type_list_offs: Vec<u32> = vec![0; self.protos.len()];
        for (i, p) in self.protos.iter().enumerate() {
            if p.params.is_empty() {
                continue;
            }
            align4(&mut out);
            type_list_offs[i] = out.len() as u32;
            out.extend_from_slice(&(p.params.len() as u32).to_le_bytes());
            for t in &p.params {
                let idx = self
                    .types
                    .iter()
                    .position(|x| x == t)
                    .expect("tipo do proto");
                out.extend_from_slice(&(idx as u16).to_le_bytes());
            }
        }

        // code items
        let mut method_code_offs: Vec<Option<u32>> = vec![None; self.methods.len()];
        for c in &self.classes {
            for slot in c.directs.iter().chain(&c.virtuals) {
                let Some(blob) = &slot.code else { continue };
                align4(&mut out);
                let midx = self
                    .methods
                    .iter()
                    .position(|(cl, p, n)| cl == &c.this && p == &slot.proto_idx && n == &slot.name)
                    .expect("método registrado");
                method_code_offs[midx] = Some(out.len() as u32);
                out.extend_from_slice(&blob.regs.to_le_bytes());
                out.extend_from_slice(&blob.ins.to_le_bytes());
                out.extend_from_slice(&blob.outs.to_le_bytes());
                out.extend_from_slice(&(blob.tries.len() as u16).to_le_bytes());
                out.extend_from_slice(&0u32.to_le_bytes()); // debug_info
                out.extend_from_slice(&(blob.units.len() as u32).to_le_bytes());
                for u in &blob.units {
                    out.extend_from_slice(&u.to_le_bytes());
                }
                if !blob.tries.is_empty() {
                    if blob.units.len() % 2 == 1 {
                        out.extend_from_slice(&0u16.to_le_bytes()); // padding
                    }
                    let mut list_bytes: Vec<u8> = Vec::new();
                    write_uleb(&mut list_bytes, blob.tries.len() as u64);
                    for t in &blob.tries {
                        out.extend_from_slice(&t.start.to_le_bytes());
                        out.extend_from_slice(&t.count.to_le_bytes());
                        // handler_off relativo ao INÍCIO da lista (inclui size)
                        out.extend_from_slice(&(list_bytes.len() as u16).to_le_bytes());
                        let n = t.typed.len() as i64;
                        write_sleb(&mut list_bytes, if t.catch_all.is_some() { -n } else { n });
                        for (tdesc, addr) in &t.typed {
                            let tidx = self
                                .types
                                .iter()
                                .position(|x| x == tdesc)
                                .expect("tipo do try registrado")
                                as u16;
                            write_uleb(&mut list_bytes, tidx as u64);
                            write_uleb(&mut list_bytes, *addr as u64);
                        }
                        if let Some(ca) = t.catch_all {
                            write_uleb(&mut list_bytes, ca as u64);
                        }
                    }
                    out.extend_from_slice(&list_bytes);
                }
            }
        }

        // class_data + static_values
        let mut class_data_offs = vec![0u32; self.classes.len()];
        let mut static_values_offs = vec![0u32; self.classes.len()];
        for (ci, c) in self.classes.iter().enumerate() {
            let has_members = !c.statics.is_empty()
                || !c.instances.is_empty()
                || !c.directs.is_empty()
                || !c.virtuals.is_empty();
            if has_members {
                align4(&mut out);
                class_data_offs[ci] = out.len() as u32;
                write_uleb(&mut out, c.statics.len() as u64);
                write_uleb(&mut out, c.instances.len() as u64);
                write_uleb(&mut out, c.directs.len() as u64);
                write_uleb(&mut out, c.virtuals.len() as u64);
                let mut prev: u32 = 0;
                for (name, ftype) in &c.statics {
                    let fidx = self.find_field(&c.this, ftype, name) as u32;
                    write_uleb(&mut out, (fidx - prev) as u64);
                    write_uleb(&mut out, 0x0009); // public static
                    prev = fidx;
                }
                let mut prev: u32 = 0;
                for (name, ftype) in &c.instances {
                    let fidx = self.find_field(&c.this, ftype, name) as u32;
                    write_uleb(&mut out, (fidx - prev) as u64);
                    write_uleb(&mut out, 0);
                    prev = fidx;
                }
                let mut prev: u32 = 0;
                for slot in &c.directs {
                    let midx = self.find_method(&c.this, slot.proto_idx, &slot.name) as u32;
                    let code_off = method_code_offs[midx as usize].unwrap_or(0);
                    write_uleb(&mut out, (midx - prev) as u64);
                    write_uleb(&mut out, slot.access as u64);
                    write_uleb(&mut out, code_off as u64);
                    prev = midx;
                }
                let mut prev: u32 = 0;
                for slot in &c.virtuals {
                    let midx = self.find_method(&c.this, slot.proto_idx, &slot.name) as u32;
                    let code_off = method_code_offs[midx as usize].unwrap_or(0);
                    write_uleb(&mut out, (midx - prev) as u64);
                    write_uleb(&mut out, slot.access as u64);
                    write_uleb(&mut out, code_off as u64);
                    prev = midx;
                }
            }
            if !c.static_values.is_empty() {
                align4(&mut out);
                static_values_offs[ci] = out.len() as u32;
                write_uleb(&mut out, c.static_values.len() as u64);
                for v in &c.static_values {
                    match v {
                        SVal::Int(i) => {
                            out.push(0x04 | (3 << 5));
                            out.extend_from_slice(&i.to_le_bytes());
                        }
                    }
                }
            }
        }

        // map_list (por último)
        align4(&mut out);
        let map_off = out.len() as u32;
        let mut map: Vec<(u16, u32, u32)> = vec![
            (0x0000, 1, 0),
            (0x0001, self.strings.len() as u32, string_ids_off),
            (0x0002, self.types.len() as u32, type_ids_off),
            (0x0003, self.protos.len() as u32, proto_ids_off),
        ];
        #[allow(clippy::vec_init_then_push)]
        {
            if !self.fields.is_empty() {
                map.push((0x0004, self.fields.len() as u32, field_ids_off));
            }
            if !self.methods.is_empty() {
                map.push((0x0005, self.methods.len() as u32, method_ids_off));
            }
            if !self.classes.is_empty() {
                map.push((0x0006, self.classes.len() as u32, class_defs_off));
            }
            if !self.strings.is_empty() {
                map.push((0x2002, self.strings.len() as u32, string_data_offsets[0]));
            }
            let n_tlists = type_list_offs.iter().filter(|o| **o != 0).count() as u32;
            if n_tlists > 0 {
                map.push((
                    0x1001,
                    n_tlists,
                    *type_list_offs.iter().find(|o| **o != 0).unwrap(),
                ));
            }
            let n_cdata = class_data_offs.iter().filter(|o| **o != 0).count() as u32;
            if n_cdata > 0 {
                map.push((
                    0x2000,
                    n_cdata,
                    *class_data_offs.iter().find(|o| **o != 0).unwrap(),
                ));
            }
            let n_code = method_code_offs.iter().filter(|o| o.is_some()).count() as u32;
            if n_code > 0 {
                map.push((
                    0x2001,
                    n_code,
                    method_code_offs
                        .iter()
                        .find(|o| o.is_some())
                        .unwrap()
                        .unwrap(),
                ));
            }
            let n_svals = static_values_offs.iter().filter(|o| **o != 0).count() as u32;
            if n_svals > 0 {
                map.push((
                    0x2005,
                    n_svals,
                    *static_values_offs.iter().find(|o| **o != 0).unwrap(),
                ));
            }
            map.push((0x1000, 1, map_off));
        }

        out.extend_from_slice(&(map.len() as u32).to_le_bytes());
        for (t, size, off) in &map {
            out.extend_from_slice(&t.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&off.to_le_bytes());
        }

        // backpatch das tabelas
        for (i, s) in string_data_offsets.iter().enumerate() {
            let at = string_ids_off as usize + i * 4;
            out[at..at + 4].copy_from_slice(&s.to_le_bytes());
        }
        for (i, t) in self.types.iter().enumerate() {
            let sidx = self
                .strings
                .iter()
                .position(|x| x == t)
                .expect("tipo tem string");
            let at = type_ids_off as usize + i * 4;
            out[at..at + 4].copy_from_slice(&(sidx as u32).to_le_bytes());
        }
        for (i, p) in self.protos.iter().enumerate() {
            let base = proto_ids_off as usize + i * 12;
            let sidx = self
                .strings
                .iter()
                .position(|x| x == &p.shorty)
                .expect("shorty");
            out[base..base + 4].copy_from_slice(&(sidx as u32).to_le_bytes());
            let ridx = self.types.iter().position(|x| x == &p.ret).expect("ret");
            out[base + 4..base + 8].copy_from_slice(&(ridx as u32).to_le_bytes());
            out[base + 8..base + 12].copy_from_slice(&type_list_offs[i].to_le_bytes());
        }
        for (i, (cl, t, n)) in self.fields.iter().enumerate() {
            let base = field_ids_off as usize + i * 8;
            out[base..base + 2].copy_from_slice(&self.type_idx_cached(cl).to_le_bytes());
            out[base + 2..base + 4].copy_from_slice(&self.type_idx_cached(t).to_le_bytes());
            let at = base + 4;
            out[at..at + 4].copy_from_slice(&self.string_idx_cached(n).to_le_bytes());
        }
        for (i, (cl, p, n)) in self.methods.iter().enumerate() {
            let base = method_ids_off as usize + i * 8;
            out[base..base + 2].copy_from_slice(&self.type_idx_cached(cl).to_le_bytes());
            out[base + 2..base + 4].copy_from_slice(&(*p as u16).to_le_bytes());
            let at = base + 4;
            out[at..at + 4].copy_from_slice(&self.string_idx_cached(n).to_le_bytes());
        }
        for (ci, c) in self.classes.iter().enumerate() {
            let base = class_defs_off as usize + ci * 32;
            out[base..base + 4]
                .copy_from_slice(&(self.type_idx_cached(&c.this) as u32).to_le_bytes());
            // layout real do class_def_item: class_idx@0, access_flags@4,
            // superclass_idx@8 (o fix antigo escrevia sup em access_flags —
            // super era lida como o primeiro tipo do DEX, "V")
            out[base + 4..base + 8].copy_from_slice(&0x1u32.to_le_bytes()); // ACC_PUBLIC
            out[base + 8..base + 12]
                .copy_from_slice(&(self.type_idx_cached(&c.sup) as u32).to_le_bytes());
            // interfaces_off @12, source_file_idx @16, annotations_off @20 → 0
            let at = base + 24;
            out[at..at + 4].copy_from_slice(&class_data_offs[ci].to_le_bytes());
            let at = base + 28;
            out[at..at + 4].copy_from_slice(&static_values_offs[ci].to_le_bytes());
        }

        // header
        let file_size = out.len() as u32;
        let patch = |out: &mut Vec<u8>, at: usize, v: u32| {
            out[at..at + 4].copy_from_slice(&v.to_le_bytes());
        };
        patch(&mut out, 32, file_size);
        patch(&mut out, 36, 112);
        out[40..44].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        patch(&mut out, 52, map_off);
        patch(&mut out, 56, self.strings.len() as u32);
        patch(&mut out, 60, string_ids_off);
        patch(&mut out, 64, self.types.len() as u32);
        patch(&mut out, 68, type_ids_off);
        patch(&mut out, 72, self.protos.len() as u32);
        patch(&mut out, 76, proto_ids_off);
        patch(&mut out, 80, self.fields.len() as u32);
        patch(&mut out, 84, field_ids_off);
        patch(&mut out, 88, self.methods.len() as u32);
        patch(&mut out, 92, method_ids_off);
        patch(&mut out, 96, self.classes.len() as u32);
        patch(&mut out, 100, class_defs_off);
        patch(&mut out, 104, file_size - data_off);
        patch(&mut out, 108, data_off);
        let adler = adler32(&out[12..]);
        patch(&mut out, 8, adler);
        out[0..8].copy_from_slice(b"dex\n035\x00");
        out
    }

    pub fn find_field(&self, class: &str, ftype: &str, name: &str) -> usize {
        self.fields
            .iter()
            .position(|(c, t, n)| c == class && t == ftype && n == name)
            .expect("campo registrado")
    }

    pub fn find_method(&self, class: &str, proto: usize, name: &str) -> usize {
        self.methods
            .iter()
            .position(|(c, p, n)| c == class && *p == proto && n == name)
            .expect("método registrado")
    }

    pub fn type_idx_cached(&self, desc: &str) -> u16 {
        self.types
            .iter()
            .position(|x| x == desc)
            .expect("tipo registrado") as u16
    }

    pub fn string_idx_cached(&self, s: &str) -> u32 {
        self.strings
            .iter()
            .position(|x| x == s)
            .expect("string registrada") as u32
    }
}

pub fn align4(out: &mut Vec<u8>) {
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

pub fn write_uleb(out: &mut Vec<u8>, mut v: u64) {
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

pub fn write_sleb(out: &mut Vec<u8>, mut v: i64) {
    loop {
        if v == 0 || v == -1 {
            out.push((v & 0x7F) as u8);
            break;
        }
        out.push(((v & 0x7F) as u8) | 0x80);
        v >>= 7;
    }
}

pub fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

pub fn shorty_of(ret: &str, params: &[String]) -> String {
    let ch = |d: &str| match d {
        "V" => 'V',
        "Z" => 'Z',
        "B" => 'B',
        "S" => 'S',
        "C" => 'C',
        "I" => 'I',
        "J" => 'J',
        "F" => 'F',
        "D" => 'D',
        _ => 'L',
    };
    let mut s = String::new();
    s.push(ch(ret));
    for p in params {
        s.push(ch(p));
    }
    s
}

// ── helpers de execução ─────────────────────────────────────────────────────

pub fn engine_of(b: &DexBuilder) -> Engine {
    let dex = rd_dex::Dex::parse(b.finish()).expect("DEX sintético parseia");
    Engine::new(vec![dex], VmConfig::default())
}

pub const ACC_PUBLIC: u32 = 0x1;
pub const ACC_STATIC: u32 = 0x8;
const ACC_CONSTRUCTOR: u32 = 0x1_0000;

pub fn invoke(eng: &mut Engine, method: &str, sig: &str, args: &[Value]) -> Result<Value, VmExit> {
    eng.invoke_static("LCaso;", method, sig, args)
}

// ── testes ──────────────────────────────────────────────────────────────────
// CONVENÇÃO ABI: parâmetros caem nos ÚLTIMOS `ins` registradores
// (regs=5, ins=2 → args em v3,v4). Fixtures seguem isso à risca.

#[cfg(test)]
mod string_data_tests {
    use super::*;
    #[test]
    fn dex_strings_encode_nul_bmp_and_surrogate_pairs_as_modified_utf8() {
        let mut b = DexBuilder::new();
        let text = "ASCII\0é\u{1f642}";
        let index = b.intern(text);
        let bytes = b.finish();
        assert!(bytes.windows(2).any(|v| v == [0xc0, 0x80]));
        assert!(bytes
            .windows(6)
            .any(|v| v == [0xed, 0xa0, 0xbd, 0xed, 0xb9, 0x82]));
        assert_eq!(rd_dex::Dex::parse(bytes).unwrap().string(index), text);
    }
}
