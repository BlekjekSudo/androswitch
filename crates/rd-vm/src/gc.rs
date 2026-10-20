//! Mark-sweep garbage collector for the app heap.
//!
//! Before this, the heap was an arena: nothing was ever freed, and a long
//! session hit the 256 MB budget (the main risk for the Moto E5 / 2 GB floor).
//!
//! Design (safe-point, non-moving, precise heap graph + conservative roots):
//! - **Safe point.** `Engine::gc` must run when no interpreter frame is live
//!   (between MCP tool calls / host entry points). The Dalvik registers live
//!   on the Rust stack, so they are not a root set we have to enumerate.
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
    pub last_live_objects: usize,
    pub last_freed_objects: usize,
    pub last_freed_bytes: usize,
    pub last_used_before: usize,
    pub last_used_after: usize,
    pub last_pause_ms: f64,
    /// WeakReference/PhantomReference referents cleared in the last cycle.
    pub last_cleared_weak: usize,
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

    /// Collects unreachable heap objects. Call only at a safe point (no
    /// interpreter frame live). Returns this cycle's statistics.
    pub fn gc(&mut self) -> GcStats {
        let t0 = std::time::Instant::now();
        let n = self.heap.len();
        let used_before = self.heap.used();
        let mut m = Marker {
            marked: vec![false; n + 1],
            stack: Vec::new(),
        };
        let mut cand = Vec::new();
        let roots = self.gc_root_text();
        let t_roots = t0.elapsed();
        scan_refs(&roots, n, &mut cand);
        let root_len = roots.len();
        drop(roots);
        for r in cand.drain(..) {
            m.push(r);
        }
        let mut text = String::new();
        // weak references: (reference, referent) pairs whose referent is not
        // traced through the reference; cleared after marking if unreached
        let mut weak: Vec<(ObjRef, ObjRef)> = Vec::new();
        let mut is_weak_class: std::collections::HashMap<String, bool> = Default::default();
        while let Some(r) = m.stack.pop() {
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
            // ephemeron: the host half of a reachable host-backed object
            if let Some(h) = self.fw.objects.get(&r) {
                text.clear();
                let _ = write!(text, "{h:?}");
                scan_refs(&text, n, &mut cand);
            }
            // module side tables keyed by object (Matcher, Spannable, Reader…)
            text.clear();
            crate::jregex::gc_entry(r, &mut text);
            crate::android_spannable::gc_entry(r, &mut text);
            crate::jio_chars::gc_entry(r, &mut text);
            if !text.is_empty() {
                scan_refs(&text, n, &mut cand);
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
        let prev = self.gc_stats.clone();
        let stats = GcStats {
            collections: prev.collections + 1,
            last_live_objects: live,
            last_freed_objects: freed,
            last_freed_bytes: freed_bytes,
            last_used_before: used_before,
            last_used_after: self.heap.used(),
            last_pause_ms: t0.elapsed().as_secs_f64() * 1000.0,
            last_cleared_weak: cleared_weak,
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
        let used = self.heap.used();
        let base = self.gc_stats.last_used_after;
        let threshold = (base.saturating_mul(2))
            .max(base + (16 << 20))
            .min(self.heap.budget() / 2);
        (used >= threshold).then(|| self.gc())
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
    fn scan_finds_only_valid_refs() {
        let mut v = Vec::new();
        scan_refs("Obj(3) [7, 12] Int(0) x99 1000", 50, &mut v);
        assert_eq!(v, vec![3, 7, 12]);
    }
}
