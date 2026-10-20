//! Mark-sweep garbage collector for the app heap.
//!
//! Before this, the heap was an arena: nothing was ever freed, and a long
//! session hit the 256 MB budget (the main risk for the Moto E5 / 2 GB floor).
//!
//! Design (safe-point, non-moving, precise heap graph + conservative roots):
//! - **Safe points.** Interpreter frames publish ObjRef roots before calls
//!   and collection; host dispatch pins Rust temporaries across callbacks.
//!   Tool boundaries remain full safe points. See ADR 006 for generations.
//! - **Roots.** Every engine structure outside the heap (statics, the host
//!   framework state, looper queues, threads, choreographer, callbacks,
//!   native/JNI state, module side tables) is scanned *conservatively*: its
//!   `Debug` rendering is tokenized and every integer that is a valid `ObjRef`
//!   keeps that object alive. This is type-agnostic, so a reference stored in
//!   any of the ~40 host side tables cannot be missed; the cost is some
//!   over-retention (an `Int` that happens to equal a live id).
//! - **Host-object table is weak (ephemeron).** `fw.objects` holds the host
//!   half of host-backed objects (collections, views, bundles…). An entry is
//!   traced only when its key object is reachable, and purged when the key is
//!   freed — that is where most garbage lives.
//! - **Heap graph is precise.** Instance fields and array elements are traced
//!   through `Value::Obj`. `WeakReference`/`PhantomReference` (and app
//!   subclasses) do not keep their `referent` alive: it is cleared to null
//!   when nothing else reaches it. `SoftReference` stays strong.
//! - **Non-moving, slots recycled.** A freed slot becomes a tombstone
//!   instance of `Lrd/gc/Freed;` (contents dropped, bytes returned to the
//!   budget) and goes on the heap free list; allocation reuses it (LIFO)
//!   before growing, so the 2^20-slot cap bounds *live* objects, not
//!   allocations. Every side table keyed by `ObjRef` is either a root, an
//!   ephemeron, or purged at sweep, so a recycled slot inherits no state.
use crate::framework::HostObj;
use crate::heap::{HeapObj, ObjRef};
use crate::{Engine, Value};
use std::fmt::Write as _;

/// Class descriptor of a collected slot.
pub const FREED_CLASS: &str = "Lrd/gc/Freed;";

/// Cumulative and last-cycle collector statistics (reported by
/// `runtime_health`).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct GcStats {
    pub collections: u64,
    pub minor_collections: u64,
    pub major_collections: u64,
    pub last_kind: &'static str,
    pub last_traced_objects: usize,
    pub last_live_objects: usize,
    pub last_freed_objects: usize,
    pub last_freed_bytes: usize,
    pub last_used_before: usize,
    pub last_used_after: usize,
    pub last_pause_ms: f64,
    /// WeakReference/PhantomReference referents cleared in the last cycle.
    pub last_cleared_weak: usize,
    /// WeakHashMap entries removed because their keys became unreachable.
    pub last_cleared_weak_keys: usize,
    pub total_freed_objects: u64,
    pub total_freed_bytes: u64,
}

/// Pushes every integer token of `s` that is a valid `ObjRef` (1..=n).
fn scan_refs(s: &str, n: usize, out: &mut Vec<ObjRef>) {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let mut v: u64 = 0;
            while i < b.len() && b[i].is_ascii_digit() {
                v = v.saturating_mul(10).saturating_add(u64::from(b[i] - b'0'));
                i += 1;
            }
            if v >= 1 && v <= n as u64 {
                out.push(v as ObjRef);
            }
        } else {
            i += 1;
        }
    }
}

struct Marker {
    marked: Vec<bool>,
    stack: Vec<ObjRef>,
}

impl Marker {
    fn push(&mut self, r: ObjRef) {
        let i = r as usize;
        if i >= 1 && i < self.marked.len() && !self.marked[i] {
            self.marked[i] = true;
            self.stack.push(r);
        }
    }
}

