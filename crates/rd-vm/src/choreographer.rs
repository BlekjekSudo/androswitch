//! `android.view.Choreographer`: the frame clock.
//!
//! Compose's `AndroidUiDispatcher`/`AndroidUiFrameClock`, `ValueAnimator`
//! and AndroidX animation all schedule work with `postFrameCallback`. With
//! no real vsync, a "frame" runs when the main Looper goes idle
//! (`drain_looper`): due callbacks are taken and `doFrame(frameTimeNanos)`
//! is invoked with the virtual clock. Callbacks posted during a frame run in
//! the next frame, as on a device.
//!
//! Frames do not advance the virtual clock; only `wait`/`advance_clock`
//! does, which keeps runs deterministic. A callback that re-posts itself
//! every frame (an endless animation) would therefore spin, so each drain
//! runs at most [`MAX_FRAMES_PER_DRAIN`] frames. The remainder runs on the
//! next drain.

use crate::engine::Engine;
use crate::err::VmExit;
use crate::heap::ObjRef;
use crate::value::Value;

pub(crate) const CHOREOGRAPHER: &str = "Landroid/view/Choreographer;";
pub(crate) const MAX_FRAMES_PER_DRAIN: usize = 8;

#[derive(Debug, Default, Clone)]
pub(crate) struct FrameState {
    pub instance: Option<ObjRef>,
    /// (callback, due virtual ms)
    pub callbacks: Vec<(ObjRef, u64)>,
    pub frames_run: u64,
    /// The single main-thread `android.animation.AnimationHandler` host
    /// object. The VM has one deterministic virtual UI thread, so unlike
    /// AOSP there is no per-thread handler map.
    pub animation_handler: Option<ObjRef>,
    /// AnimationFrameCallbacks registered with the handler, with an
    /// optional delayed start time in virtual milliseconds.
    pub animation_callbacks: Vec<(ObjRef, Option<u64>)>,
}

fn instance(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    if let Some(i) = vm.frames.instance {
        return Ok(i);
    }
    let i = vm
        .heap
        .alloc_instance(CHOREOGRAPHER.to_string(), Vec::new())?;
    vm.frames.instance = Some(i);
    Ok(i)
}

pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
) -> Result<Option<Value>, VmExit> {
    if class == CHOREOGRAPHER
        && (name == "getInstance" || name == "getMainThreadInstance")
        && sig == "()Landroid/view/Choreographer;"
    {
        return Ok(Some(Value::Obj(instance(vm)?)));
    }
    Ok(None)
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if mref_class != CHOREOGRAPHER {
        return Ok(None);
    }
    let cb = args.first().and_then(|v| v.as_ref().ok().flatten());
    let now = vm.fw.clock();
    match (name, sig) {
        ("postFrameCallback", "(Landroid/view/Choreographer$FrameCallback;)V") => {
            if let Some(cb) = cb {
                vm.frames.callbacks.push((cb, now));
            }
            Ok(Some(Value::Null))
        }
        ("postFrameCallbackDelayed", "(Landroid/view/Choreographer$FrameCallback;J)V") => {
            let delay = match args.get(1) {
                Some(Value::Long(l)) => (*l).max(0) as u64,
                _ => 0,
            };
            if let Some(cb) = cb {
                vm.frames.callbacks.push((cb, now.saturating_add(delay)));
            }
            Ok(Some(Value::Null))
        }
        ("removeFrameCallback", "(Landroid/view/Choreographer$FrameCallback;)V") => {
            if let Some(cb) = cb {
                vm.frames.callbacks.retain(|(c, _)| *c != cb);
            }
            Ok(Some(Value::Null))
        }
        ("getFrameTime", "()J") => Ok(Some(Value::Long(now as i64))),
        ("getFrameTimeNanos", "()J") => {
            Ok(Some(Value::Long((now as i64).saturating_mul(1_000_000))))
        }
        _ => Ok(None),
    }
}

/// Run one frame: every callback due now, in post order. Returns how many
/// ran.
pub(crate) fn run_frame(vm: &mut Engine) -> Result<usize, VmExit> {
    let now = vm.fw.clock();
    let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut vm.frames.callbacks)
        .into_iter()
        .partition(|(_, t)| *t <= now);
    vm.frames.callbacks = later;
    if due.is_empty() {
        return Ok(0);
    }
    vm.frames.frames_run += 1;
    let nanos = (now as i64).saturating_mul(1_000_000);
    for (cb, _) in &due {
        let class = vm.heap.class_of(*cb)?.to_string();
        if let Some((d, def, m)) = vm.cp.resolve_method(&class, "doFrame", "(J)V") {
            vm.call(
                d,
                def,
                &m,
                vec![Value::Obj(*cb), Value::Long(nanos), Value::WideHi],
            )?;
        } else if class == "Landroid/transition/TransitionManager$MultiListener;" {
            // TransitionManager schedules its pre-draw work on the next
            // deterministic frame, matching ViewRootImpl's traversal point.
            vm.invoke_instance(*cb, "onPreDraw", "()Z", &[])?;
        } else if class == "Landroid/animation/AnimationHandler;" {
            // The host AnimationHandler is itself posted as the frame
            // callback (AOSP uses an anonymous FrameCallback field). Its
            // intrinsic consumes the current virtual millisecond clock and
            // re-posts itself while animations remain registered.
            vm.invoke_instance(*cb, "doFrame", "(J)V", &[Value::Long(nanos)])?;
        }
    }
    Ok(due.len())
}

/// Whether a callback is due at the current virtual time.
pub(crate) fn has_due(vm: &Engine) -> bool {
    let now = vm.fw.clock();
    vm.frames.callbacks.iter().any(|(_, t)| *t <= now)
}
