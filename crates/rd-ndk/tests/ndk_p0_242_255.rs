use rd_ndk::elf::{Abi, Elf};

/// Builds a minimal little-endian `ET_DYN` ELF64 (`EM_X86_64`) image with one
/// `PT_LOAD` (`0x0..0x2000`) and one `PT_DYNAMIC` table at `0xa00`.
fn craft_elf64(
    extra_load: Option<(u64, u64)>,
    dyn_tags: &[(u64, u64)],
    write_payload: impl FnOnce(&mut [u8]),
) -> Vec<u8> {
    let mut d = vec![0u8; 0x3000];
    d[0..4].copy_from_slice(b"\x7fELF");
    d[4] = 2; // ELF64
    d[5] = 1; // LE
    d[6] = 1; // v1
    d[16..18].copy_from_slice(&3u16.to_le_bytes()); // ET_DYN
    d[18..20].copy_from_slice(&62u16.to_le_bytes()); // EM_X86_64
    d[20..24].copy_from_slice(&1u32.to_le_bytes());
    d[32..40].copy_from_slice(&64u64.to_le_bytes()); // e_phoff

    let phcount: u16 = if extra_load.is_some() { 3 } else { 2 };
    d[52..54].copy_from_slice(&64u16.to_le_bytes());
    d[54..56].copy_from_slice(&56u16.to_le_bytes()); // e_phentsize
    d[56..58].copy_from_slice(&phcount.to_le_bytes());

    // PHDR 0: PT_LOAD covering 0x0..0x1200 (file-backed)
    let p0 = 64usize;
    d[p0..p0 + 4].copy_from_slice(&1u32.to_le_bytes());
    d[p0 + 4..p0 + 8].copy_from_slice(&7u32.to_le_bytes());
    d[p0 + 32..p0 + 40].copy_from_slice(&0x1200u64.to_le_bytes());
    d[p0 + 40..p0 + 48].copy_from_slice(&0x1200u64.to_le_bytes());
    d[p0 + 48..p0 + 56].copy_from_slice(&0x1000u64.to_le_bytes());

    let mut next_ph = p0 + 56;
    if let Some((va, msz)) = extra_load {
        d[next_ph..next_ph + 4].copy_from_slice(&1u32.to_le_bytes());
        d[next_ph + 4..next_ph + 8].copy_from_slice(&5u32.to_le_bytes());
        d[next_ph + 8..next_ph + 16].copy_from_slice(&0x2000u64.to_le_bytes());
        d[next_ph + 16..next_ph + 24].copy_from_slice(&va.to_le_bytes());
        d[next_ph + 32..next_ph + 40].copy_from_slice(&msz.min(0x800).to_le_bytes());
        d[next_ph + 40..next_ph + 48].copy_from_slice(&msz.to_le_bytes());
        d[next_ph + 48..next_ph + 56].copy_from_slice(&0x1000u64.to_le_bytes());
        next_ph += 56;
    }

    write_payload(&mut d);

    let mut all_tags = dyn_tags.to_vec();
    all_tags.push((0, 0));
    let dyn_bytes = all_tags.len() * 16;
    for (idx, (tag, val)) in all_tags.iter().enumerate() {
        let o = 0xa00 + idx * 16;
        d[o..o + 8].copy_from_slice(&tag.to_le_bytes());
        d[o + 8..o + 16].copy_from_slice(&val.to_le_bytes());
    }

    // PT_DYNAMIC at 0xa00
    d[next_ph..next_ph + 4].copy_from_slice(&2u32.to_le_bytes());
    d[next_ph + 4..next_ph + 8].copy_from_slice(&6u32.to_le_bytes());
    d[next_ph + 8..next_ph + 16].copy_from_slice(&0xa00u64.to_le_bytes());
    d[next_ph + 16..next_ph + 24].copy_from_slice(&0xa00u64.to_le_bytes());
    d[next_ph + 32..next_ph + 40].copy_from_slice(&(dyn_bytes as u64).to_le_bytes());
    d[next_ph + 40..next_ph + 48].copy_from_slice(&(dyn_bytes as u64).to_le_bytes());
    d
}

