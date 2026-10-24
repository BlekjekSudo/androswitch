//! Shared load policy. Device callbacks report allocator state; host defaults
//! are explicit policy ceilings, never a claim about free physical memory.
use rd_vm::{err::vm_error, VmExit};
pub const MIB: usize = 1024 * 1024;
#[derive(Clone, Copy)]
pub struct LoadMemory {
    pub budget: usize,
    pub available: Option<fn() -> usize>,
    pub checkpoint: Option<fn(&'static str)>,
}
impl Default for LoadMemory {
    fn default() -> Self {
        Self {
            budget: 512 * MIB,
            available: None,
            checkpoint: None,
        }
    }
}
impl LoadMemory {
    pub fn parser_budget(self) -> usize {
        (self.budget / 2).min(256 * MIB)
    }
    pub fn raw_dex_budget(self) -> usize {
        (self.parser_budget() / 2).min(128 * MIB)
    }
    pub fn stage(self, stage: &'static str) -> Result<(), VmExit> {
        if let Some(log) = self.checkpoint {
            log(stage);
        }
        if self.available.is_some_and(|free| free() < 64 * MIB) {
            return Err(vm_error(
                "RUNTIME_MEMORY_RESERVE",
                "load needs at least 64 MiB allocator headroom for VM/framework/native work",
            )
            .into());
        }
        Ok(())
    }
}
#[derive(Debug, Default, Clone, Copy)]
pub struct LoadMemoryReport {
    pub raw_dex_bytes: usize,
    pub estimated_parser_bytes: usize,
    pub parser_budget_bytes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    #[test]
    fn admission_tracks_capacity_and_preserves_a_runtime_reserve() {
        let default = LoadMemory::default();
        assert!(default.raw_dex_budget() > 33 * MIB);
        assert_eq!(default.parser_budget(), 256 * MIB);
        let small = LoadMemory {
            budget: 64 * MIB,
            ..default
        };
        assert_eq!(small.raw_dex_budget(), 16 * MIB);
        let huge = LoadMemory {
            budget: usize::MAX,
            ..default
        };
        assert_eq!(huge.raw_dex_budget(), 128 * MIB);
        let depleted = LoadMemory {
            available: Some(|| 63 * MIB),
            ..default
        };
        assert!(depleted
            .stage("unit")
            .unwrap_err()
            .to_string()
            .contains("RUNTIME_MEMORY_RESERVE"));
        assert!(LoadMemory {
            available: Some(|| 64 * MIB),
            ..default
        }
        .stage("unit")
        .is_ok());
    }
}
