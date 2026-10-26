//! Host-only oracle rendering of the original portable VM test cases.
//! This package never participates in the Switch runner dependency graph.
#[cfg(feature = "upstream-tests")]
#[path = "../../rd-render/src/lib.rs"]
mod upstream;
#[cfg(feature = "upstream-tests")]
pub use upstream::*;
