//! Stable engine boundary. The registry contains NO executing adapters.

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
    pub fn adapter(&self, id: &str) -> RdResult<UnavailableAdapter<'_>> {
        let descriptor = self.get(id)?;
        Ok(UnavailableAdapter {
            descriptor,
            disabled_modules: self.info(descriptor)?.disabled_modules,
        })
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
