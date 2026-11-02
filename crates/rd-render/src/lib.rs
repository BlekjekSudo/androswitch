//! rd-render — M4: UI dump uiautomator + screenshot headless (software
//! renderer determinístico + PNG sem deps). Os renderers winit/web continuam
//! stub (M11): toda tentativa de uso responde `NOT_IMPLEMENTED` com o
//! contrato {code, cause, suggestion, module_id} (Lei 1).

use serde::{Deserialize, Serialize};

pub mod shot;
pub mod ui_dump;

pub use shot::{render_snapshot, render_snapshot_sized, Framebuffer, VIEW_H, VIEW_W};
pub use ui_dump::uiautomator_xml;

pub const MODULE_ID: &str = "rd-render";
/// Renderers de janela/web (winit/webcanvas) — outro milestone.
pub const MILESTONE: &str = "M11";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RdError {
    pub code: String,
    pub cause: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    pub module_id: String,
}

impl RdError {
    pub fn not_implemented() -> Self {
        RdError {
            code: "NOT_IMPLEMENTED".into(),
            cause: format!("{MODULE_ID} lands in milestone {MILESTONE}"),
            suggestion: Some("track docs/ROADMAP.md and the compat-matrix".into()),
            module_id: MODULE_ID.into(),
        }
    }
}

impl std::fmt::Display for RdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} [{}]: {}", self.code, self.module_id, self.cause)
    }
}

impl std::error::Error for RdError {}

pub fn not_implemented<T>() -> Result<T, RdError> {
    Err(RdError::not_implemented())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_contract_is_stable() {
        let e = RdError::not_implemented();
        assert_eq!(e.code, "NOT_IMPLEMENTED");
        assert_eq!(e.module_id, MODULE_ID);
        assert_eq!(e.module_id, "rd-render");
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["code"], "NOT_IMPLEMENTED");
        assert_eq!(json["module_id"], "rd-render");
    }
}
