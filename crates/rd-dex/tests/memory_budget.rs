use rd_dex::Dex;

fn minimal_dex() -> Vec<u8> {
    let mut d = vec![0u8; 112];
    d[0..4].copy_from_slice(b"dex\n");
    d[4..7].copy_from_slice(b"035");
    d[7] = 0;
    d[36..40].copy_from_slice(&112u32.to_le_bytes()); // header_size
                                                      // endian LE
    d[40..44].copy_from_slice(&rd_dex::dex::ENDIAN_CONSTANT_LE.to_le_bytes());
    // file_size = 116 (header 112 + map list size 0)
    let map_off = 112u32;
    d[52..56].copy_from_slice(&map_off.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes()); // map list size 0
    let file_size = d.len() as u32;
    d[32..36].copy_from_slice(&file_size.to_le_bytes());
    d
}

#[test]
fn budgeted_parser_rejects_expansion_and_malformed_offsets_before_decode() {
    let mut bytes = minimal_dex();
    let count = 2048u32;
    let ids = bytes.len() as u32;
    let string = ids + count * 4;
    for _ in 0..count {
        bytes.extend_from_slice(&string.to_le_bytes());
    }
    bytes.push(1); // declared UTF-16 length is untrusted, as in ordinary parser.
    bytes.extend_from_slice(&[b'x'; 1024]);
    bytes.push(0);
    bytes[56..60].copy_from_slice(&count.to_le_bytes());
    bytes[60..64].copy_from_slice(&ids.to_le_bytes());
    let size = bytes.len() as u32;
    bytes[32..36].copy_from_slice(&size.to_le_bytes());
    let checksum = rd_dex::dex::adler32(&bytes[12..]);
    bytes[8..12].copy_from_slice(&checksum.to_le_bytes());
    let error = Dex::parse_budgeted(bytes.clone(), 2 << 20).unwrap_err();
    assert_eq!(error.code, "DEX_MEMORY_BUDGET");
    let (dex, estimate) = Dex::parse_budgeted(bytes.clone(), 32 << 20).unwrap();
    assert!(estimate > 12 << 20);
    assert_eq!(dex.string(2047).len(), 1024);
    assert_eq!(
        Dex::parse_budgeted(bytes.clone(), estimate - 1)
            .unwrap_err()
            .code,
        "DEX_MEMORY_BUDGET"
    );
    bytes[ids as usize..ids as usize + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    let checksum = rd_dex::dex::adler32(&bytes[12..]);
    bytes[8..12].copy_from_slice(&checksum.to_le_bytes());
    assert_eq!(
        Dex::parse_budgeted(bytes, 32 << 20).unwrap_err().code,
        "PARSE_ERROR"
    );
}

#[test]
fn budgeted_parser_validates_integrity_before_decoded_allocation() {
    let bytes = minimal_dex();
    assert_eq!(
        Dex::parse_budgeted(bytes, usize::MAX).unwrap_err().code,
        "INVALID_CHECKSUM"
    );
}