impl Engine {
    /// Conservative root text: every engine structure outside the heap,
    /// except the weak host-object table (traced per reachable key).
    fn gc_root_text(&mut self) -> String {
        // the parsed APK (zip, arsc, layouts) holds no heap refs: skip it
        let objects = std::mem::take(&mut self.fw.objects);
        let resources = self.fw.resources.take();
        let mut s = String::with_capacity(1 << 16);
        let _ = write!(s, "{:?}", self.fw);
        self.fw.objects = objects;
        self.fw.resources = resources;
        let _ = write!(
            s,
            "|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
            self.statics,
            self.threads,
            self.pending_permission_results,
            self.window_decor,
            self.view_props,
            self.attach_listeners,
            self.attached_views,
            self.window_attached,
            self.window_token,
            self.sparse,
        );
        let _ = write!(
            s,
            "|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
            self.gfx.frame,
            self.vto,
            self.vto_by_root,
            self.frames,
            self.activity_callbacks,
            self.activity_results,
            self.back_dispatchers,
            self.back_callbacks,
            self.intent_meta,
            self.content,
        );
        #[cfg(feature = "native")]
        {
            let _ = write!(s, "|{:?}", self.native);
        }
        s
    }

    /// Full collection. Interpreter frames must have published roots; external
    /// Rust callers must store local references in engine state before calling.
    pub fn gc(&mut self) -> GcStats {
        self.collect_generation(false)
    }

    /// Young-only collection. Old heap writes are remembered; old host
    /// side states are scanned conservatively because they lack barriers.
    pub fn gc_minor(&mut self) -> GcStats {
        self.collect_generation(true)
    }

