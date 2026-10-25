//! Features exposed by this offline guest profile, not an inventory of every
//! physical Switch peripheral. Unsupported sensors/radio/platform APIs are not
//! advertised just because the underlying console contains their hardware.
use super::*;

pub(super) fn has(vm: &Engine, args: &[Value]) -> Result<Value> {
    let Some(name) = args[0].as_ref()? else {
        return Ok(Value::Int(0));
    };
    let name = vm.heap.as_str(name)?;
    let requested = if args.len() > 1 { int(args, 1)? } else { 0 };
    // Single-pointer touch injection, CPU/GLES display and both orientations
    // are implemented by the shared input/display contracts. No multitouch,
    // general joystick axes, Android networking, camera or sensor APIs yet.
    let version = match name {
        "android.hardware.touchscreen"
        | "android.hardware.faketouch"
        | "android.hardware.screen.landscape"
        | "android.hardware.screen.portrait" => Some(0),
        _ => None,
    };
    Ok(Value::Int(i32::from(
        version.is_some_and(|v| v >= requested),
    )))
}
