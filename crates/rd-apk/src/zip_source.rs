//! Positional storage for the shared ZIP parser; no filesystem or std dependency.
use super::*;

pub trait ReadAt: core::fmt::Debug + Send + Sync {
    fn len(&self) -> u64;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn read_at(&self, offset: u64, output: &mut [u8]) -> RdResult<usize>;
}

#[derive(Clone, Copy, Debug)]
pub struct ZipLimits {
    pub max_input_bytes: u64,
    pub max_directory_bytes: usize,
    pub max_entries: usize,
    /// Whole-entry extraction only; streaming assets may exceed this budget.
    pub max_entry_bytes: u64,
}
impl Default for ZipLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 512 << 20,
            max_directory_bytes: 16 << 20,
            max_entries: 100_000,
            max_entry_bytes: 64 << 20,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum Data {
    Memory(Arc<Vec<u8>>),
    Source(Arc<dyn ReadAt>),
}
impl Data {
    pub(super) fn len(&self) -> usize {
        match self {
            Self::Memory(v) => v.len(),
            Self::Source(s) => s.len() as usize,
        }
    }
    pub(super) fn read(&self, offset: usize, out: &mut [u8]) -> RdResult<()> {
        let end = offset
            .checked_add(out.len())
            .filter(|end| *end <= self.len())
            .ok_or_else(|| RdError::parse("zip: positioned read outside container"))?;
        match self {
            Self::Memory(v) => out.copy_from_slice(&v[offset..end]),
            Self::Source(source) => {
                let mut done = 0;
                while done < out.len() {
                    let chunk_end = (done + (64 << 10)).min(out.len());
                    let n = source.read_at((offset + done) as u64, &mut out[done..chunk_end])?;
                    if n == 0 || n > out.len() - done {
                        return Err(RdError::parse("zip: short positioned read"));
                    }
                    done += n;
                }
            }
        }
        Ok(())
    }
    pub(super) fn bytes(&self, offset: usize, length: usize) -> RdResult<Vec<u8>> {
        if offset
            .checked_add(length)
            .map_or(true, |end| end > self.len())
        {
            return Err(RdError::parse("zip: range outside container"));
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| RdError::invalid_format("zip: allocation budget"))?;
        bytes.resize(length, 0);
        self.read(offset, &mut bytes)?;
        Ok(bytes)
    }
}

impl Zip {
    pub fn from_source(source: Arc<dyn ReadAt>, limits: ZipLimits) -> RdResult<Self> {
        let length = source.len();
        if length < 22 || length > limits.max_input_bytes || length > usize::MAX as u64 {
            return Err(RdError::invalid_format(
                "zip: container outside configured size budget",
            ));
        }
        let data = Data::Source(source);
        let tail_start = data.len().saturating_sub(22 + 65_535);
        let tail = data.bytes(tail_start, data.len() - tail_start)?;
        let end = (0..=tail.len() - 22)
            .rev()
            .find(|&p| {
                u32_at(&tail, p).ok() == Some(EOCD_SIG)
                    && u16_at(&tail, p + 20)
                        .ok()
                        .is_some_and(|n| p + 22 + n as usize == tail.len())
            })
            .ok_or_else(|| RdError::parse("zip: EOCD signature/comment extent not found"))?;
        if u16_at(&tail, end + 4)? != 0 || u16_at(&tail, end + 6)? != 0 {
            return Err(RdError::invalid_format(
                "zip: multidisk archive unsupported",
            ));
        }
        let mut count = u16_at(&tail, end + 10)? as u64;
        let mut size = u32_at(&tail, end + 12)? as u64;
        let mut offset = u32_at(&tail, end + 16)? as u64;
        if count == 0xffff || size == 0xffff_ffff || offset == 0xffff_ffff {
            let loc = (tail_start + end)
                .checked_sub(20)
                .ok_or_else(|| RdError::parse("zip: truncated ZIP64 locator"))?;
            let locator = data.bytes(loc, 20)?;
            if u32_at(&locator, 0)? != ZIP64_EOCD_LOCATOR_SIG
                || u32_at(&locator, 4)? != 0
                || u32_at(&locator, 16)? != 1
            {
                return Err(RdError::invalid_format(
                    "zip: invalid/multidisk ZIP64 locator",
                ));
            }
            let record_offset = usize::try_from(u64_at(&locator, 8)?)
                .map_err(|_| RdError::parse("zip: ZIP64 offset overflow"))?;
            let record = data.bytes(record_offset, 56)?;
            if u32_at(&record, 0)? != ZIP64_EOCD_SIG
                || u64_at(&record, 4)? < 44
                || u32_at(&record, 16)? != 0
                || u32_at(&record, 20)? != 0
            {
                return Err(RdError::invalid_format(
                    "zip: invalid/multidisk ZIP64 record",
                ));
            }
            count = u64_at(&record, 32)?;
            size = u64_at(&record, 40)?;
            offset = u64_at(&record, 48)?;
        } else if u16_at(&tail, end + 8)? as u64 != count {
            return Err(RdError::invalid_format(
                "zip: inconsistent disk entry count",
            ));
        }
        if count > limits.max_entries as u64
            || size > limits.max_directory_bytes as u64
            || offset
                .checked_add(size)
                .map_or(true, |n| n > (tail_start + end) as u64)
        {
            return Err(RdError::invalid_format(
                "zip: directory exceeds extent/metadata budget",
            ));
        }
        let directory = data.bytes(offset as usize, size as usize)?;
        let mut entries = Vec::with_capacity(count as usize);
        let mut cursor = 0usize;
        for _ in 0..count {
            let stride = 46
                + u16_at(&directory, cursor + 28)? as usize
                + u16_at(&directory, cursor + 30)? as usize
                + u16_at(&directory, cursor + 32)? as usize;
            if cursor
                .checked_add(stride)
                .map_or(true, |n| n > directory.len())
            {
                return Err(RdError::parse("zip: central entry outside directory"));
            }
            entries.push(Self::parse_cd_entry(&directory, cursor)?);
            cursor += stride;
        }
        Ok(Self::index(data, entries, offset, limits.max_entry_bytes))
    }
    pub fn len(&self) -> u64 {
        self.data.len() as u64
    }
    pub fn is_empty(&self) -> bool {
        self.data.len() == 0
    }
    pub fn is_file_backed(&self) -> bool {
        matches!(self.data, Data::Source(_))
    }
    pub fn read_range(&self, offset: u64, length: usize) -> RdResult<Vec<u8>> {
        let offset =
            usize::try_from(offset).map_err(|_| RdError::parse("zip: range offset overflow"))?;
        self.data.bytes(offset, length)
    }
}