    fn collect_generation(&mut self, minor: bool) -> GcStats {
        let t0 = std::time::Instant::now();
        let n = self.heap.len();
        let used_before = self.heap.used();
        let mut m = Marker {
            marked: vec![false; n + 1],
            stack: Vec::new(),
        };
        if minor {
            for r in 1..=n as ObjRef {
                if !self.heap.is_young(r) {
                    m.marked[r as usize] = true;
                }
            }
            m.stack.extend(self.heap.remembered());
        }
        for r in self
            .gc_frames
            .iter()
            .flatten()
            .copied()
            .chain(self.heap.gc_pinned_roots())
        {
            m.push(r);
        }
        let mut cand = Vec::new();
        let roots = self.gc_root_text();
        let t_roots = t0.elapsed();
        scan_refs(&roots, n, &mut cand);
        let root_len = roots.len();
        drop(roots);
        for r in cand.drain(..) {
            m.push(r);
        }
        // objects with side state (host half, gfx object, module tables):
        // one bitmap built per cycle instead of ~5 hash lookups per object
        let mut side_keys: Vec<ObjRef> = Vec::new();
        let mut module_keys: Vec<ObjRef> = Vec::new();
        crate::jregex::gc_keys(&mut module_keys);
        crate::android_spannable::gc_keys(&mut module_keys);
        crate::jio_chars::gc_keys(&mut module_keys);
        side_keys.extend(self.fw.objects.keys().copied());
        side_keys.extend(self.gfx.objs.keys().copied());
        let mut has_side = vec![0u8; n + 1];
        for k in side_keys {
            if let Some(b) = has_side.get_mut(k as usize) {
                *b |= 1;
            }
        }
        for k in module_keys {
            if let Some(b) = has_side.get_mut(k as usize) {
                *b |= 2;
            }
        }
        if minor {
            // Old side-state writes can introduce young refs without touching
            // Heap::get_mut. Scan these owners each minor; no missing barrier.
            let mut seen = std::collections::HashSet::new();
            for r in 1..=n as ObjRef {
                if !self.heap.is_young(r) && has_side[r as usize] != 0 && seen.insert(r) {
                    m.stack.push(r);
                }
            }
        }
        let mut traced_objects = 0;
        let mut text = String::new();
        // weak references: (reference, referent) pairs whose referent is not
        // traced through the reference; cleared after marking if unreached
        let mut weak: Vec<(ObjRef, ObjRef)> = Vec::new();
        let mut is_weak_class: std::collections::HashMap<String, bool> = Default::default();
        while let Some(r) = m.stack.pop() {
            traced_objects += 1;
            match self.heap.get(r) {
                Ok(HeapObj::Instance { class, fields }) => {
                    for (name, v) in fields {
                        if let Value::Obj(x) = v {
                            if name == "referent" {
                                let cp = &self.cp;
                                let weak_ref =
                                    *is_weak_class.entry(class.clone()).or_insert_with(|| {
                                        cp.is_subtype(class, "Ljava/lang/ref/WeakReference;")
                                            || cp.is_subtype(
                                                class,
                                                "Ljava/lang/ref/PhantomReference;",
                                            )
                                    });
                                if weak_ref {
                                    weak.push((r, *x));
                                    continue;
                                }
                            }
                            cand.push(*x);
                        }
                    }
                }
                Ok(HeapObj::Array { elems, .. }) => {
                    for v in elems {
                        if let Value::Obj(x) = v {
                            cand.push(*x);
                        }
                    }
                }
                _ => {}
            }
            let side = has_side.get(r as usize).copied().unwrap_or(0);
            if side == 0 {
                for x in cand.drain(..) {
                    m.push(x);
                }
                continue;
            }
            // ephemeron: the host half of a reachable host-backed object
            match self.fw.objects.get(&r) {
                // precise fast path for the most common host halves (their
                // only fields are these values); everything else conservative
                Some(HostObj::List { elems }) | Some(HostObj::Iter { elems, .. }) => {
                    for v in elems {
                        if let Value::Obj(x) = v {
                            cand.push(*x);
                        }
                    }
                }
                Some(HostObj::Map { entries }) => {
                    // java.util.WeakHashMap holds values strongly (a value
                    // pointing back to its key keeps the entry alive), but
                    // keys weakly. Null keys are permanent, just like Java.
                    let weak_keys = self
                        .heap
                        .class_of(r)
                        .map(|c| self.cp.is_subtype(c, "Ljava/util/WeakHashMap;"))
                        .unwrap_or(false);
                    for (k, v) in entries {
                        if !weak_keys {
                            if let Value::Obj(x) = k {
                                cand.push(*x);
                            }
                        }
                        if let Value::Obj(x) = v {
                            cand.push(*x);
                        }
                    }
                }
                Some(h) => {
                    text.clear();
                    let _ = write!(text, "{h:?}");
                    scan_refs(&text, n, &mut cand);
                }
                None => {}
            }
            // module side tables keyed by object (Matcher, Spannable, Reader…)
            if side & 2 != 0 {
                text.clear();
                crate::jregex::gc_entry(r, &mut text);
                crate::android_spannable::gc_entry(r, &mut text);
                crate::jio_chars::gc_entry(r, &mut text);
                if !text.is_empty() {
                    scan_refs(&text, n, &mut cand);
                }
            }
            if let Some(g) = self.gfx.objs.get(&r) {
                text.clear();
                let _ = write!(text, "{g:?}");
                scan_refs(&text, n, &mut cand);
            }
            for x in cand.drain(..) {
                m.push(x);
            }
        }
        // clear weak references whose referent only they kept alive (ART
        // semantics; SoftReference stays strong — cleared only under memory
        // pressure on Android). ReferenceQueue enqueue is not modelled.
        let mut cleared_weak = 0usize;
        for (wr, referent) in weak {
            if !m.marked.get(referent as usize).copied().unwrap_or(true) {
                let _ = self.heap.put_field(wr, "referent", Value::Null);
                cleared_weak += 1;
                // registered ReferenceQueue: enqueue on an intrusive list kept
                // in heap fields (`rd$head` / `rd$next`), traced like any field
                if let Value::Obj(q) = self.heap.get_field(wr, "queue").unwrap_or(Value::Null) {
                    let enq = self.heap.get_field(wr, "rd$enq").unwrap_or(Value::Null);
                    if !matches!(enq, Value::Int(1)) {
                        let head = self.heap.get_field(q, "rd$head").unwrap_or(Value::Null);
                        let _ = self.heap.put_field(wr, "rd$next", head);
                        let _ = self.heap.put_field(wr, "rd$enq", Value::Int(1));
                        let _ = self.heap.put_field(q, "rd$head", Value::Obj(wr));
                    }
                }
            }
        }
        let mut cleared_weak_keys = 0;
        for (r, host) in &mut self.fw.objects {
            if m.marked.get(*r as usize).copied().unwrap_or(false)
                && self
                    .heap
                    .class_of(*r)
                    .map(|c| self.cp.is_subtype(c, "Ljava/util/WeakHashMap;"))
                    .unwrap_or(false)
            {
                if let HostObj::Map { entries } = host {
                    let before = entries.len();
                    entries.retain(|(key, _)| match key {
                        Value::Obj(k) => m.marked.get(*k as usize).copied().unwrap_or(false),
                        _ => true,
                    });
                    cleared_weak_keys += before - entries.len();
                }
            }
        }
        let t_mark = t0.elapsed();
        // sweep
        let mut freed = 0usize;
        let mut freed_bytes = 0usize;
        let mut live = 0usize;
        for i in 1..=n {
            if m.marked[i] {
                live += 1;
                continue;
            }
            if let Some(b) = self.heap.free_slot(i as ObjRef) {
                freed += 1;
                freed_bytes += b;
            }
        }
        // purge weak side tables of freed keys
        let marked = &m.marked;
        let alive = |r: &ObjRef| marked.get(*r as usize).copied().unwrap_or(true);
        self.fw.objects.retain(|r, _| alive(r));
        self.gfx.objs.retain(|r, _| alive(r));
        self.bignums.retain(|r, _| alive(r));
        self.break_iters.retain(|r, _| alive(r));
        let alive_ref = |r: ObjRef| alive(&r);
        crate::jregex::gc_purge(&alive_ref);
        crate::android_spannable::gc_purge(&alive_ref);
        crate::jio_chars::gc_purge(&alive_ref);
        crate::xml_res::gc_purge(&alive_ref);
        crate::jrandom::gc_purge(&alive_ref);
        crate::android_uri::gc_purge(&alive_ref);
        crate::jannotations::gc_purge(&alive_ref);
        if std::env::var_os("RD_GC_TRACE").is_some() {
            eprintln!(
                "[rd gc] roots {:?} ({} B text, {} candidates) mark {:?} total {:?}",
                t_roots,
                root_len,
                m.marked.iter().filter(|x| **x).count(),
                t_mark,
                t0.elapsed()
            );
        }
        self.heap.promote();
        self.gc_slots_after = self.heap.len() - self.heap.free_slots();
        let prev = self.gc_stats.clone();
        let stats = GcStats {
            collections: prev.collections + 1,
            minor_collections: prev.minor_collections + u64::from(minor),
            major_collections: prev.major_collections + u64::from(!minor),
            last_kind: if minor { "minor" } else { "major" },
            last_traced_objects: traced_objects,
            last_live_objects: live,
            last_freed_objects: freed,
            last_freed_bytes: freed_bytes,
            last_used_before: used_before,
            last_used_after: self.heap.used(),
            last_pause_ms: t0.elapsed().as_secs_f64() * 1000.0,
            last_cleared_weak: cleared_weak,
            last_cleared_weak_keys: cleared_weak_keys,
            total_freed_objects: prev.total_freed_objects + freed as u64,
            total_freed_bytes: prev.total_freed_bytes + freed_bytes as u64,
        };
        self.gc_stats = stats.clone();
        stats
    }

