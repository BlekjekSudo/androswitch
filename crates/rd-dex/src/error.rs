//! Erros estruturados do rd-dex — mesmo contrato do rd-apk (Apêndice I).
//!
//! Toda falha é um `RdError` serializável em JSON:
//! `{ "code", "cause", "suggestion", "module_id" }` — nunca pânico silencioso (Lei 1).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Erro estruturado do RustyDroid. `code` é estável e consumível por agentes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RdError {
    /// Código estável: `PARSE_ERROR`, `INVALID_FORMAT`, `IO_ERROR`, …
    pub code: String,
    /// Causa técnica em inglês (log/LLM-friendly).
    pub cause: String,
    /// Sugestão acionável (opcional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    /// Crate/módulo responsável (ex.: `rd-dex`).
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

    /// Falha de parsing de binário (DEX cru).
    pub fn parse(cause: impl Into<String>) -> Self {
        RdError::new("PARSE_ERROR", cause, crate::MODULE_ID)
    }

    /// Formato reconhecido mas inválido/inconsistente.
    pub fn invalid_format(cause: impl Into<String>) -> Self {
        RdError::new("INVALID_FORMAT", cause, crate::MODULE_ID)
            .with_suggestion("verify the dex with `rd dex summary` or rebuild the artifact")
    }

    /// Arquivo/entrada ausente dentro do container.
    pub fn missing_entry(name: impl Into<String>) -> Self {
        RdError::new(
            "MISSING_ENTRY",
            format!("entry {:?} not found in container", name.into()),
            crate::MODULE_ID,
        )
    }

    /// Erro de I/O com contexto.
    pub fn io(err: &std::io::Error, context: impl Into<String>) -> Self {
        RdError::new(
            "IO_ERROR",
            format!("{}: {}", context.into(), err),
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
        let e = RdError::parse("bad magic");
        let json = serde_json::to_value(e).unwrap();
        assert_eq!(json["code"], "PARSE_ERROR");
        assert_eq!(json["module_id"], "rd-dex");
    }

    #[test]
    fn display_is_human_readable() {
        let e = RdError::parse("bad magic").with_suggestion("re-download");
        assert!(e.to_string().contains("PARSE_ERROR [rd-dex]"));
        assert!(e.to_string().contains("re-download"));
    }
}
