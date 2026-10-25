//! Regression suite for Issue #332 (P0/T0 Security in `rd-dex`):
//! 1. `Dex::parse` / `LazyDex::mount`: duplicate `MapType` entries in `map_list`
//!    (`CallSiteId`, `MethodHandle`, etc.) and trailing bytes beyond `header.file_size`.
//! 2. `disasm::render_class`: uncached/unstepped `proto_sig`, unbounded buffered
//!    strings in `write_method`/`MethodRender::build`, and `EncodedValue::MethodType`
//!    `u16` truncation.
//! 3. `read_encoded_value`: uncapped `Vec::with_capacity(count)` and unbounded
//!    total `EncodedValue` node count.
//! 4. `LazyDex` / `Strings::parse_lazy` / `RawClassIndex`: decoded lazy-string byte
//!    budget under `string_id` aliasing and memoized `str_off` hashing in `find_class`.
//! 5. `Reader::sleb128` 5th-byte continuation/sign-extension validation and
//!    `CodeItem::parse` `u32`/`usize`-safe `try_item` range check.

use std::time::{Duration, Instant};

use rd_dex::annotations::read_encoded_value;
use rd_dex::code::CodeItem;
use rd_dex::dex::adler32;
use rd_dex::disasm;
use rd_dex::read::Reader;
use rd_dex::strings::{Strings, NO_INDEX};
use rd_dex::{Dex, DexBacking, LazyDex};

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

fn finalize_dex_checksum(d: &mut [u8], file_size: u32) {
    d[32..36].copy_from_slice(&file_size.to_le_bytes());
    let fs = (file_size as usize).min(d.len());
    if fs >= 12 {
        let csum = adler32(&d[12..fs]);
        d[8..12].copy_from_slice(&csum.to_le_bytes());
    }
}

fn minimal_dex_with_map(
    version: &[u8; 3],
    map_items: &[(u16, u32, u32)],
    payload: &[u8],
) -> Vec<u8> {
    let mut d = vec![0u8; 112];
    d[0..4].copy_from_slice(b"dex\n");
    d[4..7].copy_from_slice(version);
    d[7] = 0;
    d[36..40].copy_from_slice(&112u32.to_le_bytes());
    d[40..44].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    d.extend_from_slice(payload);
    while d.len() % 4 != 0 {
        d.push(0);
    }
    let map_off = d.len() as u32;
    d[52..56].copy_from_slice(&map_off.to_le_bytes());
    d.extend_from_slice(&(map_items.len() as u32).to_le_bytes());
    for &(ty, size, off) in map_items {
        d.extend_from_slice(&ty.to_le_bytes());
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend_from_slice(&size.to_le_bytes());
        d.extend_from_slice(&off.to_le_bytes());
    }
    let fs = d.len() as u32;
    finalize_dex_checksum(&mut d, fs);
    d
}

