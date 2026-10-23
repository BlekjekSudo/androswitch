//! Optional host-only CPU oracle. No worker dependency is enabled on Horizon.
use crate::*;
use rd_ndk::runtime::{Argument, Runtime};
pub struct Oracle {
    pub runtime: Runtime,
    next: u64,
    tls_used: usize,
}
fn convert(e: rd_ndk::RdError) -> Error {
    error(&e.code, "oracle", "unicorn", e.cause)
}
impl Oracle {
    pub fn new() -> Result<Self> {
        Ok(Self {
            runtime: Runtime::new(Abi::Arm64).map_err(convert)?,
            next: 0x10000000,
            tls_used: 256,
        })
    }
    pub fn call(&mut self, frame: &mut CallFrame) -> Result<()> {
        let args: Vec<Argument> = frame
            .integer
            .iter()
            .map(|v| Argument::word(*v))
            .chain(frame.float.iter().map(|v| Argument {
                kind: 'D',
                bits: *v,
            }))
            .chain(frame.stack.iter().map(|v| Argument::word(*v)))
            .collect();
        let value = self
            .runtime
            .call(frame.function, &args, 1_000_000, |_, _| {
                Err(rd_ndk::RdError::new(
                    "ORACLE_CALLBACK_UNAVAILABLE",
                    "pure native oracle has no JNI context",
                ))
            })
            .map_err(convert)?;
        frame.result_integer[0] = value.bits;
        frame.result_float[0] = value.fp_bits.unwrap_or(0);
        Ok(())
    }
}
impl NativeBackend for Oracle {
    fn allocate(&mut self, length: usize) -> Result<Allocation> {
        let address = self.next;
        self.next += length as u64 + 65536;
        self.runtime.map(address, length as u64).map_err(convert)?;
        Ok(Allocation {
            address,
            token: address,
            length,
        })
    }
    fn write(&mut self, m: Allocation, offset: usize, bytes: &[u8]) -> Result<()> {
        self.runtime
            .write(m.address + offset as u64, bytes)
            .map_err(convert)
    }
    fn finalize(&mut self, _: Allocation, _: &[u8]) -> Result<()> {
        Ok(())
    } // Unicorn maps RWX; Horizon separately enforces code permissions.
    fn release(&mut self, _: Allocation) {} // Worker process owns and releases the address space.
    fn import(&mut self, _: &str, name: &str) -> Result<Option<u64>> {
        self.runtime.bind_import(name).map(Some).map_err(convert)
    }
    fn tls(&mut self, _: usize, bytes: &[u8], size: usize, alignment: usize) -> Result<u64> {
        let offset = (self.tls_used + alignment - 1) & !(alignment - 1);
        if offset + size > 32768 {
            return Err(error("TLS_LIMIT", "oracle", "unicorn", "32 KiB template"));
        }
        self.runtime
            .write(0x58008000 + offset as u64, bytes)
            .map_err(convert)?;
        self.tls_used = offset + size;
        Ok(offset as u64)
    }
    fn tls_descriptor(&mut self, offset: u64) -> Result<(u64, u64)> {
        let code = self
            .runtime
            .alloc_bytes(&[0x00, 0x04, 0x40, 0xf9, 0xc0, 0x03, 0x5f, 0xd6])
            .map_err(convert)?;
        Ok((code, offset))
    }
}
