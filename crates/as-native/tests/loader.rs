use as_native::{Allocation, CallFrame, Loader, NativeBackend, Result};
use std::{collections::BTreeMap, fs, path::PathBuf};
#[derive(Default)]
struct Memory {
    next: u64,
    released: Vec<u64>,
    writes: BTreeMap<u64, Vec<u8>>,
    imports: Vec<(String, u8)>,
    missing: std::collections::BTreeSet<String>,
    defer: bool,
    deferred: Vec<as_native::DeferredImport>,
}
impl NativeBackend for Memory {
    fn allocate(&mut self, length: usize) -> Result<Allocation> {
        self.next += 0x100000;
        Ok(Allocation {
            address: self.next,
            token: self.next,
            length,
        })
    }
    fn write(&mut self, m: Allocation, _: usize, bytes: &[u8]) -> Result<()> {
        self.writes.insert(m.token, bytes.to_vec());
        Ok(())
    }
    fn finalize(&mut self, _: Allocation, pages: &[u8]) -> Result<()> {
        assert!(pages.contains(&1));
        Ok(())
    }
    fn release(&mut self, m: Allocation) {
        self.released.push(m.token);
    }
    fn import(&mut self, _: &str, name: &str, kind: u8) -> Result<Option<u64>> {
        self.imports.push((name.into(), kind));
        if self.missing.contains(name) {
            return Ok(None);
        }
        Ok(matches!(
            name,
            "malloc" | "free" | "memset" | "strlen" | "data_object" | "function_import"
        )
        .then_some(0x70000000))
    }
    fn defer_function(&mut self, library: &str, symbol: &str) -> Result<Option<u64>> {
        if !self.defer {
            return Ok(None);
        }
        self.deferred.push(as_native::DeferredImport {
            library: library.into(),
            symbol: symbol.into(),
        });
        Ok(Some(0x71000000))
    }
    fn deferred_imports(&self) -> Vec<as_native::DeferredImport> {
        self.deferred.clone()
    }
    fn tls(&mut self, _: usize, bytes: &[u8], size: usize, alignment: usize) -> Result<u64> {
        assert!(size >= bytes.len());
        assert!(alignment.is_power_of_two());
        Ok(0x100)
    }
    fn tls_descriptor(&mut self, offset: u64) -> Result<(u64, u64)> {
        Ok((0x70001000, offset))
    }
}
fn fixtures() -> BTreeMap<String, Vec<u8>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local-artifacts/m3/fixture");
    ["libarithmetic.so", "libcalculation.so", "libthreadstate.so"]
        .iter()
        .map(|name| {
            (
                name.to_string(),
                fs::read(root.join(name)).expect("run scripts/build-m3-fixture.ps1"),
            )
        })
        .collect()
}
#[test]
#[ignore = "requires rebuilt NDK fixtures"]
fn stripped_dependencies_relocations_constructors_and_tls_use_one_loader() {
    let sources = fixtures();
    for bytes in sources.values() {
        assert_eq!(&bytes[40..48], &[0; 8]);
        assert_eq!(&bytes[58..64], &[0; 6]);
    }
    let mut loader = Loader::new(Memory::default(), sources);
    let i = loader.load("libarithmetic.so").unwrap();
    assert_eq!(i, 1);
    assert_eq!(loader.modules[0].name, "libcalculation.so");
    assert_eq!(loader.modules[0].constructors.len(), 1);
    assert_eq!(loader.modules[1].constructors.len(), 1);
    let dep = loader.symbol("dependency_value").unwrap();
    let image = &loader.backend.writes[&loader.modules[1].memory.token];
    assert!(loader.modules.iter().all(|module| module.image.is_empty()
        && module
            .elf
            .segments
            .iter()
            .all(|segment| segment.bytes.is_empty())));
    assert!(image
        .chunks_exact(8)
        .any(|bytes| u64::from_le_bytes(bytes.try_into().unwrap()) == dep));
    let t = loader.load("libthreadstate.so").unwrap();
    assert_eq!(loader.modules[t].tls_offset, Some(256));
    assert_eq!(loader.load("libarithmetic.so").unwrap(), i);
}
#[test]
fn scalar_calling_convention_keeps_integer_float_banks_and_spill_order() {
    let mut frame = CallFrame::default();
    let types = vec!["I".into(); 9]
        .into_iter()
        .chain(vec!["D".into(); 9])
        .collect::<Vec<_>>();
    let values = (0..18).collect::<Vec<_>>();
    frame.arguments(&types, &values).unwrap();
    assert_eq!(frame.integer, [0, 1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(frame.float, [9, 10, 11, 12, 13, 14, 15, 16]);
    assert_eq!(&frame.stack[..2], &[8, 17]);
    assert!(frame.arguments(&["I".into()], &[]).is_err());
}

#[test]
#[ignore = "requires rebuilt NDK fixtures"]
fn library_provider_extracts_only_requested_dependency_graph() {
    struct Source {
        files: BTreeMap<String, Vec<u8>>,
        reads: std::cell::RefCell<Vec<String>>,
    }
    impl as_native::LibrarySource for Source {
        fn contains(&self, name: &str) -> bool {
            self.files.contains_key(name)
        }
        fn read(&self, name: &str) -> Result<Vec<u8>> {
            self.reads.borrow_mut().push(name.into());
            self.files
                .get(name)
                .cloned()
                .ok_or_else(|| as_native::error("LIBRARY_NOT_FOUND", "test", name, "fixture"))
        }
    }
    let source = std::rc::Rc::new(Source {
        files: fixtures(),
        reads: Default::default(),
    });
    let mut loader = Loader::with_source(Memory::default(), source.clone());
    assert!(source.reads.borrow().is_empty());
    loader.load("libarithmetic.so").unwrap();
    assert_eq!(
        &*source.reads.borrow(),
        &["libarithmetic.so", "libcalculation.so"]
    );
    loader.load("libarithmetic.so").unwrap();
    assert_eq!(source.reads.borrow().len(), 2);
    assert!(loader
        .modules
        .iter()
        .all(|module| module.name != "libthreadstate.so"));
}
#[test]
#[ignore = "requires rebuilt NDK fixtures"]
fn missing_dependency_and_symbol_have_required_names() {
    let mut sources = fixtures();
    sources.remove("libcalculation.so");
    let mut loader = Loader::new(Memory::default(), sources);
    let e = loader.load("libarithmetic.so").unwrap_err();
    assert_eq!(e.code, "LIBRARY_NOT_FOUND");
    assert_eq!(e.library, "libarithmetic.so");
    assert_eq!(e.required, "libcalculation.so");
    let e = loader.load("does-not-exist.so").unwrap_err();
    assert_eq!(e.code, "LIBRARY_NOT_FOUND");
}

#[test]
#[ignore = "requires scripts/test-native-abi.ps1 NDK fixtures"]
fn symbol_kinds_and_instruction_shaped_data_use_validated_elf_metadata() {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local-artifacts/m9/native-abi");
    let bytes = fs::read(root.join("libliteral.so")).expect("run scripts/test-native-abi.ps1");
    let elf = rd_ndk::elf::Elf::parse(&bytes).unwrap();
    assert!(elf.code_ranges.is_some());
    assert!(elf.segments.iter().any(|segment| segment.flags & 1 != 0
        && segment
            .bytes
            .windows(4)
            .any(|word| word == 0xd4000001u32.to_le_bytes())));
    let mut loader = Loader::new(
        Memory::default(),
        BTreeMap::from([("libliteral.so".into(), bytes.clone())]),
    );
    loader.load("libliteral.so").unwrap();
    assert!(loader.backend.imports.contains(&("data_object".into(), 1)));
    assert!(loader
        .backend
        .imports
        .contains(&("function_import".into(), 2)));
    assert!(loader.modules[0].image.is_empty());
    assert_eq!(
        loader.modules[0].destructors,
        vec![
            loader.module_symbol(0, "cleanup_second").unwrap(),
            loader.module_symbol(0, "cleanup_first").unwrap(),
        ]
    );
    let mut stripped = bytes;
    stripped[40..48].fill(0);
    stripped[58..64].fill(0);
    let mut conservative = Loader::new(
        Memory::default(),
        BTreeMap::from([("libliteral.so".into(), stripped)]),
    );
    let error = conservative.load("libliteral.so").unwrap_err();
    assert_eq!(error.code, "LINUX_SYSCALL_UNSUPPORTED");
    let syscall = fs::read(root.join("libsyscall.so")).unwrap();
    let mut forbidden = Loader::new(
        Memory::default(),
        BTreeMap::from([("libsyscall.so".into(), syscall)]),
    );
    assert_eq!(
        forbidden.load("libsyscall.so").unwrap_err().code,
        "LINUX_SYSCALL_UNSUPPORTED"
    );
}

fn change_fixture_symbol(bytes: &mut [u8], name: &str, kind: u8, weak: bool) {
    let elf = rd_ndk::elf::Elf::parse(bytes).unwrap();
    let symbol = elf
        .symbols
        .iter()
        .position(|symbol| symbol.name == name)
        .unwrap();
    let header = u64::from_le_bytes(bytes[40..48].try_into().unwrap()) as usize;
    let stride = u16::from_le_bytes(bytes[58..60].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(bytes[60..62].try_into().unwrap()) as usize;
    for i in 0..count {
        let at = header + i * stride;
        if u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) == 11 {
            let table = u64::from_le_bytes(bytes[at + 24..at + 32].try_into().unwrap()) as usize;
            bytes[table + symbol * 24 + 4] = (if weak { 32 } else { 16 }) | kind;
            return;
        }
    }
    panic!("fixture has no dynamic symbol section");
}
#[test]
#[ignore = "requires scripts/test-native-abi.ps1 NDK fixtures"]
fn lazy_failure_binding_is_explicit_and_never_weak_data_or_ambiguous() {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local-artifacts/m9/native-abi");
    let original = fs::read(root.join("libliteral.so")).unwrap();
    for (kind, weak, defer, missing_data, succeeds, deferred_count) in [
        (2, false, true, false, true, 1),
        (2, false, false, false, false, 0),
        (2, true, true, false, true, 0),
        (0, false, true, false, true, 1), // JUMP_SLOT proves function use.
        (1, false, true, false, false, 0), // Object even at JUMP_SLOT is strict.
        (2, false, true, true, false, 0),
    ] {
        let mut bytes = original.clone();
        change_fixture_symbol(&mut bytes, "function_import", kind, weak);
        let missing = if missing_data {
            ["function_import", "data_object"]
                .into_iter()
                .map(String::from)
                .collect()
        } else {
            ["function_import"].into_iter().map(String::from).collect()
        };
        let backend = Memory {
            missing,
            defer,
            ..Default::default()
        };
        let mut loader = Loader::new(backend, BTreeMap::from([("libarbitrary.so".into(), bytes)]));
        let result = loader.load("libarbitrary.so");
        assert_eq!(
            result.is_ok(),
            succeeds,
            "kind={kind}, weak={weak}, defer={defer}, missing_data={missing_data}"
        );
        if let Err(error) = result {
            assert_eq!(error.code, "SYMBOL_NOT_FOUND");
        }
        assert_eq!(loader.backend.deferred_imports().len(), deferred_count);
        assert!(loader
            .backend
            .deferred_imports()
            .iter()
            .all(|entry| entry.library == "libarbitrary.so" && entry.symbol == "function_import"));
    }
    let mut ambiguous = original;
    change_fixture_symbol(&mut ambiguous, "data_object", 0, false);
    let backend = Memory {
        missing: ["data_object"].into_iter().map(String::from).collect(),
        defer: true,
        ..Default::default()
    };
    let mut loader = Loader::new(backend, BTreeMap::from([("libany.so".into(), ambiguous)]));
    let error = loader.load("libany.so").unwrap_err();
    assert_eq!(error.code, "SYMBOL_NOT_FOUND");
    assert_eq!(error.required, "data_object");
    assert!(loader.backend.deferred_imports().is_empty());
}