    /// Collects if the heap grew past the adaptive threshold since the last
    /// cycle (live × 2, at least +16 MB, capped at half the budget so the
    /// next tool call always has >= 50 % headroom before its safe point).
    /// `RD_GC=off` disables automatic collection. Safe point required.
    pub fn maybe_gc(&mut self) -> Option<GcStats> {
        if std::env::var("RD_GC")
            .map(|v| v == "off" || v == "0")
            .unwrap_or(false)
        {
            return None;
        }
        if !self.gc_due() {
            return None;
        }
        let minor = self.gc_stats.collections % 8 != 7
            && self.heap.used() < self.heap.budget().saturating_mul(3) / 4
            && self.heap.len() - self.heap.free_slots() < crate::heap::MAX_OBJECTS * 3 / 4;
        let mut stats = self.collect_generation(minor);
        // A minor cannot reclaim old garbage. Escalate rather than repeatedly
        // returning a full heap to a caller that is about to allocate again.
        if minor && self.heap.used() >= self.heap.budget() / 2 {
            stats = self.gc();
        }
        Some(stats)
    }

    pub(crate) fn gc_due(&self) -> bool {
        let slots = self.heap.len() - self.heap.free_slots();
        // A live/pinned heap may remain above a pressure watermark. Never
        // collect every interpreter instruction when the previous GC could
        // not reclaim it: require growth and use remaining-slot headroom.
        if !self.gc_frames.is_empty()
            && self.heap.used() <= self.gc_stats.last_used_after
            && slots <= self.gc_slots_after
        {
            return false;
        }
        let slot_limit = if self.gc_frames.is_empty() {
            crate::heap::MAX_OBJECTS * 3 / 4
        } else {
            (crate::heap::MAX_OBJECTS * 3 / 4).max(
                self.gc_slots_after
                    .saturating_add(((crate::heap::MAX_OBJECTS - self.gc_slots_after) / 2).max(1)),
            )
        };
        let base = self.gc_stats.last_used_after;
        let cap = if base < self.heap.budget() / 2 {
            self.heap.budget() / 2
        } else {
            base.saturating_add((self.heap.budget() - base) / 2)
        };
        let threshold = base
            .saturating_mul(2)
            .max(base.saturating_add(16 << 20))
            .min(cap);
        (self.heap.used() >= threshold || slots >= slot_limit)
            && !std::env::var("RD_GC")
                .map(|v| v == "off" || v == "0")
                .unwrap_or(false)
    }