#[test]
fn map_list_rejects_duplicate_sections_and_clamps_to_header_file_size_332() {
    // (1a) Duplicate CallSiteId (0x0007) entries in map_list must be rejected
    // by both Dex::parse and LazyDex::mount instead of amplifying call_site_offsets.
    let payload = [0u8; 16]; // 4 u32 words at offset 112
    let dup_callsites =
        minimal_dex_with_map(b"038", &[(0x0007, 4, 112), (0x0007, 4, 112)], &payload);
    assert!(
        Dex::parse(dup_callsites.clone()).is_err(),
        "Dex::parse must reject duplicate MapType::CallSiteId in map_list"
    );
    assert!(
        LazyDex::mount(DexBacking::from(dup_callsites)).is_err(),
        "LazyDex::mount must reject duplicate MapType::CallSiteId in map_list"
    );

    // Duplicate MethodHandle (0x0008) entries in map_list must also be rejected.
    let mh_payload = [0u8; 16]; // 2 x 8-byte method_handle_item
    let dup_mh = minimal_dex_with_map(b"038", &[(0x0008, 2, 112), (0x0008, 2, 112)], &mh_payload);
    assert!(
        Dex::parse(dup_mh.clone()).is_err(),
        "Dex::parse must reject duplicate MapType::MethodHandle in map_list"
    );
    assert!(
        LazyDex::mount(DexBacking::from(dup_mh)).is_err(),
        "LazyDex::mount must reject duplicate MapType::MethodHandle in map_list"
    );

    // (1b) Trailing container bytes past header.file_size must NOT be readable by
    // map_list or section tables.
    let mut padded = minimal_dex_with_map(b"035", &[(0x1001, 0, 112)], &[]);
    // Shrink header.file_size to 112 so map_off=112 lies strictly outside [0..file_size],
    // while keeping trailing bytes in the outer Vec.
    finalize_dex_checksum(&mut padded, 112);
    assert!(
        padded.len() > 112,
        "outer buffer still carries trailing bytes beyond header.file_size"
    );
    assert!(
        Dex::parse(padded.clone()).is_err(),
        "Dex::parse must slice buffer to header.file_size before reading map_list/sections"
    );
    assert!(
        LazyDex::mount(DexBacking::from(padded)).is_err(),
        "LazyDex::mount must slice buffer to header.file_size before reading map_list/sections"
    );
}

