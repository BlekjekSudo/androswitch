//! rd-framework — implementação nativa do subconjunto `android.*` por domínio.
//!
//! Domínios (Apêndice C da spec): fd-ui, fd-content, fd-os, fd-net, fd-media,
//! fd-hardware, fd-system-services, fd-permissions. Este crate entrega os dados
//! e motores versionados transversais: **fd-permissions** (Apêndice B),
//! **fd-devices** (perfis/budgets), **fd-behavior** (comutadores por targetSdk),
//! **fd-api-coverage** (registro executável do Apêndice C em
//! `data/api-coverage.toml`), **fd-jetpack** (Apêndice D em
//! `data/jetpack.toml`) e **fd-gms** (Apêndice E em `data/gms.toml`). A maior
//! parte do comportamento `android.*` em tempo de execução vive hoje no host
//! de `rd-vm`; os registros mapeiam cada pacote/classe/biblioteca/serviço
//! para o módulo real e mantêm lacunas `NOT_IMPLEMENTED` explícitas (Lei 1),
//! em vez de fingir que um stub é implementação.

pub mod fd_api_coverage;
pub mod fd_behavior;
pub mod fd_devices;
pub mod fd_gms;
pub mod fd_jetpack;
pub mod fd_permissions;

pub use fd_api_coverage::{
    ApiClassification, ApiCoverageSummary, ApiCoverageTable, ApiDomain, ApiDomainSummary,
    ApiStatus, ApiSurface,
};
pub use fd_behavior::{BehaviorSwitch, SwitchTable};
pub use fd_devices::{DeviceProfile, DeviceTable};
pub use fd_gms::{GmsClassification, GmsService, GmsSummary, GmsTable};
pub use fd_jetpack::{
    JetpackClassification, JetpackLibrary, JetpackStrategy, JetpackSummary, JetpackTable,
};
pub use fd_permissions::{GrantState, PermissionDef, PermissionEngine, PermissionTable};

use rd_apk::RdError;

/// Stub estruturado de qualquer domínio ainda não implementado.
pub fn not_implemented(module_id: &str, milestone: &str) -> RdError {
    RdError::not_implemented(module_id, milestone)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_follows_error_contract() {
        let e = not_implemented("fd-ui", "M3");
        assert_eq!(e.code, "NOT_IMPLEMENTED");
        assert_eq!(e.module_id, "fd-ui");
    }
}
