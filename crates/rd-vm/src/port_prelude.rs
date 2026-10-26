//! Explicit core/alloc prelude for inherited framework modules.
pub(crate) use alloc::{
    borrow::ToOwned,
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

#[allow(unused_imports)]
// Host dependencies may expose inherent std methods; Horizon uses this trait.
pub(crate) use crate::port_float::FloatExt;
