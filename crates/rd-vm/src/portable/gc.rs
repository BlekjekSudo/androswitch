//! Precise full-heap collection for the portable VM's current state graph.
//! Trace active framework/native owners, shared module tables, statics, actual
//! execution frames, host pins, interned strings and canonical class objects.
use crate::{heap::HeapObj, Engine, ObjRef, Value};
use alloc::{vec, vec::Vec};

pub const FREED_CLASS: &str = "Lrd/gc/Freed;";
#[derive(Debug, Default, Clone, PartialEq)]
pub struct GcStats {
    pub collections: u64,
    pub last_used_after: usize,
    pub last_freed_objects: usize,
    pub last_had_frames: bool,
}

impl Engine {
    pub fn gc(&mut self) -> GcStats {
        let n = self.heap.len();
        let mut marked = vec![false; n + 1];
        let mut roots: Vec<ObjRef> = self.statics.values().filter_map(reference).collect();
        roots.extend(self.boxed_cache.values().copied());
        roots.extend(self.execution_frames.roots());
        roots.extend(self.heap.gc_pinned_roots());
        roots.extend(self.portable.strings.values().copied());
        roots.extend(self.portable.classes.values().copied());
        roots.extend(self.portable.monitors.roots());
        if let Some(framework) = self.framework_host() {
            roots.extend(framework.roots());
        }
        if let Some(native) = self.native_host() {
            roots.extend(native.roots());
        }
        while let Some(object) = roots.pop() {
            if object == 0 || object as usize > n || marked[object as usize] {
                continue;
            }
            marked[object as usize] = true;
            #[cfg(feature = "std")]
            roots.extend(crate::jregex::references(self, object));
            if let Some(framework) = self.framework_host() {
                roots.extend(framework.references(object));
            }
            match self.heap.get(object) {
                Ok(HeapObj::Instance { fields, .. }) => {
                    roots.extend(fields.iter().filter_map(|(_, value)| reference(value)))
                }
                Ok(HeapObj::Array { elems, .. }) => {
                    roots.extend(elems.iter().filter_map(reference))
                }
                _ => {}
            }
        }
        let mut freed = 0;
        self.collection_views.collect(&marked);
        #[cfg(feature = "std")]
        crate::jregex::gc_purge(self, &|r| marked.get(r as usize).copied().unwrap_or(false));
        if let Some(framework) = self.framework_host() {
            framework.collect(&marked);
        }
        for (object, live) in marked.iter().enumerate().skip(1) {
            if !live && self.heap.free_slot(object as ObjRef).is_some() {
                freed += 1;
            }
        }
        self.heap.promote();
        self.portable
            .files
            .retain(|object, _| marked.get(*object as usize).copied().unwrap_or(false));
        self.gc_stats.collections += 1;
        self.gc_stats.last_freed_objects = freed;
        self.gc_stats.last_used_after = self.heap.used();
        self.gc_stats.last_had_frames = !self.execution_frames.is_empty();
        self.gc_slots_after = self.heap.len() - self.heap.free_slots();
        self.gc_stats.clone()
    }
    pub(crate) fn gc_due(&self) -> bool {
        self.unpinned_boundary_due()
            || crate::gc_policy::due(
                self.heap.used(),
                self.heap.budget(),
                self.heap.len() - self.heap.free_slots(),
                self.gc_stats.last_used_after,
                self.gc_slots_after,
                !self.execution_frames.is_empty(),
            )
    }
    fn unpinned_boundary_due(&self) -> bool {
        crate::gc_policy::unpinned_boundary_due(
            self.heap.used(),
            self.heap.budget(),
            self.heap.len() - self.heap.free_slots(),
            self.gc_stats.last_had_frames,
            !self.execution_frames.is_empty(),
        )
    }
    pub fn maybe_gc(&mut self) -> Option<GcStats> {
        self.gc_due().then(|| self.gc())
    }
    pub(crate) fn publish_frame_roots(
        &mut self,
        regs: &[Value],
        result: &Value,
        exception: Option<ObjRef>,
    ) {
        if let Some(roots) = self
            .execution_frames
            .current_roots(self.heap.execution_owner())
        {
            roots.clear();
            roots.extend(
                regs.iter()
                    .chain(core::iter::once(result))
                    .filter_map(reference),
            );
            roots.extend(exception);
        }
    }
    pub fn gc_stats(&self) -> &GcStats {
        &self.gc_stats
    }
}
fn reference(value: &Value) -> Option<ObjRef> {
    if let Value::Obj(object) = value {
        Some(*object)
    } else {
        None
    }
}

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
