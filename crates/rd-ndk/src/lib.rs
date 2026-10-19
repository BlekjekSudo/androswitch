//! Bounded ELF parsing and optional isolated CPU execution (ADR 004).
//! Guest code is never passed to host dlopen or executed in the Rust process.
pub mod elf;
#[cfg(feature = "native")]
pub mod runtime;
use serde::{Deserialize, Serialize};
pub const MODULE_ID: &str = "rd-ndk";
pub const MILESTONE: &str = "M9";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RdError {
    pub code: String,
    pub cause: String,
    pub suggestion: Option<String>,
    pub module_id: String,
}
impl RdError {
    pub fn new(code: &str, cause: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            cause: cause.into(),
            suggestion: Some("see docs/m9.md and ADR 004".into()),
            module_id: MODULE_ID.into(),
        }
    }
    pub fn not_implemented() -> Self {
        Self::new(
            "NOT_IMPLEMENTED",
            "native execution requires the native feature and the isolated worker",
        )
    }
}
impl std::fmt::Display for RdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} [{}]: {}", self.code, self.module_id, self.cause)
    }
}
impl std::error::Error for RdError {}
pub type Result<T> = std::result::Result<T, RdError>;
pub fn not_implemented<T>() -> Result<T> {
    Err(RdError::not_implemented())
}