#[test]
fn disasm_render_class_charges_proto_sig_steps_and_validates_method_type_u16_332() {
    // Build a compact (< 6 KiB) synthetic DEX where:
    // - proto_ids[0] has 500 `I` parameters: `(II...I)V`
    // - method `LTest;->caller()V` executes 200 `invoke-static {}, LTest;->callee(II...I)V`
    //   instructions (200 * 500 = 100,000 parameter visits on a ~4.5 KiB DEX whose
    //   RenderWriter step budget is `dex.len() * 8 < 40,000`).
    let mut data_sec = Vec::<u8>::new();
    let push_str_item = |sec: &mut Vec<u8>, s: &str| -> u32 {
        let off = sec.len() as u32;
        push_uleb(s.encode_utf16().count() as u32, sec);
        sec.extend_from_slice(s.as_bytes());
        sec.push(0);
        off
    };
    let s_i = push_str_item(&mut data_sec, "I");
    let s_ltest = push_str_item(&mut data_sec, "LTest;");
    let s_lobj = push_str_item(&mut data_sec, "Ljava/lang/Object;");
    let s_v = push_str_item(&mut data_sec, "V");
    let shorty_500 = format!("V{}", "I".repeat(500));
    let s_shorty500 = push_str_item(&mut data_sec, &shorty_500);
    let s_caller = push_str_item(&mut data_sec, "caller");
    let s_callee = push_str_item(&mut data_sec, "callee");
    let str_rels = [s_i, s_ltest, s_lobj, s_v, s_shorty500, s_caller, s_callee];

    // type_ids: 0="I", 1="LTest;", 2="Ljava/lang/Object;", 3="V"
    let type_str_idxs = [0u32, 1, 2, 3];

    // type_list for proto 1 (500 x type_id 0 = "I")
    while data_sec.len() % 4 != 0 {
        data_sec.push(0);
    }
    let tl_rel = data_sec.len() as u32;
    data_sec.extend_from_slice(&500u32.to_le_bytes());
    for _ in 0..500 {
        data_sec.extend_from_slice(&0u16.to_le_bytes());
    }

    // code_item for caller(): 200 x `invoke-static {}, method@1` (35c: 0x0071, 0x0001, 0x0000) + `return-void` (0x000e)
    while data_sec.len() % 4 != 0 {
        data_sec.push(0);
    }
    let code_rel = data_sec.len() as u32;
    let insn_units = 200u32 * 3 + 1;
    data_sec.extend_from_slice(&1u16.to_le_bytes()); // registers_size
    data_sec.extend_from_slice(&0u16.to_le_bytes()); // ins_size
    data_sec.extend_from_slice(&0u16.to_le_bytes()); // outs_size
    data_sec.extend_from_slice(&0u16.to_le_bytes()); // tries_size
    data_sec.extend_from_slice(&0u32.to_le_bytes()); // debug_info_off
    data_sec.extend_from_slice(&insn_units.to_le_bytes());
    for _ in 0..200 {
        data_sec.extend_from_slice(&0x0071u16.to_le_bytes());
        data_sec.extend_from_slice(&0x0001u16.to_le_bytes()); // method@1 = callee
        data_sec.extend_from_slice(&0x0000u16.to_le_bytes());
    }
    data_sec.extend_from_slice(&0x000eu16.to_le_bytes()); // return-void

    // static_values_off with EncodedValue::MethodType(65536) (`0x0001_0000` > u16::MAX)
    let sv_rel = data_sec.len() as u32;
    push_uleb(1, &mut data_sec); // array size = 1
                                 // VALUE_METHOD_TYPE = 0x15, value_arg = 2 (3 bytes little-endian: 0x00, 0x00, 0x01 -> 65536)
    data_sec.push((2 << 5) | 0x15);
    data_sec.extend_from_slice(&[0x00, 0x00, 0x01]);

    // Build header + fixed tables:
    // strings(7), types(4), protos(2: ()V and (I*500)V), fields(1 static), methods(2), classes(2)
    let ns = str_rels.len();
    let nt = type_str_idxs.len();
    let np = 2usize;
    let nf = 1usize;
    let nm = 2usize;
    let nc = 2usize;
    let str_off = 112usize;
    let type_off = str_off + ns * 4;
    let proto_off = type_off + nt * 4;
    let field_off = proto_off + np * 12;
    let method_off = field_off + nf * 8;
    let class_off = method_off + nm * 8;
    let data_base = class_off + nc * 32;

    // Reserve class_data_item for class 0 (caller) at the end of data_sec now that we know data_base!
    let cd0_rel = data_sec.len() as u32;
    push_uleb(0, &mut data_sec); // static_fields
    push_uleb(0, &mut data_sec); // instance_fields
    push_uleb(1, &mut data_sec); // direct_methods = 1 (method 0 = caller)
    push_uleb(0, &mut data_sec); // virtual_methods
    push_uleb(0, &mut data_sec); // method_idx_diff = 0
    push_uleb(0x0008, &mut data_sec); // ACC_STATIC
    push_uleb(data_base as u32 + code_rel, &mut data_sec);

    let cd1_rel = data_sec.len() as u32;
    push_uleb(1, &mut data_sec); // static_fields = 1 (field 0)
    push_uleb(0, &mut data_sec); // instance_fields
    push_uleb(0, &mut data_sec); // direct_methods
    push_uleb(0, &mut data_sec); // virtual_methods
    push_uleb(0, &mut data_sec); // field_idx_diff = 0
    push_uleb(0x0008, &mut data_sec); // ACC_STATIC

    let mut d = vec![0u8; data_base];
    d[0..8].copy_from_slice(b"dex\n038\0");
    let put32 = |buf: &mut [u8], off: usize, v: u32| {
        buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
    };
    put32(&mut d, 36, 112);
    put32(&mut d, 40, 0x1234_5678);
    put32(&mut d, 56, ns as u32);
    put32(&mut d, 60, str_off as u32);
    put32(&mut d, 64, nt as u32);
    put32(&mut d, 68, type_off as u32);
    put32(&mut d, 72, np as u32);
    put32(&mut d, 76, proto_off as u32);
    put32(&mut d, 80, nf as u32);
    put32(&mut d, 84, field_off as u32);
    put32(&mut d, 88, nm as u32);
    put32(&mut d, 92, method_off as u32);
    put32(&mut d, 96, nc as u32);
    put32(&mut d, 100, class_off as u32);
    put32(&mut d, 104, data_sec.len() as u32);
    put32(&mut d, 108, data_base as u32);

    for (i, &rel) in str_rels.iter().enumerate() {
        put32(&mut d, str_off + i * 4, data_base as u32 + rel);
    }
    for (i, &si) in type_str_idxs.iter().enumerate() {
        put32(&mut d, type_off + i * 4, si);
    }
    // proto 0: ()V -> shorty="V"(3), return_type="V"(3), params=0
    put32(&mut d, proto_off, 3);
    put32(&mut d, proto_off + 4, 3);
    put32(&mut d, proto_off + 8, 0);
    // proto 1: (I*500)V -> shorty=4, return_type="V"(3), params=data_base + tl_rel
    put32(&mut d, proto_off + 12, 4);
    put32(&mut d, proto_off + 16, 3);
    put32(&mut d, proto_off + 20, data_base as u32 + tl_rel);

    // field 0: LTest;->caller:I
    d[field_off..field_off + 2].copy_from_slice(&1u16.to_le_bytes());
    d[field_off + 2..field_off + 4].copy_from_slice(&0u16.to_le_bytes());
    put32(&mut d, field_off + 4, 5);

    // method 0: LTest;->caller()V (proto 0)
    d[method_off..method_off + 2].copy_from_slice(&1u16.to_le_bytes());
    d[method_off + 2..method_off + 4].copy_from_slice(&0u16.to_le_bytes());
    put32(&mut d, method_off + 4, 5);
    // method 1: LTest;->callee(I*500)V (proto 1)
    d[method_off + 8..method_off + 10].copy_from_slice(&1u16.to_le_bytes());
    d[method_off + 10..method_off + 12].copy_from_slice(&1u16.to_le_bytes());
    put32(&mut d, method_off + 12, 6);

    // class 0: LTest; with caller() hitting 200 * 500 proto_sig steps
    put32(&mut d, class_off, 1);
    put32(&mut d, class_off + 4, 0x0001);
    put32(&mut d, class_off + 8, 2);
    put32(&mut d, class_off + 16, NO_INDEX);
    put32(&mut d, class_off + 24, data_base as u32 + cd0_rel);

    // class 1: Ljava/lang/Object; with static_values_off containing MethodType(65536)
    let c1 = class_off + 32;
    put32(&mut d, c1, 2);
    put32(&mut d, c1 + 4, 0x0001);
    put32(&mut d, c1 + 8, NO_INDEX);
    put32(&mut d, c1 + 16, NO_INDEX);
    put32(&mut d, c1 + 24, data_base as u32 + cd1_rel);
    put32(&mut d, c1 + 28, data_base as u32 + sv_rel);

    d.extend_from_slice(&data_sec);
    let fs = d.len() as u32;
    finalize_dex_checksum(&mut d, fs);
    assert!(
        d.len() < 4096,
        "compact DEX (<4 KiB -> max_steps < 32,768), got {}",
        d.len()
    );

    let dex = Dex::parse(d).expect("valid structural DEX");
    assert!(
        disasm::render_class(&dex, 0).is_err(),
        "200 invoke-static calls to a 500-param proto (100,000 steps) must trip RenderWriter step budget"
    );
    assert!(
        disasm::render_class(&dex, 1).is_err(),
        "EncodedValue::MethodType(65536) must not truncate u32 -> u16 (65536 -> proto_ids[0])"
    );
}

