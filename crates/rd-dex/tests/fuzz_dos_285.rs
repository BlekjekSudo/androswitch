//! Regression suite for Issue #285 (weekly fuzz timeout #273):
//! algorithmic amplification via aliased/overlapping offsets in `Strings::parse`,
//! `CodeItem::parse`, and `disasm::render_class`.

use std::time::Instant;

use rd_dex::code::CodeItem;
use rd_dex::disasm;
use rd_dex::strings::{Strings, NO_INDEX};
use rd_dex::Dex;

const SENTINEL_OFF: u32 = u32::MAX;

fn push_uleb(mut v: u32, out: &mut Vec<u8>) {
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

fn push_sleb(mut v: i32, out: &mut Vec<u8>) {
    loop {
        let byte = (v as u8) & 0x7F;
        v >>= 7;
        let sign_ext = (byte & 0x40) != 0;
        if (v == 0 && !sign_ext) || (v == -1 && sign_ext) {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

struct SyntheticDex {
    buf: Vec<u8>,
    patches: Vec<(usize, usize)>,
    uleb_patches: Vec<(usize, u32)>,
    strings: Vec<String>,
    str_offsets: Vec<u32>,
    types: Vec<u32>,
    protos: Vec<(u32, u32, u32)>,
    fields: Vec<(u16, u16, u32)>,
    methods: Vec<(u16, u16, u32)>,
    classes: Vec<[u32; 8]>,
}

impl SyntheticDex {
    fn new() -> Self {
        Self {
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

    fn uleb(&mut self, v: u32) {
        push_uleb(v, &mut self.buf);
    }

    fn uleb5_patch(&mut self, rel_off: u32) {
        let pos = self.buf.len();
        for i in 0..4 {
            self.buf.push((((rel_off >> (7 * i)) & 0x7F) | 0x80) as u8);
        }
        self.buf.push(((rel_off >> 28) & 0x0F) as u8);
        self.uleb_patches.push((pos, rel_off));
    }

    fn push(&mut self, bytes: &[u8]) -> u32 {
        let pos = self.buf.len() as u32;
        self.buf.extend_from_slice(bytes);
        pos
    }

    fn align4(&mut self) {
        while self.buf.len() % 4 != 0 {
            self.buf.push(0);
        }
    }

    fn str_id(&mut self, s: &str) -> u32 {
        if let Some(i) = self.strings.iter().position(|x| x == s) {
            return i as u32;
        }
        let mut hdr = Vec::new();
        push_uleb(s.encode_utf16().count() as u32, &mut hdr);
        let off = self.push(&hdr);
        self.push(s.as_bytes());
        self.push(&[0]);
        self.strings.push(s.to_string());
        self.str_offsets.push(off);
        (self.strings.len() - 1) as u32
    }

    fn type_id(&mut self, desc: &str) -> u32 {
        let si = self.str_id(desc);
        if let Some(i) = self.types.iter().position(|&x| x == si) {
            return i as u32;
        }
        self.types.push(si);
        (self.types.len() - 1) as u32
    }

    fn shorty_char(&self, type_idx: u32) -> char {
        let si = self.types[type_idx as usize];
        self.strings[si as usize].chars().next().unwrap_or('L')
    }

    fn proto_id(&mut self, params: &[u32], ret: u32) -> u16 {
        let mut shorty = String::new();
        shorty.push(self.shorty_char(ret));
        for &p in params {
            shorty.push(self.shorty_char(p));
        }
        let si = self.str_id(&shorty);
        let poff = if params.is_empty() {
            SENTINEL_OFF
        } else {
            self.align4();
            let mut tl = Vec::new();
            tl.extend_from_slice(&(params.len() as u32).to_le_bytes());
            for &p in params {
                tl.extend_from_slice(&(p as u16).to_le_bytes());
            }
            self.push(&tl)
        };
        self.protos.push((si, ret, poff));
        (self.protos.len() - 1) as u16
    }

    fn method_id(&mut self, class: u16, name: &str, proto: u16) -> u32 {
        let ni = self.str_id(name);
        self.methods.push((class, proto, ni));
        (self.methods.len() - 1) as u32
    }

    fn patch_u32(&mut self, target_rel: Option<u32>) {
        let pos = self.buf.len();
        self.buf.extend_from_slice(&0u32.to_le_bytes());
        if let Some(t) = target_rel {
            self.patches.push((pos, t as usize));
        }
    }

    fn annotation_item(&mut self, vis: u8, type_idx: u32, count: u32, elems: &[u8]) -> u32 {
        let off = self.buf.len() as u32;
        self.buf.push(vis);
        self.uleb(type_idx);
        self.uleb(count);
        self.push(elems);
        off
    }

    fn annotation_set(&mut self, items: &[u32]) -> u32 {
        self.align4();
        let off = self.buf.len() as u32;
        self.buf
            .extend_from_slice(&(items.len() as u32).to_le_bytes());
        for &it in items {
            self.patch_u32(Some(it));
        }
        off
    }

    fn annotation_set_ref_list(&mut self, sets: &[u32]) -> u32 {
        self.align4();
        let off = self.buf.len() as u32;
        self.buf
            .extend_from_slice(&(sets.len() as u32).to_le_bytes());
        for &s in sets {
            self.patch_u32(Some(s));
        }
        off
    }

    fn annotations_directory(
        &mut self,
        class_set: Option<u32>,
        fields: &[(u32, u32)],
        methods: &[(u32, u32)],
        params: &[(u32, u32)],
    ) -> u32 {
        self.align4();
        let off = self.buf.len() as u32;
        self.patch_u32(class_set);
        self.buf
            .extend_from_slice(&(fields.len() as u32).to_le_bytes());
        self.buf
            .extend_from_slice(&(methods.len() as u32).to_le_bytes());
        self.buf
            .extend_from_slice(&(params.len() as u32).to_le_bytes());
        for &(i, o) in fields {
            self.buf.extend_from_slice(&i.to_le_bytes());
            self.patch_u32(Some(o));
        }
        for &(i, o) in methods {
            self.buf.extend_from_slice(&i.to_le_bytes());
            self.patch_u32(Some(o));
        }
        for &(i, o) in params {
            self.buf.extend_from_slice(&i.to_le_bytes());
            self.patch_u32(Some(o));
        }
        off
    }

    fn simple_code(&mut self, registers: u16, ins: u16, insns: &[u16]) -> u32 {
        self.align4();
        let off = self.buf.len() as u32;
        self.buf.extend_from_slice(&registers.to_le_bytes());
        self.buf.extend_from_slice(&ins.to_le_bytes());
        self.buf.extend_from_slice(&0u16.to_le_bytes());
        self.buf.extend_from_slice(&0u16.to_le_bytes());
        self.buf.extend_from_slice(&0u32.to_le_bytes());
        self.buf
            .extend_from_slice(&(insns.len() as u32).to_le_bytes());
        for &u in insns {
            self.buf.extend_from_slice(&u.to_le_bytes());
        }
        off
    }

    fn aliased_direct_methods_class_data(
        &mut self,
        method_idx: u32,
        repeat_count: usize,
        code_rel: u32,
    ) -> u32 {
        let off = self.buf.len() as u32;
        self.uleb(0); // static_fields_size
        self.uleb(0); // instance_fields_size
        self.uleb(repeat_count as u32); // direct_methods_size
        self.uleb(0); // virtual_methods_size
        for i in 0..repeat_count {
            let diff = if i == 0 { method_idx } else { 0 };
            self.uleb(diff);
            self.uleb(0x0008); // ACC_STATIC
            self.uleb5_patch(code_rel);
        }
        off
    }

    fn add_class(&mut self, class_idx: u32, sup: u32, anns: Option<u32>, data: Option<u32>) {
        self.classes.push([
            class_idx,
            0x0001,
            sup,
            SENTINEL_OFF,
            NO_INDEX,
            anns.unwrap_or(0),
            data.unwrap_or(0),
            0,
        ]);
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
        let type_off = 112 + 4 * ns;
        let proto_off = type_off + 4 * nt;
        let field_off = proto_off + 12 * np;
        let method_off = field_off + 8 * nf;
        let class_off = method_off + 8 * nm;
        let data_base = class_off + 32 * nc;

        let mut d = vec![0u8; data_base.max(112)];
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
        put32(&mut d, 108, data_base as u32);

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
                if poff == SENTINEL_OFF {
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
            put32(&mut d, base, c[0]);
            put32(&mut d, base + 4, c[1]);
            put32(&mut d, base + 8, c[2]);
            put32(
                &mut d,
                base + 12,
                if c[3] == SENTINEL_OFF {
                    0
                } else {
                    data_base as u32 + c[3]
                },
            );
            put32(&mut d, base + 16, c[4]);
            put32(
                &mut d,
                base + 20,
                if c[5] == 0 {
                    0
                } else {
                    data_base as u32 + c[5]
                },
            );
            put32(
                &mut d,
                base + 24,
                if c[6] == 0 {
                    0
                } else {
                    data_base as u32 + c[6]
                },
            );
            put32(
                &mut d,
                base + 28,
                if c[7] == 0 {
                    0
                } else {
                    data_base as u32 + c[7]
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

#[test]
fn aliased_and_overlapping_string_data_off_rejected_fast() {
    // 1. 16_000 string_id_item entries all pointing to a single 16 KiB string (~80 KiB input,
    //    ~256 MiB eager amplification without budget).
    let count = 16_000u32;
    let str_len = 16 * 1024usize;
    let mut data = Vec::with_capacity((count as usize) * 4 + str_len + 16);
    let target_off = count * 4;
    for _ in 0..count {
        data.extend_from_slice(&target_off.to_le_bytes());
    }
    push_uleb(str_len as u32, &mut data);
    data.extend(std::iter::repeat(b'A').take(str_len));
    data.push(0);

    let t0 = Instant::now();
    let res = Strings::parse(&data, count, 0);
    assert!(res.is_err(), "expected aliased string_data_off to fail");
    assert!(
        t0.elapsed().as_millis() < 1000,
        "aliased Strings::parse took too long: {:?}",
        t0.elapsed()
    );

    // 2. Overlapping distinct offsets (`base + 2*i`) into the same 20 KiB ASCII body
    //    (evades naive offset-equality dedup caches).
    let ov_count = 6_000u32;
    let body_len = 20 * 1024usize;
    let mut ov = Vec::with_capacity((ov_count as usize) * 4 + body_len + 16);
    let body_start = (ov_count as usize) * 4;
    for i in 0..ov_count as usize {
        // Each offset `body_start + 2*i` starts with byte `b'A'` (= uleb128 utf16_len 65)
        // followed by all remaining `'A'` bytes up to the single NUL terminator at the end!
        let off = (body_start + (i * 2).min(body_len - 2)) as u32;
        ov.extend_from_slice(&off.to_le_bytes());
    }
    ov.extend(std::iter::repeat(b'A').take(body_len));
    ov.push(0);

    let t1 = Instant::now();
    let ov_res = Strings::parse(&ov, ov_count, 0);
    assert!(
        ov_res.is_err(),
        "expected overlapping string_data_off suffixes to fail"
    );
    assert!(
        t1.elapsed().as_millis() < 1000,
        "overlapping Strings::parse took too long: {:?}",
        t1.elapsed()
    );
}

#[test]
fn disasm_render_class_blocks_annotation_and_method_dag_amplification() {
    // Build a ~4 KiB DEX with 8 ClassDefs sharing class_data_off (200 direct methods with
    // method_idx_diff = 0) and a 3-layer parameter_annotations DAG (64 params -> shared set of
    // 64 refs -> shared annotation_item with 64 array elements: > 400M nodes unguarded).
    let mut db = SyntheticDex::new();
    let obj_t = db.type_id("Ljava/lang/Object;");
    let void_t = db.type_id("V");
    let int_t = db.type_id("I");
    let ann_t = db.type_id("Lpkg/Tag;");
    let val_name = db.str_id("value");

    let params: Vec<u32> = std::iter::repeat(int_t).take(64).collect();
    let proto = db.proto_id(&params, void_t);

    let cls0 = db.type_id("Lpkg/Dag0;");
    let m0 = db.method_id(cls0 as u16, "f", proto);

    // return-void (0x000e) with 64 incoming int parameter registers.
    let code_rel = db.simple_code(64, 64, &[0x000e]);

    // Single annotation_item containing `value = { 0, 0, ..., 0 }` (64 elements).
    let mut ann_elems = Vec::new();
    push_uleb(val_name, &mut ann_elems);
    ann_elems.push(0x1c); // VALUE_ARRAY (value_arg = 0)
    push_uleb(64, &mut ann_elems);
    for _ in 0..64 {
        ann_elems.push(0x00); // VALUE_BYTE (1 byte)
        ann_elems.push(0x00);
    }
    let ann_item = db.annotation_item(1, ann_t, 1, &ann_elems);
    let shared_set: Vec<u32> = std::iter::repeat(ann_item).take(64).collect();
    let set_off = db.annotation_set(&shared_set);
    let shared_refs: Vec<u32> = std::iter::repeat(set_off).take(64).collect();
    let ref_list_off = db.annotation_set_ref_list(&shared_refs);
    let dir_off = db.annotations_directory(None, &[], &[(m0, set_off)], &[(m0, ref_list_off)]);
    let cd_off = db.aliased_direct_methods_class_data(m0, 200, code_rel);

    db.add_class(cls0, obj_t, Some(dir_off), Some(cd_off));
    for k in 1..8 {
        let ct = db.type_id(&format!("Lpkg/Dag{k};"));
        db.add_class(ct, obj_t, Some(dir_off), Some(cd_off));
    }

    let bytes = db.finish();
    assert!(
        bytes.len() < 8192,
        "expected compact fuzz-sized DEX, got {} bytes",
        bytes.len()
    );

    let t0 = Instant::now();
    let parsed = Dex::parse(bytes);
    assert!(parsed.is_ok(), "synthetic DEX header/tables should parse");
    if let Ok(dex) = parsed {
        let mut saw_budget_err = false;
        for def in dex.class_defs.iter().take(8) {
            if disasm::render_class(&dex, def.index).is_err() {
                saw_budget_err = true;
            }
        }
        assert!(saw_budget_err, "expected DAG render_class to hit budget");
    }
    assert!(
        t0.elapsed().as_millis() < 1500,
        "fuzz target replica took too long: {:?}",
        t0.elapsed()
    );

    // Duplicate method_idx in annotations_directory_item must be rejected immediately.
    let mut dup_db = SyntheticDex::new();
    let o_t = dup_db.type_id("Ljava/lang/Object;");
    let v_t = dup_db.type_id("V");
    let a_t = dup_db.type_id("Lpkg/Ann;");
    let c_t = dup_db.type_id("Lpkg/DupDir;");
    let pr = dup_db.proto_id(&[], v_t);
    let mid = dup_db.method_id(c_t as u16, "m", pr);
    let c_rel = dup_db.simple_code(1, 0, &[0x000e]);
    let ai = dup_db.annotation_item(1, a_t, 0, &[]);
    let as_off = dup_db.annotation_set(&[ai]);
    let dup_dir = dup_db.annotations_directory(None, &[], &[(mid, as_off), (mid, as_off)], &[]);
    let dup_cd = dup_db.aliased_direct_methods_class_data(mid, 1, c_rel);
    dup_db.add_class(c_t, o_t, Some(dup_dir), Some(dup_cd));
    let dup_dex_res = Dex::parse(dup_db.finish());
    assert!(dup_dex_res.is_ok());
    if let Ok(dup_dex) = dup_dex_res {
        assert!(
            disasm::render_class(&dup_dex, 0).is_err(),
            "duplicate method_idx in annotations_directory must fail"
        );
    }
}

#[test]
fn disasm_render_class_caps_repeated_large_const_string_output() {
    let mut db = SyntheticDex::new();
    let obj_t = db.type_id("Ljava/lang/Object;");
    let void_t = db.type_id("V");
    let cls_t = db.type_id("Lpkg/BigStr;");
    let big = "X".repeat(24 * 1024);
    let big_idx = db.str_id(&big);
    let proto = db.proto_id(&[], void_t);
    let mid = db.method_id(cls_t as u16, "spam", proto);

    // 200 × `const-string v0, big_idx` (opcode 0x1a, format 21c) + `return-void` (0x000e):
    // ~4.8 MiB of smali output from a ~26 KiB DEX.
    let mut insns = Vec::with_capacity(401);
    for _ in 0..200 {
        insns.push(0x001a);
        insns.push(big_idx as u16);
    }
    insns.push(0x000e);
    let code_rel = db.simple_code(1, 0, &insns);
    let cd_off = db.aliased_direct_methods_class_data(mid, 1, code_rel);
    db.add_class(cls_t, obj_t, None, Some(cd_off));

    let t0 = Instant::now();
    let dex_res = Dex::parse(db.finish());
    assert!(dex_res.is_ok());
    if let Ok(dex) = dex_res {
        assert!(
            disasm::render_class(&dex, 0).is_err(),
            "expected oversized const-string expansion to exceed render output budget"
        );
    }
    assert!(
        t0.elapsed().as_millis() < 1000,
        "const-string cap took too long: {:?}",
        t0.elapsed()
    );
}

#[test]
fn code_item_aliased_catch_handler_off_rejected_while_valid_shared_finally_passes() {
    // 1. Adversarial ~3.5 KiB code_item: 400 TryItems all pointing to handler_off = 1
    //    with 64 typed catch clauses (25_600 clauses from ~3.5 KiB).
    let tries_size = 400u16;
    let mut bad = Vec::new();
    bad.extend_from_slice(&1u16.to_le_bytes()); // registers_size
    bad.extend_from_slice(&0u16.to_le_bytes()); // ins_size
    bad.extend_from_slice(&0u16.to_le_bytes()); // outs_size
    bad.extend_from_slice(&tries_size.to_le_bytes());
    bad.extend_from_slice(&0u32.to_le_bytes()); // debug_info_off
    bad.extend_from_slice(&2u32.to_le_bytes()); // insns_size = 2 units (4 bytes, already aligned)
    bad.extend_from_slice(&0x0000u16.to_le_bytes()); // nop
    bad.extend_from_slice(&0x000eu16.to_le_bytes()); // return-void
    for _ in 0..tries_size {
        bad.extend_from_slice(&0u32.to_le_bytes()); // start_addr = 0
        bad.extend_from_slice(&1u16.to_le_bytes()); // insn_count = 1
        bad.extend_from_slice(&1u16.to_le_bytes()); // handler_off = 1 (right after list_size byte)
    }
    bad.push(1); // encoded_catch_handler_list size = 1
    push_sleb(-64, &mut bad); // 64 typed pairs + catch_all
    for ty in 0..64u32 {
        push_uleb(ty, &mut bad);
        push_uleb(1, &mut bad);
    }
    push_uleb(1, &mut bad); // catch_all addr

    let t0 = Instant::now();
    let bad_res = CodeItem::parse(&bad, 0);
    assert!(
        bad_res.is_err(),
        "expected aliased catch handler explosion to fail"
    );
    assert!(
        t0.elapsed().as_millis() < 1000,
        "CodeItem::parse aliased handler check took too long: {:?}",
        t0.elapsed()
    );

    // 2. Legitimate javac `try/finally` pattern: 4 TryItems sharing handler_off = 1.
    let mut ok_code = Vec::new();
    ok_code.extend_from_slice(&1u16.to_le_bytes());
    ok_code.extend_from_slice(&0u16.to_le_bytes());
    ok_code.extend_from_slice(&0u16.to_le_bytes());
    ok_code.extend_from_slice(&4u16.to_le_bytes()); // 4 tries sharing 1 catchall handler
    ok_code.extend_from_slice(&0u32.to_le_bytes());
    ok_code.extend_from_slice(&4u32.to_le_bytes()); // 4 units
    for _ in 0..3 {
        ok_code.extend_from_slice(&0x0000u16.to_le_bytes());
    }
    ok_code.extend_from_slice(&0x000eu16.to_le_bytes());
    for s in 0..4u32 {
        ok_code.extend_from_slice(&s.to_le_bytes());
        ok_code.extend_from_slice(&1u16.to_le_bytes());
        ok_code.extend_from_slice(&1u16.to_le_bytes()); // shared handler_off = 1
    }
    ok_code.push(1);
    push_sleb(0, &mut ok_code); // 0 typed + catch_all
    push_uleb(3, &mut ok_code);

    let ok_res = CodeItem::parse(&ok_code, 0);
    assert!(
        ok_res.is_ok(),
        "valid shared try/finally handler_off should parse cleanly"
    );
}
