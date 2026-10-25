//! Precise full-heap collection for the portable VM's current state graph.
//! Platform side tables are excluded from this build, so roots are statics,
//! active frames, host pins, interned strings and canonical class objects.
use crate::{heap::HeapObj, Engine, ObjRef, Value};
use alloc::{vec, vec::Vec};

pub const FREED_CLASS: &str = "Lrd/gc/Freed;";
#[derive(Debug, Default, Clone, PartialEq)]
pub struct GcStats {
    pub collections: u64,
    pub last_used_after: usize,
    pub last_freed_objects: usize,
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
        self.gc_slots_after = self.heap.len() - self.heap.free_slots();
        self.gc_stats.clone()
    }
    pub(crate) fn gc_due(&self) -> bool {
        self.heap.used() > self.gc_stats.last_used_after
            && self.heap.used()
                >= self
                    .gc_stats
                    .last_used_after
                    .saturating_mul(2)
                    .max(16 << 20)
                    .min(self.heap.budget() / 2)
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
