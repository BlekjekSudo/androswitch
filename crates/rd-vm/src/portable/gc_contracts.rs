//! Pressure/lifetime contracts against the shared original collector.
use crate::{Engine, Value};
use alloc::vec;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::VmConfig;

    #[test]
    fn live_heap_above_half_budget_waits_for_growth_before_recollecting() {
        let mut vm = Engine::new(
            vec![],
            VmConfig {
                heap_budget: 64 * 1024,
                ..Default::default()
            },
        );
        let live = vm.heap.alloc_string("a".repeat(20_000)).unwrap();
        vm.statics
            .insert(("LGcPressure;".into(), "live".into()), Value::Obj(live));
        let frame = vm
            .execution_frames
            .enter(0, "pressure".into(), "LGcPressure;".into(), vec![])
            .unwrap();
        assert!(vm.maybe_gc().is_some());
        assert!(vm.heap.used() > vm.heap.budget() / 2);
        let collections = vm.gc_stats.collections;
        vm.heap.alloc_instance("LSmall;".into(), vec![]).unwrap();
        assert!(
            vm.maybe_gc().is_none(),
            "one small allocation must not recollect the same live heap"
        );
        assert_eq!(vm.gc_stats.collections, collections);
        vm.heap.alloc_string("garbage".repeat(1_000)).unwrap();
        assert!(vm.maybe_gc().is_some(), "real growth still triggers GC");
        assert!(vm.gc_stats.last_freed_objects >= 2);
        assert_eq!(vm.heap.as_str(live).unwrap().len(), 20_000);
        vm.execution_frames.leave(frame);
        vm.statics.clear();
        vm.gc();
        assert_eq!(vm.heap.used(), 0);
    }

    #[test]
    fn an_unpinned_boundary_reclaims_a_heap_retained_by_the_previous_callback() {
        let mut vm = Engine::new(
            vec![],
            VmConfig {
                heap_budget: 64 * 1024,
                ..Default::default()
            },
        );
        vm.heap.alloc_string("temporary".repeat(2_400)).unwrap();
        let scope = vm.heap.gc_scope_depth();
        vm.heap.gc_host_enter(true);
        let frame = vm
            .execution_frames
            .enter(0, "callback".into(), "LCallback;".into(), vec![])
            .unwrap();
        vm.gc();
        assert!(vm.heap.used() > vm.heap.budget() / 2);
        vm.heap.alloc_instance("LSmall;".into(), vec![]).unwrap();
        assert!(vm.maybe_gc().is_none());
        vm.execution_frames.leave(frame);
        vm.heap.gc_scope_restore(scope);
        assert!(
            vm.maybe_gc().is_some(),
            "unwinding the old callback must invalidate its pinned live watermark"
        );
        assert_eq!(vm.heap.used(), 0);
        assert!(vm.maybe_gc().is_none());
    }
}