#[test]
fn read_encoded_value_caps_total_nodes_and_nested_preallocation_332() {
    // 1. Single top-level ARRAY with 300,000 VALUE_NULL elements (300 KB input)
    // must be rejected by the per-call node budget instead of allocating ~10 MiB+ of AST nodes.
    let count = 300_000u32;
    let mut raw = Vec::with_capacity(count as usize + 8);
    raw.push(0x1c); // VALUE_ARRAY (arg=0)
    push_uleb(count, &mut raw);
    raw.resize(raw.len() + count as usize, 0x1e); // VALUE_NULL
    let mut r = Reader::new(&raw);
    let res = read_encoded_value(&mut r);
    assert!(
        res.is_err(),
        "read_encoded_value must cap total decoded EncodedValue nodes, got Ok"
    );

    // 2. Truncated nested array headers (`0x1c <uleb 2_000_000>` x 8 followed by 2 MB of 0x1e
    // truncated at EOF) must fail fast without multi-GiB Vec::with_capacity pre-allocation.
    let mut nested = Vec::with_capacity(2_000_100);
    for _ in 0..8 {
        nested.push(0x1c);
        push_uleb(2_000_000, &mut nested);
    }
    nested.resize(2_000_050, 0x1e);
    let t0 = Instant::now();
    let mut r2 = Reader::new(&nested);
    assert!(read_encoded_value(&mut r2).is_err());
    assert!(
        t0.elapsed() < Duration::from_millis(500),
        "nested read_encoded_value took too long: {:?}",
        t0.elapsed()
    );
}

