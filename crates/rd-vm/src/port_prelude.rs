//! Explicit core/alloc prelude for inherited framework modules.
#[allow(unused_imports)] // Each inherited module consumes a different prelude subset.
pub(crate) use alloc::{
    borrow::ToOwned,
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

#[allow(unused_imports)]
#[cfg(feature = "upstream-runtime")]
// Host dependencies may expose inherent std methods; Horizon uses this trait.
pub(crate) use crate::port_float::FloatExt;
