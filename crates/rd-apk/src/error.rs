//! Erros estruturados do RustyDroid — contrato único do projeto (Apêndice I).
//!
//! Toda falha é um `RdError` serializável em JSON:
//! `{ "code", "cause", "suggestion", "module_id" }` — nunca pânico silencioso (Lei 1).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Erro estruturado do RustyDroid. `code` é estável e consumível por agentes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RdError {
    /// Código estável: `NOT_IMPLEMENTED`, `PARSE_ERROR`, `INVALID_FORMAT`, `IO_ERROR`, …
    pub code: String,
    /// Causa técnica em inglês (log/LLM-friendly).
    pub cause: String,
    /// Sugestão acionável (opcional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    /// Crate/módulo responsável (ex.: `rd-apk`).
    pub module_id: String,
}

impl RdError {
    pub fn new(
        code: impl Into<String>,
        cause: impl Into<String>,
        module_id: impl Into<String>,
    ) -> Self {
        RdError {
            code: code.into(),
            cause: cause.into(),
            suggestion: None,
            module_id: module_id.into(),
        }
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    /// Falha de parsing de binário (ZIP/AXML/ARSC/…).
    pub fn parse(cause: impl Into<String>) -> Self {
        RdError::new("PARSE_ERROR", cause, crate::MODULE_ID)
    }

    /// Formato reconhecido mas inválido/inconsistente.
    pub fn invalid_format(cause: impl Into<String>) -> Self {
        RdError::new("INVALID_FORMAT", cause, crate::MODULE_ID)
            .with_suggestion("verify the file with `rd inspect` or regenerate the artifact")
    }

    /// Módulo do checklist ainda não implementado — stub obrigatório (Lei 1).
    pub fn not_implemented(module_id: impl Into<String>, milestone: &str) -> Self {
        let module_id = module_id.into();
        RdError::new(
            "NOT_IMPLEMENTED",
            format!("{module_id} lands in milestone {milestone}"),
            module_id,
        )
        .with_suggestion("track progress in docs/ROADMAP.md and the compat-matrix")
    }

    /// Erro de I/O com contexto.
    pub fn io(err: &std::io::Error, context: impl Into<String>) -> Self {
        RdError::new(
            "IO_ERROR",
            format!("{}: {}", context.into(), err),
            crate::MODULE_ID,
        )
    }

    /// Arquivo/entrada ausente dentro do container.
    pub fn missing_entry(name: impl Into<String>) -> Self {
        RdError::new(
            "MISSING_ENTRY",
            format!("entry {:?} not found in container", name.into()),
            crate::MODULE_ID,
        )
    }
}

impl fmt::Display for RdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}]: {}", self.code, self.module_id, self.cause)?;
        if let Some(s) = &self.suggestion {
            write!(f, " — {s}")?;
        }
        Ok(())
    }
}

impl std::error::Error for RdError {}

impl From<std::io::Error> for RdError {
    fn from(err: std::io::Error) -> Self {
        RdError::io(&err, "I/O failure")
    }
}

pub type RdResult<T> = Result<T, RdError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_to_agent_contract_shape() {
        let e = RdError::not_implemented("rd-dex", "M1");
        let json = serde_json::to_value(e).unwrap();
        assert_eq!(json["code"], "NOT_IMPLEMENTED");
        assert_eq!(json["module_id"], "rd-dex");
        assert!(json["cause"].as_str().unwrap().contains("M1"));
        assert!(json["suggestion"].is_string());
    }

    #[test]
    fn display_is_human_readable() {
        let e = RdError::parse("bad magic").with_suggestion("re-download");
        assert!(e.to_string().contains("PARSE_ERROR [rd-apk]"));
        assert!(e.to_string().contains("re-download"));
    }
}
