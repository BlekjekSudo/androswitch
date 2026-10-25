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
