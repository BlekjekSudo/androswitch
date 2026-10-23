//! Contrato de erro da VM (mesma forma {code, cause, suggestion, module_id}
//! do projeto — Lei 1: falha sempre tipada, nunca pânico, nunca silêncio).

use alloc::{
    format,
    string::{String, ToString},
};
pub use rd_dex::RdError;

pub const MODULE_ID: &str = "rd-vm";

/// RdError com o `module_id` da VM e código arbitrário (o construtor do
/// rd-dex fixa códigos de parser; a VM tem o próprio conjunto).
pub fn vm_error(code: &str, cause: impl Into<String>) -> RdError {
    let mut e = RdError::parse(cause);
    e.code = code.to_string();
    e.module_id = MODULE_ID.to_string();
    e
}

pub fn with_suggestion(mut e: RdError, s: impl Into<String>) -> RdError {
    e.suggestion = Some(s.into());
    e
}

/// Recurso fora do escopo do M2 — sempre com pista do que usar.
pub fn not_implemented(what: impl core::fmt::Display) -> RdError {
    with_suggestion(
        vm_error(
            "NOT_IMPLEMENTED",
            format!("{what} fora do escopo do M2 (interpretador mínimo)"),
        ),
        "ver docs/ROADMAP.md (M3+ ampliam a cobertura da VM)",
    )
}

/// Exceção Java materializada (escape de `throw` sem handler).
#[derive(Debug, Clone)]
pub struct Throwable {
    /// descritor da classe da exceção (`Ljava/lang/ArithmeticException;`)
    pub class: String,
    pub message: Option<String>,
    /// referência de heap quando a exceção foi construída como objeto
    pub obj: Option<crate::heap::ObjRef>,
}

impl Throwable {
    pub fn new(class: &str, message: impl Into<String>) -> Self {
        Throwable {
            class: class.to_string(),
            message: Some(message.into()),
            obj: None,
        }
    }

    pub fn name_without_l(&self) -> String {
        self.class
            .trim_start_matches('L')
            .trim_end_matches(';')
            .replace('/', ".")
    }
}

/// Saída de uma execução: valor Java ou exceção escapando.
#[derive(Debug)]
pub enum VmExit {
    /// exceção Java não capturada (chegou ao topo)
    Exception(Throwable),
    /// erro estruturado da VM (NOT_IMPLEMENTED/VM_OOM/…)
    Error(RdError),
}

impl From<RdError> for VmExit {
    fn from(e: RdError) -> Self {
        VmExit::Error(e)
    }
}

impl From<Throwable> for VmExit {
    fn from(t: Throwable) -> Self {
        VmExit::Exception(t)
    }
}

impl From<String> for VmExit {
    fn from(cause: String) -> Self {
        VmExit::Error(vm_error("VM_TYPE_ERROR", cause))
    }
}

impl From<crate::heap::OomError> for VmExit {
    fn from(e: crate::heap::OomError) -> Self {
        VmExit::Error(vm_error(
            "VM_OOM",
            format!(
                "alocação de {} bytes excede o heap de {} bytes (piso E5)",
                e.requested, e.budget
            ),
        ))
    }
}

/// issue #49: OOM de alocação em site de execução Java é `OutOfMemoryError`
/// THROWABLE (capturável por catch Throwable/Error — JLS), não `VM_OOM`
/// run-killer. Todos os sites de alloc do interpretador usam isto.
pub fn oom_throwable(e: crate::heap::OomError) -> VmExit {
    VmExit::Exception(crate::err::Throwable::new(
        "Ljava/lang/OutOfMemoryError;",
        format!(
            "alocação de {} bytes excede o heap de {} bytes (piso E5)",
            e.requested, e.budget
        ),
    ))
}

/// issue #49: resultado de put_field → VmExit tipado (OOM vira
/// OutOfMemoryError capturável; não-instância é bug interno → VM_TYPE_ERROR).
pub fn put_field_result(res: Result<(), crate::heap::FieldWriteErr>) -> Result<(), VmExit> {
    match res {
        Ok(()) => Ok(()),
        Err(crate::heap::FieldWriteErr::Oom(e)) => Err(oom_throwable(e)),
        Err(crate::heap::FieldWriteErr::NotInstance) => Err(VmExit::Error(vm_error(
            "VM_TYPE_ERROR",
            "escrita de campo em objeto que não é instância",
        ))),
    }
}

impl core::fmt::Display for VmExit {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VmExit::Exception(t) => {
                write!(
                    f,
                    "exception {}: {}",
                    t.name_without_l(),
                    t.message.clone().unwrap_or_default()
                )
            }
            VmExit::Error(e) => write!(f, "{e}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for VmExit {}
