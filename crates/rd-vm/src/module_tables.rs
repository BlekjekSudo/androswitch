//! Module-local tables belong to one Engine and cannot leak across relaunches.
use crate::{android_spannable, android_uri, jio_chars, jregex, juuid, xml_res};
use crate::{
    heap::ObjRef,
    maps::{HashMap, HashSet},
};
use alloc::{rc::Rc, string::String, vec::Vec};
use core::cell::{Cell, RefCell};
pub(crate) struct Local<T>(Rc<T>);
impl<T> Clone for Local<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Local<T> {
    pub(crate) fn new(value: T) -> Self {
        Self(Rc::new(value))
    }
    pub(crate) fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&self.0)
    }
}
type CompiledPatterns = HashMap<(String, i32, u8), fancy_regex::Regex>;

pub(crate) struct ModuleTables {
    pub(crate) android_spannable_states: Local<RefCell<HashMap<ObjRef, android_spannable::State>>>,
    pub(crate) android_uri_builders: Local<RefCell<HashMap<ObjRef, android_uri::B>>>,
    pub(crate) jannotations_instances: Local<RefCell<HashSet<ObjRef>>>,
    pub(crate) jio_chars_writers_st: Local<RefCell<HashMap<ObjRef, jio_chars::WState>>>,
    pub(crate) jio_chars_readers_st: Local<RefCell<HashMap<ObjRef, jio_chars::RState>>>,
    pub(crate) jio_chars_bytes_st: Local<RefCell<HashMap<ObjRef, jio_chars::BState>>>,
    pub(crate) jrandom_seeds: Local<RefCell<HashMap<ObjRef, u64>>>,
    pub(crate) jrandom_uniq: Local<RefCell<u64>>,
    pub(crate) jregex_cache: Local<RefCell<CompiledPatterns>>,
    pub(crate) jregex_matchers: Local<RefCell<HashMap<ObjRef, jregex::MState>>>,
    pub(crate) juuid_rng: Local<Cell<u64>>,
    pub(crate) xml_res_parsers: Local<RefCell<HashMap<ObjRef, xml_res::ParserState>>>,
    pub(crate) xml_res_arrays:
        Local<RefCell<HashMap<ObjRef, Vec<Option<rd_apk::axml::XmlAttribute>>>>>,
}
impl Default for ModuleTables {
    fn default() -> Self {
        Self {
            android_spannable_states: Local::new(RefCell::new(HashMap::new())),
            android_uri_builders: Local::new(RefCell::new(HashMap::new())),
            jannotations_instances: Local::new(RefCell::new(HashSet::new())),
            jio_chars_writers_st: Local::new(RefCell::new(HashMap::new())),
            jio_chars_readers_st: Local::new(RefCell::new(HashMap::new())),
            jio_chars_bytes_st: Local::new(RefCell::new(HashMap::new())),
            jrandom_seeds: Local::new(RefCell::new(HashMap::new())),
            jrandom_uniq: Local::new(RefCell::new(0)),
            jregex_cache: Local::new(RefCell::new(HashMap::new())),
            jregex_matchers: Local::new(RefCell::new(HashMap::new())),
            juuid_rng: Local::new(Cell::new(juuid::initial_seed())),
            xml_res_parsers: Local::new(RefCell::new(HashMap::new())),
            xml_res_arrays: Local::new(RefCell::new(HashMap::new())),
        }
    }
}
