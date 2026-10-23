//! Run explicitly with both default and --no-default-features after extracting
//! the pinned F-Droid classes.dex. The same lookup semantics must hold in both.
use rd_dex::Dex;

#[test]
#[ignore = "requires fixtures/cache/fdroid-classes.dex extracted from the pinned APK"]
fn real_dex_class_index_preserves_definition_lookup() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/cache/fdroid-classes.dex");
    let dex = Dex::parse(std::fs::read(path).expect("required real DEX fixture")).unwrap();
    assert!(dex.verify_checksum());
    assert!(!dex.class_defs.is_empty());
    for def in dex.class_defs.iter().take(100) {
        let descriptor = dex.type_str(def.class_idx);
        let expected = dex
            .class_defs
            .iter()
            .find(|candidate| dex.type_str(candidate.class_idx) == descriptor)
            .unwrap();
        assert!(std::ptr::eq(dex.find_class(descriptor).unwrap(), expected));
    }
    assert!(dex
        .find_class("Lorg/androswitch/DefinitelyMissingClass;")
        .is_none());
}
