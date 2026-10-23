//! Shared PCM contract. A successful write means these frames reached the
//! platform output; unsupported formats never silently discard samples.
use alloc::vec::Vec;
use rd_vm::{Engine, ObjRef, Throwable, VmExit};
pub trait PcmOutput {
    fn open(&self, rate: i32, channels: i32, format: i32) -> Result<u64, VmExit>;
    fn start(&self, id: u64) -> Result<(), VmExit>;
    fn write(&self, id: u64, pcm: &[u8]) -> Result<usize, VmExit>;
    fn stop(&self, id: u64) -> Result<(), VmExit>;
    fn close(&self, id: u64) -> Result<(), VmExit>;
}
pub(crate) fn samples(
    vm: &Engine,
    array: ObjRef,
    offset: i32,
    length: i32,
    shorts: bool,
) -> Result<Vec<u8>, VmExit> {
    let (values, _) = vm
        .heap
        .as_array_elems(array)
        .map_err(|e| rd_vm::err::vm_error("INVALID_AUDIO_ARRAY", e))?;
    let expected = if shorts { "[S" } else { "[B" };
    if vm.heap.class_of(array)? != expected
        || offset < 0
        || length < 0
        || offset as usize > values.len()
        || length as usize > values.len() - offset as usize
    {
        return Err(
            Throwable::new("Ljava/lang/IndexOutOfBoundsException;", "AudioTrack.write").into(),
        );
    }
    let bytes = length as usize * if shorts { 2 } else { 1 };
    if bytes > 192000 || !bytes.is_multiple_of(4) {
        return Err(rd_vm::err::vm_error(
            "AUDIO_FORMAT_UNAVAILABLE",
            "write requires <=1s of complete stereo PCM16 frames",
        )
        .into());
    }
    let mut out = Vec::with_capacity(bytes);
    for value in &values[offset as usize..offset as usize + length as usize] {
        let n = value.as_int()?;
        if shorts {
            out.extend_from_slice(&(n as i16).to_le_bytes());
        } else {
            out.push(n as u8);
        }
    }
    Ok(out)
}
