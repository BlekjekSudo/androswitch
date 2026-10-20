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
#[cfg(test)]
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

/// Tokenizes the exact conservative Debug byte stream without buffering it.
/// A numeric token may span any number of write_str/write_char calls. The
/// saturating arithmetic and emitted order match the original scan_refs.
struct RefScanner<F> {
    limit: u64,
    pending: u64,
    bytes: usize,
    emit: F,
}

impl<F: FnMut(ObjRef)> RefScanner<F> {
    fn new(n: usize, emit: F) -> Self {
        Self {
            limit: n as u64,
            pending: 0,
            bytes: 0,
            emit,
        }
    }

    fn flush(&mut self) {
        if self.pending >= 1 && self.pending <= self.limit {
            (self.emit)(self.pending as ObjRef);
        }
        self.pending = 0;
    }

    fn finish(mut self) -> usize {
        self.flush();
        self.bytes
    }
}

impl<F: FnMut(ObjRef)> std::fmt::Write for RefScanner<F> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.bytes = self.bytes.saturating_add(s.len());
        let bytes = s.as_bytes();
        let mut i = 0;
        // Complete a token split across formatter writes. Empty writes must
        // not terminate it; a delimiter in this write must terminate it.
        if self.pending != 0 {
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                self.pending = self
                    .pending
                    .saturating_mul(10)
                    .saturating_add(u64::from(bytes[i] - b'0'));
                i += 1;
            }
            if i == bytes.len() {
                return Ok(());
            }
            self.flush();
        }
        // Most root text is non-numeric. Scan runs without resetting pending
        // or testing the ObjRef range once per non-digit byte.
        while i < bytes.len() {
            if bytes[i].is_ascii_digit() {
                let mut value = 0u64;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    value = value
                        .saturating_mul(10)
                        .saturating_add(u64::from(bytes[i] - b'0'));
                    i += 1;
                }
                self.pending = value;
                if i < bytes.len() {
                    self.flush();
                }
            } else {
                i += 1;
            }
        }
        Ok(())
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
    /// Conservative root rendering: every engine structure outside the heap,
    /// except the weak host-object table (traced per reachable key). A generic
    /// sink lets collection scan the identical Debug text without allocating it.
    fn write_gc_roots(&mut self, s: &mut impl std::fmt::Write) {
        // the parsed APK (zip, arsc, layouts) holds no heap refs: skip it
        let objects = std::mem::take(&mut self.fw.objects);
        let resources = self.fw.resources.take();
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
        let root_len = {
            let mut scanner = RefScanner::new(n, |r| m.push(r));
            self.write_gc_roots(&mut scanner);
            scanner.finish()
        };
        // Root phase now includes tokenization, rather than only formatting.
        let t_roots = t0.elapsed();
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
            // Arena indices are already unique; no auxiliary HashSet needed.
            for r in 1..=n as ObjRef {
                if !self.heap.is_young(r) && has_side[r as usize] != 0 {
                    m.stack.push(r);
                }
            }
        }
        let mut traced_objects = 0;
        // weak references: (reference, referent) pairs whose referent is not
        // traced through the reference; cleared after marking if unreached
        let mut weak: Vec<(ObjRef, ObjRef)> = Vec::new();
        // Heap/class metadata is immutable throughout marking. Borrow the class
        // descriptor rather than allocating its clone for every reference.
        let mut is_weak_class: std::collections::HashMap<&str, bool> = Default::default();
        while let Some(r) = m.stack.pop() {
            traced_objects += 1;
            match self.heap.get(r) {
                Ok(HeapObj::Instance { class, fields }) => {
                    for (name, v) in fields {
                        if let Value::Obj(x) = v {
                            if name == "referent" {
                                let cp = &self.cp;
                                let weak_ref =
                                    *is_weak_class.entry(class.as_str()).or_insert_with(|| {
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
                    let mut scanner = RefScanner::new(n, |x| cand.push(x));
                    let _ = write!(scanner, "{h:?}");
                    scanner.finish();
                }
                None => {}
            }
            // module side tables keyed by object (Matcher, Spannable, Reader…)
            if side & 2 != 0 {
                let mut scanner = RefScanner::new(n, |x| cand.push(x));
                crate::jregex::gc_entry(r, &mut scanner);
                crate::android_spannable::gc_entry(r, &mut scanner);
                crate::jio_chars::gc_entry(r, &mut scanner);
                scanner.finish();
            }
            if let Some(g) = self.gfx.objs.get(&r) {
                let mut scanner = RefScanner::new(n, |x| cand.push(x));
                let _ = write!(scanner, "{g:?}");
                scanner.finish();
            }
            for x in cand.drain(..) {
                m.push(x);
            }
        }
        // Release borrowed heap descriptors before weak-reference writes.
        drop(is_weak_class);
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
            // Only Map host halves contain weak keys. Other host variants
            // never entered the clearing body, so avoid resolving their class
            // hierarchy (especially every View in a large UI tree).
            let HostObj::Map { entries } = host else {
                continue;
            };
            if m.marked.get(*r as usize).copied().unwrap_or(false)
                && self
                    .heap
                    .class_of(*r)
                    .map(|c| self.cp.is_subtype(c, "Ljava/util/WeakHashMap;"))
                    .unwrap_or(false)
            {
                let before = entries.len();
                entries.retain(|(key, _)| match key {
                    Value::Obj(k) => m.marked.get(*k as usize).copied().unwrap_or(false),
                    _ => true,
                });
                cleared_weak_keys += before - entries.len();
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
    fn streaming_scanner_matches_buffered_scanner_at_every_utf8_boundary() {
        let cases = [
            "",
            "0",
            "00000000001",
            "12",
            "1234",
            "-1 +2 3.4 5e6 0x7f8",
            "α12β345🌳٦٧٨9\n10\t11",
            "\\u{0012} \\\"34\\\"",
            "18446744073709551615 18446744073709551616 999999999999999999999999999999999999 2",
            "[Some(1), Obj(234), Int(-56), Long(0), {name: \"789\"}]",
        ];
        for n in [0, 1, 9, 255, 1024, crate::heap::MAX_OBJECTS, usize::MAX] {
            for text in cases {
                let mut expected = Vec::new();
                scan_refs(text, n, &mut expected);
                for cut in (0..=text.len()).filter(|i| text.is_char_boundary(*i)) {
                    let mut actual = Vec::new();
                    let mut sink = RefScanner::new(n, |r| actual.push(r));
                    sink.write_str(&text[..cut]).unwrap();
                    sink.write_str("").unwrap();
                    sink.write_str(&text[cut..]).unwrap();
                    assert_eq!(sink.finish(), text.len());
                    assert_eq!(actual, expected, "n={n} cut={cut} text={text:?}");
                }
                let mut actual = Vec::new();
                let mut sink = RefScanner::new(n, |r| actual.push(r));
                for c in text.chars() {
                    sink.write_char(c).unwrap();
                }
                assert_eq!(sink.finish(), text.len());
                assert_eq!(actual, expected);
            }
        }
    }

    #[test]
    fn streaming_scanner_differential_random_chunks_and_overflow() {
        // Fixed seed: reproducible property-style differential coverage,
        // independent of formatting chunk boundaries or an extra dependency.
        let mut seed = 0x5eed_1234_2026_1020u64;
        fn next(seed: &mut u64) -> u64 {
            *seed ^= *seed << 13;
            *seed ^= *seed >> 7;
            *seed ^= *seed << 17;
            *seed
        }
        let pieces = [
            "0",
            "1",
            "9",
            "00000",
            "18446744073709551616",
            "9876543210",
            "|",
            "-",
            ".",
            "e",
            "α",
            "🌳",
            "٣",
            "\n",
            "\\u{12}",
            "Obj(",
        ];
        for _ in 0..2048 {
            let n = (next(&mut seed) as usize) & 0xffff;
            let mut text = String::new();
            for _ in 0..96 {
                text.push_str(pieces[(next(&mut seed) as usize) % pieces.len()]);
            }
            let mut expected = Vec::new();
            scan_refs(&text, n, &mut expected);
            let mut actual = Vec::new();
            let mut sink = RefScanner::new(n, |r| actual.push(r));
            let mut start = 0;
            while start < text.len() {
                let mut end = (start + 1 + (next(&mut seed) % 23) as usize).min(text.len());
                while !text.is_char_boundary(end) {
                    end += 1;
                }
                sink.write_str(&text[start..end]).unwrap();
                start = end;
            }
            assert_eq!(sink.finish(), text.len());
            assert_eq!(actual, expected, "n={n} text={text:?}");
        }
    }

    #[test]
    fn streaming_engine_roots_equal_buffered_roots_and_restore_host_tables() {
        let mut vm = Engine::new(vec![], Default::default());
        for _ in 0..128 {
            vm.heap.alloc_instance("LPad;".into(), vec![]).unwrap();
        }
        vm.statics
            .insert(("LRoot123;".into(), "field9".into()), Value::Obj(99));
        vm.statics.insert(
            ("LRoot;".into(), "184467440737095516160🌳".into()),
            Value::Long(123),
        );
        vm.fw.objects.insert(
            100,
            HostObj::Bundle {
                map: vec![("hidden124".into(), Value::Obj(127))],
            },
        );
        let before = format!("{:?}", vm.fw);
        let mut text = String::new();
        vm.write_gc_roots(&mut text);
        let mut expected = Vec::new();
        scan_refs(&text, vm.heap.len(), &mut expected);
        let mut actual = Vec::new();
        let mut scanner = RefScanner::new(vm.heap.len(), |r| actual.push(r));
        vm.write_gc_roots(&mut scanner);
        assert_eq!(scanner.finish(), text.len());
        assert_eq!(actual, expected);
        assert_eq!(format!("{:?}", vm.fw), before);
        // Weak host-object contents must not accidentally become strong roots.
        assert!(!actual.contains(&127));
    }

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
