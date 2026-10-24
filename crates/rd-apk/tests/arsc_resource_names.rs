//! Original tiny binary resource tables exercise identity independently of APKs.
use rd_apk::arsc::{identifier_name, identifier_number, Arsc, ResourceName};
fn put16(data: &mut [u8], at: usize, value: u16) {
    data[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(data: &mut [u8], at: usize, value: u32) {
    data[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn pool(strings: &[&str]) -> Vec<u8> {
    let start = 28 + strings.len() * 4;
    let mut data = vec![0; start];
    put16(&mut data, 0, 1);
    put16(&mut data, 2, 28);
    put32(&mut data, 8, strings.len() as u32);
    put32(&mut data, 16, 0x100);
    put32(&mut data, 20, start as u32);
    for (index, text) in strings.iter().enumerate() {
        let offset = (data.len() - start) as u32;
        put32(&mut data, 28 + index * 4, offset);
        data.push(text.len() as u8);
        data.push(text.len() as u8);
        data.extend(text.as_bytes());
        data.push(0);
    }
    while data.len() % 4 != 0 {
        data.push(0);
    }
    let size = data.len() as u32;
    put32(&mut data, 4, size);
    data
}
fn table(package: u32, offset: u32, legacy: bool, raw_type: u8) -> Vec<u8> {
    let global = pool(&["actual value"]);
    let types = pool(&["string"]);
    let keys = pool(&["owned", "other"]);
    let header = if legacy { 284 } else { 288 };
    let mut pkg = vec![0; header];
    put16(&mut pkg, 0, 0x200);
    put16(&mut pkg, 2, header as u16);
    put32(&mut pkg, 8, package);
    for (index, unit) in "table.owner".encode_utf16().enumerate() {
        put16(&mut pkg, 12 + 2 * index, unit);
    }
    put32(&mut pkg, 268, header as u32);
    put32(&mut pkg, 276, (header + types.len()) as u32);
    if !legacy {
        put32(&mut pkg, 284, offset);
    }
    pkg.extend(types);
    pkg.extend(keys);
    let mut ty = vec![0; 44];
    put16(&mut ty, 0, 0x201);
    put16(&mut ty, 2, 24);
    put32(&mut ty, 4, 44);
    ty[8] = raw_type;
    put32(&mut ty, 12, 1);
    put32(&mut ty, 16, 28);
    put32(&mut ty, 20, 4);
    put16(&mut ty, 28, 8);
    put32(&mut ty, 32, 0);
    put16(&mut ty, 36, 8);
    ty[39] = 3;
    pkg.extend(ty);
    let size = pkg.len() as u32;
    put32(&mut pkg, 4, size);
    let mut all = vec![0; 12];
    put16(&mut all, 0, 2);
    put16(&mut all, 2, 12);
    put32(&mut all, 8, 1);
    all.extend(global);
    all.extend(pkg);
    let size = all.len() as u32;
    put32(&mut all, 4, size);
    all
}
#[test]
fn zero_and_nonzero_type_offsets_preserve_actual_table_identity() {
    for offset in [0, 2, 100, 254] {
        let arsc = Arsc::parse(&table(0x7f, offset, false, 1)).unwrap();
        let id = 0x7f000000 | ((offset + 1) << 16);
        assert_eq!(
            arsc.resource_name(id),
            Some(ResourceName {
                package: "table.owner",
                resource_type: "string",
                entry: "owned"
            })
        );
        assert_eq!(arsc.resolve_name(id).as_deref(), Some("@string/owned"));
        assert_eq!(arsc.resolve_string(id).as_deref(), Some("actual value"));
        if offset != 0 {
            assert_eq!(arsc.resource_name(0x7f010000), None);
        }
    }
}
#[test]
fn legacy_header_defaults_offset_to_zero_and_high_package_ids_are_real() {
    let arsc = Arsc::parse(&table(0x80, 200, true, 1)).unwrap();
    assert_eq!(arsc.packages[0].type_id_offset, 0);
    assert_eq!(
        arsc.resource_name(0x80010000).unwrap().package,
        "table.owner"
    );
    assert_eq!(arsc.resource_name(0x7f010000), None);
}
#[test]
fn type_package_and_entry_index_overflow_are_rejected_instead_of_masked() {
    for (package, offset, ty) in [(0x7f, 255, 1), (0x7f, 256, 1), (0x17f, 0, 1), (0x7f, 0, 0)] {
        assert!(
            Arsc::parse(&table(package, offset, false, ty)).is_err(),
            "{package:x}/{offset}/{ty}"
        );
    }
    let mut data = table(0x7f, 0, false, 1);
    let at = data.len() - 44;
    put32(&mut data, at + 12, 65537);
    assert!(Arsc::parse(&data).is_err());
}
#[test]
fn unknown_ids_missing_keys_and_ambiguous_packages_do_not_invent_names() {
    let arsc = Arsc::parse(&table(0x7f, 0, false, 1)).unwrap();
    for id in [
        0, 0x7f000000, 0x7f010001, 0x7f020000, 0x80010000, 0xffffffff,
    ] {
        assert_eq!(arsc.resource_name(id), None);
    }
    let mut damaged = arsc.clone();
    damaged.packages[0].entries.get_mut(&0x7f010000).unwrap()[0].key_idx = 99;
    assert_eq!(damaged.resource_name(0x7f010000), None);
    let mut inconsistent = arsc.clone();
    let mut variant = inconsistent.packages[0].entries[&0x7f010000][0].clone();
    variant.key_idx = 1;
    inconsistent.packages[0]
        .entries
        .get_mut(&0x7f010000)
        .unwrap()
        .push(variant);
    assert_eq!(inconsistent.resource_name(0x7f010000), None);
    let mut duplicate = arsc.clone();
    duplicate.packages.push(duplicate.packages[0].clone());
    assert_eq!(
        duplicate.resource_name(0x7f010000),
        arsc.resource_name(0x7f010000)
    );
    duplicate.packages[1].name = "different.owner".into();
    assert_eq!(duplicate.resource_name(0x7f010000), None);
}
#[test]
fn string_pools_cannot_escape_the_owning_package_chunk() {
    let mut data = table(0x7f, 0, false, 1);
    let global_len = pool(&["actual value"]).len();
    let base = 12 + global_len;
    let package_size = u32::from_le_bytes(data[base + 4..base + 8].try_into().unwrap());
    put32(&mut data, base + 268, package_size - 4);
    assert!(Arsc::parse(&data).is_err());
}

fn owned() -> ResourceName<'static> {
    ResourceName {
        package: "table.owner",
        resource_type: "string",
        entry: "owned",
    }
}
#[test]
fn reverse_lookup_uses_package_fragments_type_offset_and_high_ids() {
    for (package, offset) in [(0x7f, 0), (0x80, 2), (0xfe, 254)] {
        let mut arsc = Arsc::parse(&table(package, offset, false, 1)).unwrap();
        let id = package << 24 | ((offset + 1) << 16);
        assert_eq!(arsc.find_identifier(owned()).unwrap(), Some(id));
        arsc.packages.push(arsc.packages[0].clone());
        assert_eq!(arsc.find_identifier(owned()).unwrap(), Some(id));
        for query in [
            ResourceName {
                package: "manifest.only",
                ..owned()
            },
            ResourceName {
                resource_type: "integer",
                ..owned()
            },
            ResourceName {
                entry: "absent",
                ..owned()
            },
        ] {
            assert_eq!(arsc.find_identifier(query).unwrap(), None);
        }
    }
}
#[test]
fn reverse_lookup_distinguishes_absence_from_malformed_or_ambiguous_identity() {
    let original = Arsc::parse(&table(0x7f, 0, false, 1)).unwrap();
    let mut duplicate = original.clone();
    let entry = duplicate.packages[0].entries[&0x7f010000].clone();
    duplicate.packages[0].entries.insert(0x7f010001, entry);
    assert_eq!(
        duplicate.find_identifier(owned()).unwrap_err().code,
        "AMBIGUOUS_RESOURCE_NAME"
    );
    duplicate.packages[0].entries.get_mut(&0x7f010001).unwrap()[0].key_idx = 1;
    assert_eq!(
        duplicate.find_identifier(owned()).unwrap(),
        Some(0x7f010000)
    );
    assert_eq!(
        duplicate
            .find_identifier(ResourceName {
                entry: "other",
                ..owned()
            })
            .unwrap(),
        Some(0x7f010001)
    );
    let mut damaged = original.clone();
    damaged.packages[0].entries.get_mut(&0x7f010000).unwrap()[0].key_idx = 99;
    assert!(damaged.find_identifier(owned()).is_err());
    let mut conflict = original.clone();
    conflict.packages.push(conflict.packages[0].clone());
    conflict.packages[1].name = "different.owner".into();
    assert_eq!(
        conflict.find_identifier(owned()).unwrap_err().code,
        "AMBIGUOUS_RESOURCE_NAME"
    );
}
#[test]
fn reverse_name_syntax_requires_real_defaults_and_preserves_explicit_components() {
    for name in [
        "table.owner:string/owned",
        "@table.owner:string/owned",
        "@*table.owner:string/owned",
    ] {
        assert_eq!(identifier_name(name, None, None), Some(owned()));
        assert_eq!(
            identifier_name(name, Some("wrong"), Some("wrong")),
            Some(owned())
        );
    }
    assert_eq!(
        identifier_name("string/owned", None, Some("table.owner")),
        Some(owned())
    );
    assert_eq!(
        identifier_name("table.owner:owned", Some("string"), None),
        Some(owned())
    );
    assert_eq!(
        identifier_name("owned", Some("string"), Some("table.owner")),
        Some(owned())
    );
    for (name, kind, package) in [
        ("", Some("string"), Some("table.owner")),
        ("owned", None, Some("table.owner")),
        ("string/owned", None, None),
        ("owned", Some("string"), None),
        (":string/owned", None, None),
        ("table.owner:/owned", None, None),
        ("table.owner:string/", None, None),
    ] {
        assert_eq!(identifier_name(name, kind, package), None, "{name}");
    }
}
#[test]
fn numeric_names_are_signed_decimal_raw_ids_with_checked_overflow() {
    for (text, expected) in [
        ("0", 0),
        ("+123", 123),
        ("-1", -1),
        ("2147483647", i32::MAX),
        ("-2147483648", i32::MIN),
        ("0002147483647", i32::MAX),
        ("\u{0661}\u{0662}\u{0663}", 123),
        ("\u{ff11}\u{ff12}\u{ff13}", 123),
    ] {
        assert_eq!(identifier_number(text), Some(expected));
    }
    for text in [
        "2147483648",
        "-2147483649",
        "0x7f010000",
        " 123",
        "123 ",
        "+",
        "-",
        "",
        "1_000",
        "\u{00b2}",
        "\u{1d7d9}",
    ] {
        assert_eq!(identifier_number(text), None, "{text}");
    }
}