#[test]
fn p0_242_relr_odd_bitmap_near_u64_max_returns_error_without_panic() {
    // DT_RELR (36) at 0x600, DT_RELRSZ (35) = 16, DT_RELRENT (37) = 8:
    // Entry 0 (even): 0xffff_ffff_ffff_ffe0 -> address becomes 0xffff_ffff_ffff_ffe8.
    // Entry 1 (odd bitmap): 0x3f -> bit 4..5 previously overflowed `u64` and panicked at elf.rs:337.
    let img = craft_elf64(None, &[(36, 0x600), (35, 16), (37, 8)], |d| {
        d[0x600..0x608].copy_from_slice(&0xffff_ffff_ffff_ffe0u64.to_le_bytes());
        d[0x608..0x610].copy_from_slice(&0x3fu64.to_le_bytes());
    });
    let err = Elf::parse(&img).expect_err("overflowing DT_RELR bitmap must be rejected");
    assert_eq!(err.code, "INVALID_ELF");
}

#[test]
fn p0_242_defined_symbol_values_validated_against_pt_load_segments() {
    let make_sym_elf = |sym_value: u64, sym_info: u8, shndx: u16| {
        craft_elf64(
            Some((0x2000, 0x800)), // hole at 0x1200..0x2000!
            &[
                (5, 0x300), // DT_STRTAB
                (10, 16),   // DT_STRSZ
                (6, 0x400), // DT_SYMTAB
                (11, 24),   // DT_SYMENT
                (4, 0x500), // DT_HASH -> nchain = 2
            ],
            |d| {
                d[0x300..0x305].copy_from_slice(b"\0fn0\0");
                d[0x500..0x504].copy_from_slice(&1u32.to_le_bytes()); // nbucket
                d[0x504..0x508].copy_from_slice(&2u32.to_le_bytes()); // nchain = 2
                let s1 = 0x400 + 24;
                d[s1..s1 + 4].copy_from_slice(&1u32.to_le_bytes()); // name = "fn0"
                d[s1 + 4] = sym_info;
                d[s1 + 6..s1 + 8].copy_from_slice(&shndx.to_le_bytes());
                d[s1 + 8..s1 + 16].copy_from_slice(&sym_value.to_le_bytes());
            },
        )
    };

    // Valid function inside PT_LOAD (0x2100 in 0x2000..0x2800) succeeds
    let ok = Elf::parse(&make_sym_elf(0x2100, 0x12, 1)).expect("in-range symbol should parse");
    assert_eq!(ok.symbol("fn0"), Some(0x2100));

    // Linker boundary symbol (`_end` at exact end 0x2800 of PT_LOAD) succeeds
    let boundary =
        Elf::parse(&make_sym_elf(0x2800, 0x10, 1)).expect("segment-end marker should parse");
    assert_eq!(boundary.symbol("fn0"), Some(0x2800));

    // Out-of-bounds values (`u64::MAX`, ARENA delta `0x3000_0000`, unmapped hole `0x1800`) fail cleanly
    for bad_val in [u64::MAX, 0x3000_0000, 0x1800] {
        let err = Elf::parse(&make_sym_elf(bad_val, 0x12, 1))
            .expect_err("out-of-bounds defined symbol must fail");
        assert_eq!(err.code, "INVALID_ELF");
    }
}

