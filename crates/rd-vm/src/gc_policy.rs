//! Allocation-pressure policy extracted from RustyDroid gc.rs at 3761815.
//! Both collectors retain their own tracing graph, but use the same adaptive
//! byte/slot headroom and pinned-frame backoff. No platform state is stored here.

pub(crate) fn due(
    used: usize,
    budget: usize,
    slots: usize,
    previous_used: usize,
    previous_slots: usize,
    has_frames: bool,
) -> bool {
    // A live/pinned heap may remain above a pressure watermark. Never
    // collect every interpreter instruction when the previous GC could
    // not reclaim it: require growth and use remaining-slot headroom.
    if has_frames && used <= previous_used && slots <= previous_slots {
        return false;
    }
    let slot_limit = if !has_frames {
        crate::heap::MAX_OBJECTS * 3 / 4
    } else {
        (crate::heap::MAX_OBJECTS * 3 / 4).max(
            previous_slots.saturating_add(((crate::heap::MAX_OBJECTS - previous_slots) / 2).max(1)),
        )
    };
    let base = previous_used;
    let cap = if base < budget / 2 {
        budget / 2
    } else {
        base.saturating_add((budget - base) / 2)
    };
    let threshold = base
        .saturating_mul(2)
        .max(base.saturating_add(16 << 20))
        .min(cap);
    used >= threshold || slots >= slot_limit
}

/// A pinned callback can leave an artificially high live watermark. Revisit it
/// once at an unpinned platform/tool boundary, where old host temporaries are
/// no longer roots. A genuinely large live heap does not recollect every turn:
/// the resulting collection records that no interpreter frames were active.
pub(crate) fn unpinned_boundary_due(
    used: usize,
    budget: usize,
    slots: usize,
    previous_had_frames: bool,
    has_frames: bool,
) -> bool {
    previous_had_frames
        && !has_frames
        && (used >= budget / 2 || slots >= crate::heap::MAX_OBJECTS * 3 / 4)
}

#[cfg(test)]
mod tests {
    use super::{due, unpinned_boundary_due};

    #[test]
    fn an_unpinned_watermark_recheck_happens_once_and_never_drops_active_frames() {
        assert!(unpinned_boundary_due(40 << 20, 64 << 20, 1, true, false));
        assert!(!unpinned_boundary_due(40 << 20, 64 << 20, 1, true, true));
        assert!(!unpinned_boundary_due(40 << 20, 64 << 20, 1, false, false));
        assert!(!unpinned_boundary_due(8 << 20, 64 << 20, 1, true, false));
        assert!(unpinned_boundary_due(
            1,
            64 << 20,
            crate::heap::MAX_OBJECTS * 3 / 4,
            true,
            false,
        ));
    }

    #[test]
    fn pinned_slot_pressure_backs_off_but_a_tool_boundary_can_reclaim() {
        let slots = crate::heap::MAX_OBJECTS * 3 / 4 + 8;
        assert!(!due(1, 64 << 20, slots, 1, slots, true));
        assert!(!due(33, 64 << 20, slots + 1, 1, slots, true));
        assert!(due(1, 64 << 20, slots, 1, slots, false));
        let grown = slots + (crate::heap::MAX_OBJECTS - slots) / 2;
        assert!(due(1, 64 << 20, grown, 1, slots, true));
    }

    #[test]
    fn byte_pressure_uses_remaining_headroom_with_saturating_watermarks() {
        let budget = 64 << 20;
        assert!(!due(40 << 20, budget, 1, 40 << 20, 1, true));
        assert!(!due((40 << 20) + 32, budget, 2, 40 << 20, 1, true));
        assert!(due(52 << 20, budget, 2, 40 << 20, 1, true));
        assert!(!due(8 << 20, budget, 1, 8 << 20, 1, true));
        assert!(due(24 << 20, budget, 1, 8 << 20, 1, true));
        assert!(due(usize::MAX, usize::MAX, 2, usize::MAX - 8, 1, true));
    }
}
