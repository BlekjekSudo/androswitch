//! Engine boundary: three opt-in bounded native adapters; other routes fail honestly.

use crate::{EngineDescriptor, Registry};
use rd_apk::{RdError, RdResult};

#[derive(Debug, Clone, Copy)]
pub struct Artifact<'a> {
    pub name: &'a str,
    pub bytes: &'a [u8],
}

#[derive(Debug, Clone, Copy)]
pub struct EngineEvent<'a> {
    pub name: &'a str,
    pub payload: &'a [u8],
}

#[derive(Debug, Clone, Copy)]
pub struct EngineSyscall<'a> {
    pub name: &'a str,
    pub args: &'a [u64],
}

/// A future `rd-engine-<name>` must implement this boundary and its golden
/// runtime flow. Detection goldens alone never satisfy the execution DoD.
pub trait EngineAdapter {
    type Process;
    fn descriptor(&self) -> &EngineDescriptor;
    fn parse(&self, artifact: Artifact<'_>) -> RdResult<Self::Process>;
    fn event(&self, process: &mut Self::Process, event: EngineEvent<'_>) -> RdResult<()>;
    fn syscall(&self, process: &mut Self::Process, call: EngineSyscall<'_>) -> RdResult<()>;
}

#[derive(Debug)]
pub struct UnavailableAdapter<'a> {
    descriptor: &'a EngineDescriptor,
    disabled_modules: Vec<String>,
}

impl Registry {
    pub fn adapter(&self, id: &str) -> RdResult<Adapter<'_>> {
        let descriptor = self.get(id)?;
        #[cfg(feature = "native-execution")]
        if crate::native_execution_available(id) {
            return Ok(Adapter::Native(descriptor));
        }
        Ok(Adapter::Unavailable(UnavailableAdapter {
            descriptor,
            disabled_modules: self.info(descriptor)?.disabled_modules,
        }))
    }
}

impl UnavailableAdapter<'_> {
    fn error(&self, operation: &str) -> RdError {
        let e = self.descriptor;
        let gates = if self.disabled_modules.is_empty() {
            "features selected, but no executing registry adapter is implemented".into()
        } else {
            format!(
                "disabled modules: {}; enabling them does not implement a runtime",
                self.disabled_modules.join(", ")
            )
        };
        RdError::new(
            "NOT_IMPLEMENTED",
            format!("engine {}: {operation} unavailable ({gates}); no process was created", e.id),
            &e.modules[0],
        ).with_suggestion(format!(
            "see docs/engine-registry.md; planned/existing route: {}; existing DEX subset uses rd vm/app, not this adapter",
            e.adapter_crate
        ))
    }
}

impl EngineAdapter for UnavailableAdapter<'_> {
    /// A placeholder type for the contract only: parse NEVER returns a value.
    type Process = ();
    fn descriptor(&self) -> &EngineDescriptor {
        self.descriptor
    }
    fn parse(&self, _artifact: Artifact<'_>) -> RdResult<Self::Process> {
        Err(self.error("parse"))
    }
    fn event(&self, _process: &mut Self::Process, _event: EngineEvent<'_>) -> RdResult<()> {
        Err(self.error("event"))
    }
    fn syscall(&self, _process: &mut Self::Process, _call: EngineSyscall<'_>) -> RdResult<()> {
        Err(self.error("syscall"))
    }
}

/// Feature-gated real native route, or the explicit unavailable contract.
#[derive(Debug)]
pub enum Adapter<'a> {
    Unavailable(UnavailableAdapter<'a>),
    #[cfg(feature = "native-execution")]
    Native(&'a EngineDescriptor),
}
#[derive(Debug)]
pub enum AdapterProcess {
    /// Sentinel for negative-contract tests; no unavailable parse creates it.
    Unavailable,
    #[cfg(feature = "native-execution")]
    Native(Box<crate::native_execution::NativeProcess>),
}
impl AdapterProcess {
    pub fn report(&self) -> RdResult<serde_report::Report> {
        match self {
            Self::Unavailable => Err(RdError::new(
                "NOT_IMPLEMENTED",
                "no process exists",
                crate::MODULE_ID,
            )),
            #[cfg(feature = "native-execution")]
            Self::Native(p) => Ok(p.report()),
        }
    }
}
// Keep minimal builds independent from serde_json/VM.
#[cfg(feature = "native-execution")]
mod serde_report {
    pub type Report = serde_json::Value;
}
#[cfg(not(feature = "native-execution"))]
mod serde_report {
    pub type Report = ();
}
impl EngineAdapter for Adapter<'_> {
    type Process = AdapterProcess;
    fn descriptor(&self) -> &EngineDescriptor {
        match self {
            Self::Unavailable(a) => a.descriptor(),
            #[cfg(feature = "native-execution")]
            Self::Native(d) => d,
        }
    }
    fn parse(&self, artifact: Artifact<'_>) -> RdResult<Self::Process> {
        match self {
            Self::Unavailable(a) => a.parse(artifact).map(|()| AdapterProcess::Unavailable),
            #[cfg(feature = "native-execution")]
            Self::Native(d) => crate::native_execution::NativeProcess::parse(&d.id, artifact)
                .map(|p| AdapterProcess::Native(Box::new(p))),
        }
    }
    fn event(&self, process: &mut Self::Process, event: EngineEvent<'_>) -> RdResult<()> {
        let _ = &process;
        match self {
            Self::Unavailable(a) => a.event(&mut (), event),
            #[cfg(feature = "native-execution")]
            Self::Native(d) => match process {
                AdapterProcess::Native(p) if p.engine_id() == d.id => p.event(event),
                _ => Err(
                    RdError::new("INVALID_PROCESS", "process/engine mismatch", &d.modules[0])
                        .with_suggestion(
                            "pass the process returned by parse on this same engine adapter",
                        ),
                ),
            },
        }
    }
    fn syscall(&self, process: &mut Self::Process, call: EngineSyscall<'_>) -> RdResult<()> {
        let _ = &process;
        match self {
            Self::Unavailable(a) => a.syscall(&mut (), call),
            #[cfg(feature = "native-execution")]
            Self::Native(d) => match process {
                AdapterProcess::Native(p) if p.engine_id() == d.id => p.syscall(call),
                _ => Err(
                    RdError::new("INVALID_PROCESS", "process/engine mismatch", &d.modules[0])
                        .with_suggestion(
                            "pass the process returned by parse on this same engine adapter",
                        ),
                ),
            },
        }
    }
}