#[test]
fn p0_242_dt_init_and_dt_init_array_validated_against_pt_load_and_arm_thumb_allowed() {
    // Out-of-bounds DT_INIT (12) -> rejected
    for bad_init in [u64::MAX, 0x6000_0000, 0x1800] {
        let img = craft_elf64(Some((0x2000, 0x800)), &[(12, bad_init)], |_| {});
        let err = Elf::parse(&img).expect_err("out-of-bounds DT_INIT must fail");
        assert_eq!(err.code, "INVALID_ELF");
    }

    // Valid DT_INIT inside PT_LOAD -> accepted
    let good_init = craft_elf64(None, &[(12, 0x800)], |_| {});
    assert_eq!(
        Elf::parse(&good_init).expect("valid DT_INIT").init,
        Some(0x800)
    );

    // Out-of-bounds DT_INIT_ARRAY (25) / DT_INIT_ARRAYSZ (27) -> rejected
    for (arr_va, arr_sz) in [(0x4000_0000, 8), (0x11fc, 8), (u64::MAX - 8, 16)] {
        let img = craft_elf64(None, &[(25, arr_va), (27, arr_sz)], |_| {});
        let err = Elf::parse(&img).expect_err("out-of-bounds DT_INIT_ARRAY must fail");
        assert_eq!(err.code, "INVALID_ELF");
    }

    // ARM32 (`armeabi-v7a`) Thumb LSB (`addr | 1`) on defined symbol and DT_INIT is accepted
    let mut arm = vec![0u8; 0x1000];
    arm[0..4].copy_from_slice(b"\x7fELF");
    arm[4] = 1; // ELF32
    arm[5] = 1;
    arm[6] = 1;
    arm[16..18].copy_from_slice(&3u16.to_le_bytes()); // ET_DYN
    arm[18..20].copy_from_slice(&40u16.to_le_bytes()); // EM_ARM
    arm[20..24].copy_from_slice(&1u32.to_le_bytes());
    arm[28..32].copy_from_slice(&52u32.to_le_bytes()); // e_phoff
    arm[42..44].copy_from_slice(&32u16.to_le_bytes()); // e_phentsize
    arm[44..46].copy_from_slice(&2u16.to_le_bytes()); // PT_LOAD + PT_DYNAMIC

    // PT_LOAD covering 0x0..0x800 (note Thumb function at 0x7fe has st_value = 0x7ff!)
    let p0 = 52usize;
    arm[p0..p0 + 4].copy_from_slice(&1u32.to_le_bytes());
    arm[p0 + 16..p0 + 20].copy_from_slice(&0x800u32.to_le_bytes());
    arm[p0 + 20..p0 + 24].copy_from_slice(&0x800u32.to_le_bytes());
    arm[p0 + 24..p0 + 28].copy_from_slice(&5u32.to_le_bytes());

    arm[0x200..0x207].copy_from_slice(b"\0thumb\0");
    arm[0x240..0x244].copy_from_slice(&1u32.to_le_bytes());
    arm[0x244..0x248].copy_from_slice(&2u32.to_le_bytes()); // nchain = 2
    let sym1 = 0x280 + 16;
    arm[sym1..sym1 + 4].copy_from_slice(&1u32.to_le_bytes());
    arm[sym1 + 4..sym1 + 8].copy_from_slice(&0x7ffu32.to_le_bytes()); // Thumb LSB set at end of segment!
    arm[sym1 + 12] = 0x12; // STB_GLOBAL | STT_FUNC
    arm[sym1 + 14..sym1 + 16].copy_from_slice(&1u16.to_le_bytes());

    let dyn_tags: [(u32, u32); 7] = [
        (5, 0x200),
        (10, 7),
        (4, 0x240),
        (6, 0x280),
        (11, 16),
        (12, 0x7ff), // Thumb DT_INIT!
        (0, 0),
    ];
    for (i, (t, v)) in dyn_tags.iter().enumerate() {
        let o = 0x400 + i * 8;
        arm[o..o + 4].copy_from_slice(&t.to_le_bytes());
        arm[o + 4..o + 8].copy_from_slice(&v.to_le_bytes());
    }
    let p1 = p0 + 32;
    arm[p1..p1 + 4].copy_from_slice(&2u32.to_le_bytes());
    arm[p1 + 4..p1 + 8].copy_from_slice(&0x400u32.to_le_bytes());
    arm[p1 + 8..p1 + 12].copy_from_slice(&0x400u32.to_le_bytes());
    arm[p1 + 16..p1 + 20].copy_from_slice(&56u32.to_le_bytes());
    arm[p1 + 20..p1 + 24].copy_from_slice(&56u32.to_le_bytes());

    let parsed_arm = Elf::parse(&arm).expect("ARM32 Thumb symbol and DT_INIT must succeed");
    assert_eq!(parsed_arm.abi, Abi::Arm);
    assert_eq!(parsed_arm.symbol("thumb"), Some(0x7ff));
    assert_eq!(parsed_arm.init, Some(0x7ff));
}

