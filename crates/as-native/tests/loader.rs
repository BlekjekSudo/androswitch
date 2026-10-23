use as_native::{Allocation, CallFrame, Loader, NativeBackend, Result};
use std::{collections::BTreeMap, fs, path::PathBuf};
#[derive(Default)]
struct Memory {
    next: u64,
    released: Vec<u64>,
    writes: BTreeMap<u64, Vec<u8>>,
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
    fn import(&mut self, _: &str, name: &str) -> Result<Option<u64>> {
        Ok(matches!(name, "malloc" | "free" | "memset" | "strlen").then_some(0x70000000))
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
    let image = &loader.modules[1].image;
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