#[test]
fn lazy_dex_and_lazy_strings_enforce_decoded_byte_budget_and_memoize_class_index_332() {
    // Build a ~25 KiB DEX (`byte_budget` = `max(len * 4, 64 KiB)` = ~100 KiB) with:
    // - 200 string_ids all aliased to ONE 16 KiB non-ASCII MUTF-8 string ("é" * 8192)
    // - 2,000 class_defs all aliased to that same non-ASCII descriptor (32 MiB of MUTF-8
    //   hashing on first find_class without str_off memoization).
    let non_ascii = "é".repeat(8192); // 16,384 UTF-8 bytes, 8,192 UTF-16 code units
    let mut str_item = Vec::new();
    push_uleb(8192, &mut str_item);
    str_item.extend_from_slice(non_ascii.as_bytes());
    str_item.push(0);

    let ns = 200usize;
    let nt = 1usize;
    let nc = 2_000usize;
    let str_off = 112usize;
    let type_off = str_off + ns * 4;
    let class_off = type_off + nt * 4;
    let data_base = class_off + nc * 32;

    let mut d = vec![0u8; data_base];
    d[0..8].copy_from_slice(b"dex\n035\0");
    let put32 = |buf: &mut [u8], off: usize, v: u32| {
        buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
    };
    put32(&mut d, 36, 112);
    put32(&mut d, 40, 0x1234_5678);
    put32(&mut d, 56, ns as u32);
    put32(&mut d, 60, str_off as u32);
    put32(&mut d, 64, nt as u32);
    put32(&mut d, 68, type_off as u32);
    put32(&mut d, 96, nc as u32);
    put32(&mut d, 100, class_off as u32);
    put32(&mut d, 104, str_item.len() as u32);
    put32(&mut d, 108, data_base as u32);

    for i in 0..ns {
        put32(&mut d, str_off + i * 4, data_base as u32);
    }
    put32(&mut d, type_off, 0);
    for i in 0..nc {
        let c = class_off + i * 32;
        put32(&mut d, c, 0);
        put32(&mut d, c + 4, 0x0001);
        put32(&mut d, c + 8, NO_INDEX);
        put32(&mut d, c + 16, NO_INDEX);
    }
    d.extend_from_slice(&str_item);
    let fs = d.len() as u32;
    finalize_dex_checksum(&mut d, fs);

    // (4a) Strings::parse_lazy + try_get_with_data must trip the cumulative decoded-byte budget
    // (200 * 16 KiB = 3.2 MiB >> ~330 KiB budget for an ~82 KiB DEX).
    let lazy_strings = Strings::parse_lazy(&d, ns as u32, str_off as u32).expect("parse_lazy");
    let mut saw_budget_err = false;
    for i in 0..ns as u32 {
        if lazy_strings.try_get_with_data(i, &d).is_err() {
            saw_budget_err = true;
            break;
        }
    }
    assert!(
        saw_budget_err,
        "Strings::parse_lazy must enforce cumulative decoded-byte budget across aliased string_ids"
    );

    // LazyDex::string must likewise enforce the cumulative decoded-byte budget.
    let lazy = LazyDex::mount(DexBacking::from(d)).expect("mount");
    let mut saw_lazy_err = false;
    for i in 0..ns as u32 {
        if lazy.string(i).is_err() {
            saw_lazy_err = true;
            break;
        }
    }
    assert!(
        saw_lazy_err,
        "LazyDex::string must enforce cumulative decoded-byte budget across aliased string_ids"
    );

    // (4b) First find_class on 2,000 aliased non-ASCII class_defs must memoize/deduplicate str_off.
    let t0 = Instant::now();
    let _ = lazy.find_class("Lcom/missing/Target;");
    assert!(
        t0.elapsed() < Duration::from_millis(50),
        "LazyDex::find_class took {:?} on aliased non-ASCII class_defs",
        t0.elapsed()
    );
}