#[test]
fn hidden_gnu_hash_and_vaoff_overflow_rejected_cleanly() {
    // Crafted DT_GNU_HASH (0x6ffffef5) where bucket index exceeds MAX_ENTRIES -> rejected cleanly
    let img = craft_elf64(
        None,
        &[
            (5, 0x300),
            (10, 8),
            (6, 0x400),
            (11, 24),
            (0x6ffffef5, 0x500),
        ],
        |d| {
            d[0x500..0x504].copy_from_slice(&1u32.to_le_bytes()); // nbucket = 1
            d[0x504..0x508].copy_from_slice(&1u32.to_le_bytes()); // symoffset = 1
            d[0x508..0x50c].copy_from_slice(&1u32.to_le_bytes()); // bloom_size = 1
            d[0x518..0x51c].copy_from_slice(&0xffff_fffeu32.to_le_bytes()); // bucket[0] huge!
        },
    );
    let err = Elf::parse(&img).expect_err("oversized DT_GNU_HASH chain must fail");
    assert_eq!(err.code, "INVALID_ELF");
}

#[cfg(feature = "native")]
mod p0_255_runtime_integration {
    use std::collections::BTreeMap;
    use std::sync::{Mutex, OnceLock};

    use rd_ndk::elf::Abi;
    use rd_ndk::runtime::{Argument, Return, Runtime, TRAPS};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    const MOCK_PY_SCRIPT: &str = r#"#!/usr/bin/env python3
import json, sys

mem = {}
mapped = []

def is_mapped(addr, size):
    if size == 0:
        return True
    end = addr + size
    cur = addr
    while cur < end:
        ok = False
        for a, s in mapped:
            if a <= cur < a + s:
                cur = min(end, a + s)
                ok = True
                break
        if not ok:
            return False
    return True

def send(v):
    sys.stdout.write(json.dumps(v) + "\n")
    sys.stdout.flush()

for raw in sys.stdin:
    q = json.loads(raw)
    op = q["op"]
    if op == "init":
        send({"version": 1})
    elif op == "map":
        mapped.append((int(q["address"]), int(q["size"])))
        send({"event": "ok"})
    elif op == "write":
        addr = int(q["address"])
        data = bytes.fromhex(q["hex"])
        if not is_mapped(addr, len(data)):
            send({"event": "error", "code": "NATIVE_FAULT", "cause": "unmapped write"})
        else:
            for idx, b in enumerate(data):
                mem[addr + idx] = b
            send({"event": "ok"})
    elif op == "read":
        addr = int(q["address"])
        sz = int(q["size"])
        if not is_mapped(addr, sz):
            send({"event": "error", "code": "NATIVE_FAULT", "cause": "unmapped read"})
        else:
            out = bytes(mem.get(addr + idx, 0) for idx in range(sz))
            send({"event": "ok", "hex": out.hex()})
    elif op == "call":
        addr = int(q["address"])
        slot = (addr - 0x70000000) // 16
        args = [int(a["bits"]) for a in q["args"]]
        send({"event": "trap", "slot": slot, "args": args, "fp": [], "sp": 0x60000100, "instructions": 1})
    elif op == "resume":
        send({"event": "return", "value": int(q["value"]), "instructions": 2})
"#;

