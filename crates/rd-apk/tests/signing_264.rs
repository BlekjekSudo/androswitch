//! Regression tests for issue #264 (`APK Sig Block 42` bounds, initial/trailing
//! `size_of_block` equality, `block_size >= 24`, and strict `pair_end <= cd - 24`).

use rd_apk::signing::{
    self, SigningInfo, APK_SIG_BLOCK_MAGIC, BLOCK_ID_V2, BLOCK_ID_V3, BLOCK_ID_V3_1,
};
use rd_apk::Zip;

fn empty_zip() -> Zip {
    // Minimal 22-byte EOCD-only empty ZIP archive.
    let mut eocd = vec![0x50, 0x4b, 0x05, 0x06];
    eocd.resize(22, 0);
    Zip::parse(eocd).expect("valid empty zip")
}

fn build_sig_block(initial_size: u64, pairs: &[u8], trailing_size: u64) -> (Vec<u8>, u64) {
    let mut buf = vec![0xAA; 16]; // synthetic local-entry prefix before sig block
    buf.extend_from_slice(&initial_size.to_le_bytes());
    buf.extend_from_slice(pairs);
    buf.extend_from_slice(&trailing_size.to_le_bytes());
    buf.extend_from_slice(APK_SIG_BLOCK_MAGIC);
    let cd_offset = buf.len() as u64;
    buf.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]); // synthetic CD marker
    (buf, cd_offset)
}

fn make_pair(id: u32, payload: &[u8]) -> Vec<u8> {
    let pair_len = (4 + payload.len()) as u64;
    let mut out = Vec::with_capacity(8 + 4 + payload.len());
    out.extend_from_slice(&pair_len.to_le_bytes());
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

#[test]
fn valid_v2_v3_v31_sig_block_is_detected() {
    let zip = empty_zip();
    let mut pairs = Vec::new();
    pairs.extend_from_slice(&make_pair(BLOCK_ID_V2, &[1, 2, 3, 4]));
    pairs.extend_from_slice(&make_pair(BLOCK_ID_V3, &[5, 6]));
    pairs.extend_from_slice(&make_pair(BLOCK_ID_V3_1, &[]));
    let block_size = (pairs.len() + 24) as u64;
    let (data, cd) = build_sig_block(block_size, &pairs, block_size);

    assert_eq!(
        signing::detect(&data, cd, &zip),
        SigningInfo {
            v1: false,
            v2: true,
            v3: true,
            v3_1: true,
        }
    );
}

#[test]
fn issue_264_rejects_block_size_below_minimum_24() {
    let zip = empty_zip();
    // Craft trailing `block_size = 12` (< 24) so `cd - 12 - 8` lands inside the
    // footer/magic itself; even if bytes happen to encode a fake V2 ID, it must
    // be rejected.
    let mut data = vec![0u8; 64];
    let cd = 48usize;
    data[cd - 16..cd].copy_from_slice(APK_SIG_BLOCK_MAGIC);
    data[cd - 24..cd - 16].copy_from_slice(&12u64.to_le_bytes());
    // Put matching initial `12u64` at `block_start = cd - 20 = 28` and fake pair!
    data[28..36].copy_from_slice(&12u64.to_le_bytes());
    assert_eq!(
        signing::detect(&data, cd as u64, &zip),
        SigningInfo::default(),
        "block_size < 24 must be rejected"
    );

    // Boundary `block_size = 23` (< 24) also rejected; `block_size = 24` (empty pairs) accepted as unsigned.
    let (d23, cd23) = build_sig_block(23, &[], 23);
    assert_eq!(signing::detect(&d23, cd23, &zip), SigningInfo::default());
}

#[test]
fn issue_264_rejects_mismatched_initial_and_trailing_size_of_block() {
    let zip = empty_zip();
    let pairs = make_pair(BLOCK_ID_V2, &[0xde, 0xad, 0xbe, 0xef]);
    let block_size = (pairs.len() + 24) as u64;
    // Corrupt initial `size_of_block` (`block_size + 1` != `block_size`).
    let (data, cd) = build_sig_block(block_size + 1, &pairs, block_size);

    assert_eq!(
        signing::detect(&data, cd, &zip),
        SigningInfo::default(),
        "divergent initial vs trailing size_of_block must not report v2 signature"
    );
}

#[test]
fn issue_264_rejects_pair_invading_trailing_size_of_block_footer_and_u64_wrap() {
    let zip = empty_zip();
    // Pair claims `pair_len = 12` (4-byte ID + 8 bytes payload), while the pair
    // area only has `12` total bytes (`8` len + `4` ID + `0` payload), meaning
    // `pair_end = end + 8` invades the 8-byte trailing `size_of_block`!
    let mut bad_pair = Vec::new();
    bad_pair.extend_from_slice(&12u64.to_le_bytes()); // pair_len = 12 (needs 20 bytes total!)
    bad_pair.extend_from_slice(&BLOCK_ID_V2.to_le_bytes()); // only 12 bytes in pairs slice!
    let block_size = (bad_pair.len() + 24) as u64;
    let (data, cd) = build_sig_block(block_size, &bad_pair, block_size);

    assert_eq!(
        signing::detect(&data, cd, &zip),
        SigningInfo::default(),
        "pair extending past cd - 24 into trailing size_of_block must be rejected"
    );

    // 64-bit `block_size = 0x1_0000_0020` (would truncate to `0x20` on 32-bit `as usize`):
    let (mut data64, cd64) = build_sig_block(32, &pairs_8bytes(), 0x1_0000_0020);
    let cd_idx = cd64 as usize;
    data64[cd_idx - 24..cd_idx - 16].copy_from_slice(&0x1_0000_0020u64.to_le_bytes());
    assert_eq!(signing::detect(&data64, cd64, &zip), SigningInfo::default());
}

fn pairs_8bytes() -> Vec<u8> {
    vec![0u8; 8]
}
