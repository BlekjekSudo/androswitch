//! Conservative admission accounting before allocating decoded DEX metadata.
//! This estimates owned parser storage, not allocator RSS or VM object memory.
use crate::{dex::Header, read, strings, Dex, RdError, RdResult};
use core::mem::size_of;

fn add(total: &mut usize, bytes: usize, limit: usize) -> RdResult<()> {
    *total = total.checked_add(bytes).ok_or_else(|| budget(limit))?;
    if *total > limit {
        return Err(budget(limit));
    }
    Ok(())
}
fn budget(limit: usize) -> RdError {
    RdError::new(
        "DEX_MEMORY_BUDGET",
        alloc::format!(
            "decoded DEX admission exceeds {limit} bytes; raw input is only part of parser memory"
        ),
        crate::MODULE_ID,
    )
}
fn table(data: &[u8], count: u32, offset: u32, width: usize) -> RdResult<()> {
    let length = (count as usize)
        .checked_mul(width)
        .ok_or_else(|| budget(0))?;
    let start = offset as usize;
    let end = start.checked_add(length).ok_or_else(|| budget(0))?;
    data.get(start..end)
        .ok_or_else(|| RdError::parse("DEX table outside input"))?;
    Ok(())
}

/// Upper admission estimate, including raw capacity, decoded strings/tables and
/// class-index allowance. Duplicate/overlapping string offsets are charged on
/// every occurrence, so a tiny malformed DEX cannot cause unbounded decoding.
pub fn estimate(data: &[u8], raw_capacity: usize, h: &Header, limit: usize) -> RdResult<usize> {
    use alloc::string::String;
    let mut total = raw_capacity;
    add(&mut total, 1 << 20, limit)?;
    for (count, offset, width, owned) in [
        (
            h.string_ids_size,
            h.string_ids_off,
            4,
            // `Strings::values` can grow geometrically above its initial cap;
            // include spare slots and per-string allocator alignment allowance.
            size_of::<String>() * 2 + 4 + 32,
        ),
        (h.type_ids_size, h.type_ids_off, 4, 4),
        (
            h.proto_ids_size,
            h.proto_ids_off,
            12,
            size_of::<crate::protos::ProtoId>(),
        ),
        (
            h.field_ids_size,
            h.field_ids_off,
            8,
            size_of::<crate::fields::FieldId>(),
        ),
        (
            h.method_ids_size,
            h.method_ids_off,
            8,
            size_of::<crate::methods::MethodId>(),
        ),
        (
            h.class_defs_size,
            h.class_defs_off,
            32,
            size_of::<crate::classes::ClassDef>() + 256,
        ),
    ] {
        table(data, count, offset, width)?;
        add(
            &mut total,
            (count as usize)
                .checked_mul(owned)
                .ok_or_else(|| budget(limit))?,
            limit,
        )?;
    }
    for i in 0..h.string_ids_size as usize {
        let offset = read::u32_at(data, h.string_ids_off as usize + i * 4)? as usize;
        let encoded = strings::read_string_slice(data, offset)?;
        // Lossy MUTF-8 can expand malformed bytes by three; String growth can
        // round capacity again. Six times input plus a small minimum safely
        // covers the decoder's current allocation strategy.
        add(&mut total, (encoded.len() * 6).max(8), limit)?;
    }
    // Class descriptor keys are independently owned by the portable BTreeMap.
    for i in 0..h.class_defs_size as usize {
        let class = read::u32_at(data, h.class_defs_off as usize + i * 32)?;
        if class >= h.type_ids_size {
            continue;
        }
        let string = read::u32_at(data, h.type_ids_off as usize + class as usize * 4)?;
        if string >= h.string_ids_size {
            continue;
        }
        let offset = read::u32_at(data, h.string_ids_off as usize + string as usize * 4)?;
        add(
            &mut total,
            strings::read_string_slice(data, offset as usize)?.len() * 4,
            limit,
        )?;
    }
    if h.map_off != 0 {
        let count = read::u32_at(data, h.map_off as usize)?;
        table(
            data,
            count,
            h.map_off.checked_add(4).ok_or_else(|| budget(limit))?,
            12,
        )?;
        add(
            &mut total,
            count as usize * size_of::<crate::MapItem>(),
            limit,
        )?;
        for i in 0..count as usize {
            let at = h.map_off as usize + 4 + i * 12;
            let kind = read::u16_at(data, at)?;
            let count = read::u32_at(data, at + 4)? as usize;
            let width = match kind {
                0x0007 => 8, // Vec::push may retain up to twice the used slots.
                0x0008 => size_of::<crate::methods::MethodHandle>(),
                _ => 0,
            };
            add(
                &mut total,
                count.checked_mul(width).ok_or_else(|| budget(limit))?,
                limit,
            )?;
        }
    }
    Ok(total)
}

impl Dex {
    /// Device-facing path: admit parser allocations before decoding metadata.
    /// Legacy `parse` keeps its existing behavior for upstream callers.
    pub fn parse_budgeted(data: alloc::vec::Vec<u8>, limit: usize) -> RdResult<(Self, usize)> {
        let header = Self::parse_header(&data)?;
        if !header.checksum_ok {
            return Err(RdError::new(
                "INVALID_CHECKSUM",
                "DEX Adler-32 mismatch",
                crate::MODULE_ID,
            ));
        }
        let estimate = estimate(&data, data.capacity(), &header, limit)?;
        let mut dex = Self::parse_unverified(data)?;
        // Integrity was checked before decoded allocation; do not scan twice.
        dex.header.checksum_ok = true;
        Ok((dex, estimate))
    }
}