    /// Protect allocations made by a Rust host dispatch while permitting a
    /// guest callback to collect its own allocations.
    pub(crate) fn with_host_roots<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let depth = self.heap.gc_scope_depth();
        self.heap.gc_host_enter(false);
        let out = f(self);
        self.heap.gc_host_restore(depth);
        out
    }

    pub(crate) fn publish_frame_roots(
        &mut self,
        regs: &[Value],
        result: &Value,
        exception: Option<ObjRef>,
    ) {
        if let Some(roots) = self.gc_frames.last_mut() {
            roots.clear();
            roots.extend(
                regs.iter()
                    .chain(std::iter::once(result))
                    .filter_map(|v| match v {
                        Value::Obj(r) => Some(*r),
                        _ => None,
                    }),
            );
            roots.extend(exception);
        }
    }

    /// Statistics of the collector so far.
    pub fn gc_stats(&self) -> &GcStats {
        &self.gc_stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_host_scope_returns_stay_rooted_until_the_outer_callback_returns() {
        let mut vm = Engine::new(vec![], Default::default());
        // Avoid small ObjRef ids retained by conservative scalar roots.
        for _ in 0..64 {
            vm.heap.alloc_instance("LPad;".into(), vec![]).unwrap();
        }
        vm.gc_frames.push(vec![]);
        let r = vm.with_host_roots(|vm| {
            let r = vm.with_host_roots(|vm| {
                vm.heap
                    .alloc_string("nested host temporary".into())
                    .unwrap()
            });
            vm.gc();
            assert_eq!(vm.heap.as_str(r).unwrap(), "nested host temporary");
            r
        });
        vm.gc_frames.pop();
        vm.gc();
        assert_eq!(
            vm.heap.class_of(r).unwrap(),
            FREED_CLASS,
            "pins must not leak after the enclosing callback ends"
        );
    }

    #[test]
    fn slot_pressure_does_not_recollect_unchanged_pinned_heap_each_instruction() {
        let mut vm = Engine::new(vec![], Default::default());
        for _ in 0..crate::heap::MAX_OBJECTS * 3 / 4 + 8 {
            vm.heap.alloc_instance("LExisting;".into(), vec![]).unwrap();
        }
        let scope = vm.heap.gc_scope_depth();
        vm.heap.gc_host_enter(true);
        vm.gc_frames.push(vec![]);
        assert!(vm.gc_due());
        vm.maybe_gc().unwrap();
        assert!(!vm.gc_due());
        assert!(vm.maybe_gc().is_none());
        vm.heap.alloc_instance("LNew;".into(), vec![]).unwrap();
        assert!(!vm.gc_due(), "growth must reach the backed-off watermark");
        vm.gc_frames.pop();
        vm.heap.gc_scope_restore(scope);
        assert!(
            vm.gc_due(),
            "tool boundary can collect after host pins are released"
        );
        vm.gc();
        assert!(
            vm.heap.len() - vm.heap.free_slots() <= 16,
            "only conservative scalar roots may remain"
        );
    }

    #[test]
    fn host_temporary_from_guest_return_survives_another_guest_callback() {
        use rd_testsupport::vm::*;
        let mut b = DexBuilder::new();
        let c = b.class("LCaso;", "Ljava/lang/Object;");
        let ty = b.type_idx("[B");
        let mut make = op11n(0x12, 0, 1);
        make.extend(op22c(0x23, 1, 0, ty));
        make.extend(op11x(0x11, 1));
        b.direct(
            c,
            "make",
            "[B",
            vec![],
            ACC_PUBLIC | ACC_STATIC,
            Some(b.code(2, 0, 0, make)),
        );
        let mut code = op11n(0x12, 0, 0);
        code.extend(op21s(0x13, 1, 100));
        code.extend(op21s(0x13, 2, 32767));
        let start = code.len();
        code.extend(op22c(0x23, 3, 2, ty));
        code.extend(op22b(0xd8, 0, 0, 1));
        let at = code.len();
        code.extend(op22t(0x34, 0, 1, start as i16 - at as i16));
        code.extend(op11x(0x0f, 0));
        b.direct(
            c,
            "run",
            "I",
            vec![],
            ACC_PUBLIC | ACC_STATIC,
            Some(b.code(4, 0, 0, code)),
        );
        let dex = rd_dex::Dex::parse(b.finish()).unwrap();
        let mut vm = Engine::new(
            vec![dex],
            crate::VmConfig {
                heap_budget: 4 << 20,
                ..Default::default()
            },
        );
        // Simulate a suspended Java caller around an inline host callback.
        vm.gc_frames.push(vec![]);
        vm.with_host_roots(|vm| {
            let Value::Obj(r) = vm.invoke_static("LCaso;", "make", "()[B", &[]).unwrap() else {
                panic!()
            };
            vm.invoke_static("LCaso;", "run", "()I", &[]).unwrap();
            assert_eq!(vm.heap.as_array_elems(r).unwrap().0.len(), 1);
            assert!(vm.gc_stats.collections > 10);
        });
        vm.gc_frames.pop();
    }

    #[test]
    fn scan_finds_only_valid_refs() {
        let mut v = Vec::new();
        scan_refs("Obj(3) [7, 12] Int(0) x99 1000", 50, &mut v);
        assert_eq!(v, vec![3, 7, 12]);
    }
}