    fn mock_runtime(abi: Abi) -> Runtime {
        static SCRIPT_PATH: OnceLock<std::path::PathBuf> = OnceLock::new();
        let path = SCRIPT_PATH.get_or_init(|| {
            let p = std::env::temp_dir()
                .join(format!("rd_ndk_mock_worker_255_{}.py", std::process::id()));
            std::fs::write(&p, MOCK_PY_SCRIPT).expect("write mock worker");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&p).expect("meta").permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&p, perms).expect("chmod");
            }
            p
        });
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("RD_NATIVE_PYTHON", path);
        let mut rt = Runtime::new(abi).expect("Runtime::new with mock worker");
        rt.map(0x6000_0000, 0x1000).expect("map stack");
        rt
    }

    fn build_import_elf64(imports: &[(&str, bool)]) -> Vec<u8> {
        let mut d = vec![0u8; 0x2000];
        d[0..4].copy_from_slice(b"\x7fELF");
        d[4] = 2; // ELF64
        d[5] = 1;
        d[6] = 1;
        d[16..18].copy_from_slice(&3u16.to_le_bytes()); // ET_DYN
        d[18..20].copy_from_slice(&62u16.to_le_bytes()); // EM_X86_64
        d[20..24].copy_from_slice(&1u32.to_le_bytes());
        d[32..40].copy_from_slice(&64u64.to_le_bytes());
        d[52..54].copy_from_slice(&64u16.to_le_bytes());
        d[54..56].copy_from_slice(&56u16.to_le_bytes());
        d[56..58].copy_from_slice(&2u16.to_le_bytes());

        let p0 = 64usize;
        d[p0..p0 + 4].copy_from_slice(&1u32.to_le_bytes());
        d[p0 + 4..p0 + 8].copy_from_slice(&7u32.to_le_bytes());
        d[p0 + 32..p0 + 40].copy_from_slice(&0x2000u64.to_le_bytes());
        d[p0 + 40..p0 + 48].copy_from_slice(&0x2000u64.to_le_bytes());
        d[p0 + 48..p0 + 56].copy_from_slice(&0x1000u64.to_le_bytes());

        let mut strtab = vec![0u8];
        for (idx, (name, weak)) in imports.iter().enumerate() {
            let noff = strtab.len() as u32;
            strtab.extend_from_slice(name.as_bytes());
            strtab.push(0);
            let sym_off = 0x400 + (idx + 1) * 24;
            d[sym_off..sym_off + 4].copy_from_slice(&noff.to_le_bytes());
            d[sym_off + 4] = if *weak { 0x22 } else { 0x12 };
            let rela_off = 0x700 + idx * 24;
            let got_va = 0x1000u64 + (idx as u64) * 8;
            let r_info = (((idx + 1) as u64) << 32) | 7u64; // R_X86_64_JUMP_SLOT
            d[rela_off..rela_off + 8].copy_from_slice(&got_va.to_le_bytes());
            d[rela_off + 8..rela_off + 16].copy_from_slice(&r_info.to_le_bytes());
        }
        d[0x200..0x200 + strtab.len()].copy_from_slice(&strtab);

        let dyn_entries: [(u64, u64); 8] = [
            (5, 0x200),
            (10, strtab.len() as u64),
            (6, 0x400),
            (11, 24),
            (7, 0x700),
            (8, (imports.len() * 24) as u64),
            (9, 24),
            (0, 0),
        ];
        for (i, (tag, val)) in dyn_entries.iter().enumerate() {
            let o = 0xa00 + i * 16;
            d[o..o + 8].copy_from_slice(&tag.to_le_bytes());
            d[o + 8..o + 16].copy_from_slice(&val.to_le_bytes());
        }
        let p1 = 64 + 56;
        d[p1..p1 + 4].copy_from_slice(&2u32.to_le_bytes());
        d[p1 + 4..p1 + 8].copy_from_slice(&6u32.to_le_bytes());
        d[p1 + 8..p1 + 16].copy_from_slice(&0xa00u64.to_le_bytes());
        d[p1 + 16..p1 + 24].copy_from_slice(&0xa00u64.to_le_bytes());
        d[p1 + 32..p1 + 40].copy_from_slice(&128u64.to_le_bytes());
        d[p1 + 40..p1 + 48].copy_from_slice(&128u64.to_le_bytes());
        d
    }

    fn build_import_elf32_arm(imports: &[&str]) -> Vec<u8> {
        let mut d = vec![0u8; 0x2000];
        d[0..4].copy_from_slice(b"\x7fELF");
        d[4] = 1; // ELF32
        d[5] = 1;
        d[6] = 1;
        d[16..18].copy_from_slice(&3u16.to_le_bytes()); // ET_DYN
        d[18..20].copy_from_slice(&40u16.to_le_bytes()); // EM_ARM
        d[20..24].copy_from_slice(&1u32.to_le_bytes());
        d[28..32].copy_from_slice(&52u32.to_le_bytes()); // e_phoff
        d[42..44].copy_from_slice(&32u16.to_le_bytes()); // e_phentsize
        d[44..46].copy_from_slice(&2u16.to_le_bytes());

        let p0 = 52usize;
        d[p0..p0 + 4].copy_from_slice(&1u32.to_le_bytes());
        d[p0 + 16..p0 + 20].copy_from_slice(&0x2000u32.to_le_bytes());
        d[p0 + 20..p0 + 24].copy_from_slice(&0x2000u32.to_le_bytes());
        d[p0 + 24..p0 + 28].copy_from_slice(&7u32.to_le_bytes());
        d[p0 + 28..p0 + 32].copy_from_slice(&0x1000u32.to_le_bytes());

        let mut strtab = vec![0u8];
        for (idx, name) in imports.iter().enumerate() {
            let noff = strtab.len() as u32;
            strtab.extend_from_slice(name.as_bytes());
            strtab.push(0);
            let sym_off = 0x400 + (idx + 1) * 16;
            d[sym_off..sym_off + 4].copy_from_slice(&noff.to_le_bytes());
            d[sym_off + 12] = 0x12; // STB_GLOBAL | STT_FUNC
            let rel_off = 0x700 + idx * 8;
            let got_va = 0x1000u32 + (idx as u32) * 4;
            let r_info = (((idx + 1) as u32) << 8) | 22u32; // R_ARM_JUMP_SLOT = 22
            d[rel_off..rel_off + 4].copy_from_slice(&got_va.to_le_bytes());
            d[rel_off + 4..rel_off + 8].copy_from_slice(&r_info.to_le_bytes());
        }
        d[0x200..0x200 + strtab.len()].copy_from_slice(&strtab);

        let dyn_entries: [(u32, u32); 8] = [
            (5, 0x200),
            (10, strtab.len() as u32),
            (6, 0x400),
            (11, 16),
            (17, 0x700), // DT_REL
            (18, (imports.len() * 8) as u32),
            (19, 8),
            (0, 0),
        ];
        for (i, (tag, val)) in dyn_entries.iter().enumerate() {
            let o = 0xa00 + i * 8;
            d[o..o + 4].copy_from_slice(&tag.to_le_bytes());
            d[o + 4..o + 8].copy_from_slice(&val.to_le_bytes());
        }
        let p1 = 52 + 32;
        d[p1..p1 + 4].copy_from_slice(&2u32.to_le_bytes());
        d[p1 + 4..p1 + 8].copy_from_slice(&0xa00u32.to_le_bytes());
        d[p1 + 8..p1 + 12].copy_from_slice(&0xa00u32.to_le_bytes());
        d[p1 + 16..p1 + 20].copy_from_slice(&64u32.to_le_bytes());
        d[p1 + 20..p1 + 24].copy_from_slice(&64u32.to_le_bytes());
        d[p1 + 24..p1 + 28].copy_from_slice(&6u32.to_le_bytes());
        d
    }

    fn call_tramp(rt: &mut Runtime, tramp: u64, args: &[u64]) -> rd_ndk::Result<u64> {
        let wrapped: Vec<Argument> = args.iter().map(|&b| Argument::word(b)).collect();
        let ret = rt.call(tramp, &wrapped, 64, |_, _| {
            Ok(Return {
                bits: 0,
                fp_bits: None,
            })
        })?;
        Ok(ret.bits)
    }

    #[test]
    fn p0_255_printf_sign_zero_pad_width_caps_and_arm32_intmax_end_to_end() {
        let mut rt = mock_runtime(Abi::X86_64);
        let mut avail = BTreeMap::new();
        avail.insert(
            "libfmt.so".to_string(),
            build_import_elf64(&[("snprintf", false)]),
        );
        rt.load("libfmt.so", &avail).expect("load libfmt.so");
        let snprintf_tramp = rt.read_word(0x10000000 + 0x1000).expect("GOT[0]");
        assert!(snprintf_tramp >= TRAPS);

        let dst = rt.allocate(64).expect("dst");
        let fmt_sign = rt.alloc_bytes(b"%05d|%+05d|%#08x\0").expect("fmt");
        let n = call_tramp(
            &mut rt,
            snprintf_tramp,
            &[dst, 64, fmt_sign, (-42i64) as u64, 42, 0xab],
        )
        .expect("snprintf sign/zero");
        assert_eq!(n, 20);
        assert_eq!(
            rt.read(dst, 20).expect("read formatted"),
            b"-0042|+0042|0x0000ab"
        );

        // Oversized literal width (> 1 MiB) and dynamic `%*s` width (`i32::MAX` / `i32::MIN`) -> NATIVE_MEMORY (no 2 GiB/18 EiB OOM)
        let hi = rt.alloc_bytes(b"hi\0").expect("hi");
        let fmt_huge = rt.alloc_bytes(b"%2000000s\0").expect("huge fmt");
        let err_huge = call_tramp(&mut rt, snprintf_tramp, &[dst, 64, fmt_huge, hi])
            .expect_err("oversized width must fail cleanly");
        assert_eq!(err_huge.code, "NATIVE_MEMORY");

        let fmt_star = rt.alloc_bytes(b"%*s\0").expect("star fmt");
        for bad_w in [i32::MAX, i32::MIN, -2_000_000] {
            let err = call_tramp(
                &mut rt,
                snprintf_tramp,
                &[dst, 64, fmt_star, (bad_w as u32) as u64, hi],
            )
            .expect_err("huge dynamic width must fail cleanly");
            assert_eq!(err.code, "NATIVE_MEMORY");
        }

        // Negative dynamic width `-8` left-aligns cleanly (`"hi      "`), negative precision `-1` omits cap
        assert_eq!(
            call_tramp(
                &mut rt,
                snprintf_tramp,
                &[dst, 64, fmt_star, (-8i32 as u32) as u64, hi],
            )
            .expect("neg width"),
            8
        );
        assert_eq!(rt.read(dst, 8).expect("read neg width"), b"hi      ");

        let fmt_prec = rt.alloc_bytes(b"%.*s\0").expect("prec fmt");
        let hello = rt.alloc_bytes(b"hello\0").expect("hello");
        assert_eq!(
            call_tramp(
                &mut rt,
                snprintf_tramp,
                &[dst, 64, fmt_prec, (-1i32 as u32) as u64, hello],
            )
            .expect("neg prec"),
            5
        );
        assert_eq!(rt.read(dst, 5).expect("read neg prec"), b"hello");

        // ARM32 (`armeabi-v7a`): `%jd` is 64-bit `intmax_t` with 8-byte AAPCS32 va_list alignment!
        let mut arm_rt = mock_runtime(Abi::Arm);
        let mut arm_avail = BTreeMap::new();
        arm_avail.insert(
            "libarm.so".to_string(),
            build_import_elf32_arm(&["vsnprintf"]),
        );
        arm_rt
            .load("libarm.so", &arm_avail)
            .expect("load libarm.so");
        let vsnprintf_tramp = arm_rt.read_word(0x10000000 + 0x1000).expect("ARM GOT[0]");
        let arm_dst = arm_rt.allocate(64).expect("arm dst");
        let arm_fmt = arm_rt.alloc_bytes(b"%jd|%s\0").expect("arm fmt");
        let arm_ok = arm_rt.alloc_bytes(b"ok\0").expect("arm ok");
        let val64: i64 = -0x1234_5678_9abc_def0;
        let lo = ((val64 as u64) & 0xffff_ffff) as u32;
        let hi_w = (((val64 as u64) >> 32) & 0xffff_ffff) as u32;
        // Start `va_list` at unaligned `0x6000_0204`; AAPCS32 aligns 64-bit arg up to `0x6000_0208`!
        arm_rt
            .write(0x6000_0208, &lo.to_le_bytes())
            .expect("write lo");
        arm_rt
            .write(0x6000_020c, &hi_w.to_le_bytes())
            .expect("write hi");
        arm_rt
            .write(0x6000_0210, &(arm_ok as u32).to_le_bytes())
            .expect("write ok ptr");
        let expected = format!("{val64}|ok");
        let arm_n = call_tramp(
            &mut arm_rt,
            vsnprintf_tramp,
            &[arm_dst, 64, arm_fmt, 0x6000_0204],
        )
        .expect("ARM32 vsnprintf %jd");
        assert_eq!(arm_n as usize, expected.len());
        assert_eq!(
            arm_rt.read(arm_dst, expected.len()).expect("read arm_dst"),
            expected.as_bytes()
        );
    }

    #[test]
    fn p0_255_bounded_reads_near_unmapped_page_and_posix_memalign_end_to_end() {
        let mut rt = mock_runtime(Abi::X86_64);
        let mut avail = BTreeMap::new();
        avail.insert(
            "libmem.so".to_string(),
            build_import_elf64(&[
                ("strncmp", false),
                ("strncpy", false),
                ("snprintf", false),
                ("memcmp", false),
                ("posix_memalign", false),
            ]),
        );
        rt.load("libmem.so", &avail).expect("load libmem.so");
        let strncmp_tramp = rt.read_word(0x10000000 + 0x1000).expect("GOT[0]");
        let strncpy_tramp = rt.read_word(0x10000000 + 0x1008).expect("GOT[1]");
        let snprintf_tramp = rt.read_word(0x10000000 + 0x1010).expect("GOT[2]");
        let memcmp_tramp = rt.read_word(0x10000000 + 0x1018).expect("GOT[3]");
        let memalign_tramp = rt.read_word(0x10000000 + 0x1020).expect("GOT[4]");

        // Map single page 0x3000_0000..0x3000_1000 (`0x3000_1000` is unmapped!)
        rt.map(0x3000_0000, 0x1000).expect("map edge page");
        let edge = 0x3000_0ffb;
        rt.write(edge, b"ABCDE").expect("write edge");
        let other = rt.alloc_bytes(b"ABCDE\0").expect("other");
        let dst = rt.allocate(16).expect("dst");

        assert_eq!(
            call_tramp(&mut rt, strncmp_tramp, &[edge, other, 5]).expect("strncmp bounded"),
            0
        );
        assert_eq!(
            call_tramp(&mut rt, strncpy_tramp, &[dst, edge, 5]).expect("strncpy bounded"),
            dst
        );
        assert_eq!(rt.read(dst, 5).expect("strncpy bytes"), b"ABCDE");

        let fmt_prec = rt.alloc_bytes(b"%.*s\0").expect("fmt");
        assert_eq!(
            call_tramp(&mut rt, snprintf_tramp, &[dst, 16, fmt_prec, 5, edge])
                .expect("snprintf %.*s bounded"),
            5
        );
        assert_eq!(rt.read(dst, 6).expect("snprintf bytes"), b"ABCDE\0");

        let err_mc = call_tramp(&mut rt, memcmp_tramp, &[other, other, 16 * 1024 * 1024 + 1])
            .expect_err("memcmp > 16 MiB must fail");
        assert_eq!(err_mc.code, "NATIVE_MEMORY");

        let out_ptr = rt.allocate(8).expect("out_ptr");
        for bad_align in [1u64, 2, 4] {
            assert_eq!(
                call_tramp(&mut rt, memalign_tramp, &[out_ptr, bad_align, 32])
                    .expect("posix_memalign bad align"),
                22
            );
        }
        let err_ovf = call_tramp(&mut rt, memalign_tramp, &[out_ptr, 16, u64::MAX - 4])
            .expect_err("posix_memalign overflow");
        assert_eq!(err_ovf.code, "NATIVE_OOM");
    }

    #[test]
    fn p0_255_weak_builtin_imports_bind_to_trampolines_end_to_end() {
        let mut rt = mock_runtime(Abi::X86_64);
        let syms = [
            ("getauxval", true),
            ("posix_memalign", true),
            ("snprintf", true),
            ("fmodf", true),
            ("pthread_rwlock_rdlock", true),
        ];
        let mut avail = BTreeMap::new();
        avail.insert("libweak.so".to_string(), build_import_elf64(&syms));
        rt.load("libweak.so", &avail).expect("load libweak.so");
        for (idx, (sym, _)) in syms.iter().enumerate() {
            let got = rt
                .read_word(0x10000000 + 0x1000 + (idx as u64) * 8)
                .expect("read GOT");
            assert!(
                got >= TRAPS,
                "weak builtin {sym} resolved to {got:#x} instead of TRAPS trampoline"
            );
        }
    }
}