#[test]
fn sleb128_rejects_5th_byte_continuation_and_sign_overflow_and_code_try_range_overflow_332() {
    // Unterminated 5-byte sleb128 must NOT return Ok(0) or silently truncate a 6-byte sequence.
    for bad in [
        &[0x80u8, 0x80, 0x80, 0x80, 0x80][..],
        &[0x81, 0x80, 0x80, 0x80, 0x80, 0x01][..],
        &[0x80, 0x80, 0x80, 0x80, 0xf8][..],
        &[0x80, 0x80, 0x80, 0x80, 0x08][..], // +2^31 > i32::MAX
        &[0xff, 0xff, 0xff, 0xff, 0x77][..], // -2^31 - 1 < i32::MIN
    ] {
        let mut r = Reader::new(bad);
        assert!(
            r.sleb128().is_err(),
            "Reader::sleb128 must reject malformed/overflowing 5-byte sequence {bad:02x?}"
        );
    }

    // Valid 5-byte boundary encodings must continue to round-trip accurately.
    for (good, expected) in [
        (&[0x80u8, 0x80, 0x80, 0x80, 0x00][..], 0i32),
        (&[0xff, 0xff, 0xff, 0xff, 0x7f][..], -1i32),
        (&[0xff, 0xff, 0xff, 0xff, 0x07][..], i32::MAX),
        (&[0x80, 0x80, 0x80, 0x80, 0x78][..], i32::MIN),
    ] {
        let mut r = Reader::new(good);
        assert_eq!(r.sleb128().expect("valid 5-byte sleb128"), expected);
    }

    // CodeItem::parse with try_item { start_addr: 0xFFFF_FFFF, insn_count: 1 } must fail cleanly
    // with INVALID_FORMAT on all pointer widths (including 32-bit wasm32).
    let mut code = Vec::new();
    code.extend_from_slice(&1u16.to_le_bytes()); // registers_size
    code.extend_from_slice(&0u16.to_le_bytes()); // ins_size
    code.extend_from_slice(&0u16.to_le_bytes()); // outs_size
    code.extend_from_slice(&1u16.to_le_bytes()); // tries_size = 1
    code.extend_from_slice(&0u32.to_le_bytes()); // debug_info_off
    code.extend_from_slice(&1u32.to_le_bytes()); // insns_size = 1
    code.extend_from_slice(&0x000eu16.to_le_bytes()); // return-void
    code.extend_from_slice(&0u16.to_le_bytes()); // 4-byte padding before tries
                                                 // try_item (8 bytes): start_addr=0xFFFF_FFFF, insn_count=1, handler_off=1
    code.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    code.extend_from_slice(&1u16.to_le_bytes());
    code.extend_from_slice(&1u16.to_le_bytes());
    // encoded_catch_handler_list: size=1, catch_all_addr=0 (sleb 0 -> 0x00, addr 0 -> 0x00)
    code.push(0x01);
    code.push(0x00);
    code.push(0x00);

    let err = CodeItem::parse(&code, 0).expect_err("overflowing try range must fail");
    assert_eq!(err.code, "INVALID_FORMAT");
}
