//! Set-constructor deduplication extracted from RustyDroid's host intrinsics.
//! Callers own storage/GC roots and supply their actual Java equality policy.
use alloc::vec::Vec;

/// Retain the first occurrence of each value, preserving encounter order.
/// Equality is queried in the same direction as Java addIfAbsent: incoming
/// element first. Callers stage values and publish only on success; on error
/// the supplied vector may be partially compacted.
pub fn deduplicate_in_place<T, E>(
    values: &mut Vec<T>,
    mut equal: impl FnMut(&T, &T) -> Result<bool, E>,
) -> Result<(), E> {
    let mut kept = 0;
    for source in 0..values.len() {
        let mut duplicate = false;
        for existing in values.iter().take(kept) {
            if equal(&values[source], existing)? {
                duplicate = true;
                break;
            }
        }
        if !duplicate {
            values.swap(kept, source);
            kept += 1;
        }
    }
    values.truncate(kept);
    Ok(())
}
