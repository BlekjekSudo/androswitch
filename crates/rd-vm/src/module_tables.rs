//! Module-local tables belong to one Engine and cannot leak across relaunches.
pub(crate) use crate::local_table::Local;
use crate::{android_spannable, android_uri, juuid, xml_res};
use crate::{
    heap::ObjRef,
    maps::{HashMap, HashSet},
};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

pub(crate) struct ModuleTables {
    pub(crate) android_spannable_states: Local<RefCell<HashMap<ObjRef, android_spannable::State>>>,
    pub(crate) android_uri_builders: Local<RefCell<HashMap<ObjRef, android_uri::B>>>,
    pub(crate) jannotations_instances: Local<RefCell<HashSet<ObjRef>>>,
    pub(crate) jrandom_seeds: Local<RefCell<HashMap<ObjRef, u64>>>,
    pub(crate) jrandom_uniq: Local<RefCell<u64>>,
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
            jrandom_seeds: Local::new(RefCell::new(HashMap::new())),
            jrandom_uniq: Local::new(RefCell::new(0)),
            juuid_rng: Local::new(Cell::new(juuid::initial_seed())),
            xml_res_parsers: Local::new(RefCell::new(HashMap::new())),
            xml_res_arrays: Local::new(RefCell::new(HashMap::new())),
        }
    }
}
