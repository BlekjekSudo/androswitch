//! `android.view.VelocityTracker`: estimates the finger velocity of a gesture
//! so that scrolling, flinging and ViewPager code can run.
//!
//! Mirrors the AOSP default strategy (`libs/input/VelocityTracker.cpp`):
//!
//! - **Strategy.** LSQ2 for the X and Y axes: a second-order least-squares fit
//!   of position against time over the samples newer than `HORIZON` (100 ms),
//!   with at most `HISTORY_SIZE` (20) samples. The velocity is the linear
//!   coefficient of that fit, in px/s, scaled by `units / 1000` and clamped
//!   to `maxVelocity`.
//! - **Gaps.** If no event arrives for more than `ASSUME_POINTER_STOPPED`
//!   (40 ms), the samples are dropped before the next one is added, as AOSP does.
//! - **Actions.** `DOWN`/`HOVER_ENTER` clear the tracker; `POINTER_DOWN` clears
//!   the samples of the pointer; `MOVE`/`HOVER_MOVE` add a sample; `UP` and
//!   `POINTER_UP` keep the last velocity; other actions are ignored.
//! - **Single pointer.** The model has one pointer, id 0. Pointer ids other
//!   than 0 (or the active pointer, -1) report 0, as AOSP does for untracked ids.
//! - **Fit for 2 samples.** AOSP uses its general QR solver with degree 1. With
//!   unit weights that is the ordinary least-squares line, which is computed
//!   here in closed form. The results match up to float rounding.
//! - **Not modelled.** The weighted strategies (WLSQ2, LSQ3, INT1/2, impulse),
//!   the scroll axis and the object pool (`recycle` is a no-op, `obtain` always
//!   allocates).
//!
//! State lives in heap fields (`rd_vt_*`), like `android_motion.rs`, so the GC
//! sees it and nothing lives outside the VM heap.

use crate::engine::Engine;
use crate::err::VmExit;
use crate::heap::ObjRef;
use crate::value::Value;

pub(crate) const VT: &str = "Landroid/view/VelocityTracker;";

/// AOSP `LeastSquaresVelocityTrackerStrategy::HISTORY_SIZE`.
const HISTORY_SIZE: usize = 20;
/// AOSP `LeastSquaresVelocityTrackerStrategy::HORIZON` (100 ms).
const HORIZON_NS: i64 = 100_000_000;
/// AOSP `ASSUME_POINTER_STOPPED_TIME` (40 ms).
const ASSUME_POINTER_STOPPED_NS: i64 = 40_000_000;

const AXIS_X: i32 = 0;
const AXIS_Y: i32 = 1;

const ACTION_MASK: i32 = 0xff;
const ACTION_DOWN: i32 = 0;
const ACTION_UP: i32 = 1;
const ACTION_MOVE: i32 = 2;
const ACTION_POINTER_DOWN: i32 = 5;
const ACTION_POINTER_UP: i32 = 6;
const ACTION_HOVER_MOVE: i32 = 7;
const ACTION_HOVER_ENTER: i32 = 9;

/// One position sample (AOSP `Movement`), with time in nanoseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Sample {
    t: i64,
    x: f32,
    y: f32,
}

fn put(vm: &mut Engine, r: ObjRef, f: &str, v: Value) -> Result<(), VmExit> {
    crate::err::put_field_result(vm.heap.put_field(r, f, v))
}

fn getf(vm: &Engine, r: ObjRef, f: &str) -> f32 {
    match vm.heap.get_field(r, f) {
        Ok(Value::Float(v)) => v,
        Ok(Value::Int(i)) => i as f32,
        Ok(Value::Double(d)) => d as f32,
        _ => 0.0,
    }
}

fn geti(vm: &Engine, r: ObjRef, f: &str) -> i32 {
    match vm.heap.get_field(r, f) {
        Ok(Value::Int(i)) => i,
        _ => 0,
    }
}

fn getl(vm: &Engine, r: ObjRef, f: &str) -> i64 {
    match vm.heap.get_field(r, f) {
        Ok(Value::Long(l)) => l,
        Ok(Value::Int(i)) => i as i64,
        _ => 0,
    }
}

fn arg_f(args: &[Value], i: usize) -> f32 {
    match args.get(i) {
        Some(Value::Float(f)) => *f,
        // raw f32 bits in an int slot
        Some(Value::Int(b)) => f32::from_bits(*b as u32),
        Some(Value::Double(d)) => *d as f32,
        _ => 0.0,
    }
}

fn arg_i(args: &[Value], i: usize) -> i32 {
    match args.get(i) {
        Some(Value::Int(v)) => *v,
        _ => 0,
    }
}

fn slot(name: &str, i: usize) -> String {
    format!("rd_vt_{name}{i}")
}

/// A new tracker, with no samples and velocity 0.
fn new_tracker(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    let r = vm.heap.alloc_instance(VT.to_string(), Vec::new())?;
    put(vm, r, "rd_vt_n", Value::Int(0))?;
    put(vm, r, "rd_vt_idx", Value::Int(-1))?;
    put(vm, r, "rd_vt_has", Value::Int(0))?;
    put(vm, r, "rd_vt_last", Value::Long(0))?;
    put(vm, r, "rd_vt_vx", Value::Float(0.0))?;
    put(vm, r, "rd_vt_vy", Value::Float(0.0))?;
    Ok(r)
}

/// Drops the samples (AOSP `clearPointer` / erase of the strategies). The
/// pointer bit and the last event time are kept.
fn clear_samples(vm: &mut Engine, r: ObjRef) -> Result<(), VmExit> {
    put(vm, r, "rd_vt_n", Value::Int(0))?;
    put(vm, r, "rd_vt_idx", Value::Int(-1))
}

/// AOSP `VelocityTracker::clear()`: samples and pointer state, not the
/// computed velocity (that changes only on `computeCurrentVelocity`).
fn clear(vm: &mut Engine, r: ObjRef) -> Result<(), VmExit> {
    clear_samples(vm, r)?;
    put(vm, r, "rd_vt_has", Value::Int(0))
}

/// Samples newest first.
fn read_samples(vm: &Engine, r: ObjRef) -> Vec<Sample> {
    let n = geti(vm, r, "rd_vt_n").clamp(0, HISTORY_SIZE as i32) as usize;
    let idx = geti(vm, r, "rd_vt_idx");
    if n == 0 || idx < 0 {
        return Vec::new();
    }
    let idx = idx as usize % HISTORY_SIZE;
    (0..n)
        .map(|k| {
            let i = (idx + HISTORY_SIZE - k) % HISTORY_SIZE;
            Sample {
                t: getl(vm, r, &slot("t", i)),
                x: getf(vm, r, &slot("x", i)),
                y: getf(vm, r, &slot("y", i)),
            }
        })
        .collect()
}

/// AOSP `LeastSquaresVelocityTrackerStrategy::addMovement`: a new sample
/// goes into the next ring slot, or overwrites the newest one when it has
/// the same timestamp.
fn write_sample(vm: &mut Engine, r: ObjRef, s: Sample) -> Result<(), VmExit> {
    let n = geti(vm, r, "rd_vt_n").clamp(0, HISTORY_SIZE as i32);
    let idx = geti(vm, r, "rd_vt_idx");
    let (new_idx, new_n) = if n == 0 || idx < 0 {
        (0usize, 1)
    } else if getl(vm, r, &slot("t", idx as usize % HISTORY_SIZE)) != s.t {
        (
            (idx as usize + 1) % HISTORY_SIZE,
            (n + 1).min(HISTORY_SIZE as i32),
        )
    } else {
        (idx as usize % HISTORY_SIZE, n)
    };
    put(vm, r, &slot("t", new_idx), Value::Long(s.t))?;
    put(vm, r, &slot("x", new_idx), Value::Float(s.x))?;
    put(vm, r, &slot("y", new_idx), Value::Float(s.y))?;
    put(vm, r, "rd_vt_idx", Value::Int(new_idx as i32))?;
    put(vm, r, "rd_vt_n", Value::Int(new_n))
}

/// AOSP `VelocityTracker::addMovement(eventTime, pointerId, axis, position)`
/// for the planar axes, with the 40 ms gap check.
fn add_sample(vm: &mut Engine, r: ObjRef, t: i64, x: f32, y: f32) -> Result<(), VmExit> {
    let has = geti(vm, r, "rd_vt_has");
    let last = getl(vm, r, "rd_vt_last");
    if has != 0 && t - last > ASSUME_POINTER_STOPPED_NS {
        clear_samples(vm, r)?;
    }
    put(vm, r, "rd_vt_last", Value::Long(t))?;
    put(vm, r, "rd_vt_has", Value::Int(1))?;
    write_sample(vm, r, Sample { t, x, y })
}

/// AOSP `VelocityTracker::addMovement(const MotionEvent*)` for a single
/// touch pointer. The event time is in milliseconds, as in `MotionEvent`.
fn add_motion_event(vm: &mut Engine, r: ObjRef, ev: ObjRef) -> Result<(), VmExit> {
    let action = geti(vm, ev, "rd_action") & ACTION_MASK;
    let t = getl(vm, ev, "rd_time") * 1_000_000;
    let x = getf(vm, ev, "rd_x");
    let y = getf(vm, ev, "rd_y");
    match action {
        ACTION_DOWN | ACTION_HOVER_ENTER => {
            clear(vm, r)?;
            add_sample(vm, r, t, x, y)
        }
        ACTION_POINTER_DOWN => {
            clear_samples(vm, r)?;
            add_sample(vm, r, t, x, y)
        }
        ACTION_MOVE | ACTION_HOVER_MOVE => add_sample(vm, r, t, x, y),
        ACTION_UP | ACTION_POINTER_UP => {
            // Keep the last velocity. Drop the samples if the finger stopped
            // long before it lifted.
            if t - getl(vm, r, "rd_vt_last") > ASSUME_POINTER_STOPPED_NS {
                clear_samples(vm, r)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Solves the unweighted second-order least squares fit `y = c + b·x + a·x²`
/// (AOSP `solveUnweightedLeastSquaresDeg2`, same float operation order).
/// Returns `[c, b, a]`, or `None` if the system is singular.
fn solve_deg2(x: &[f32], y: &[f32]) -> Option<[f32; 3]> {
    let count = x.len() as f32;
    let (mut sxi, mut sxiyi, mut syi, mut sxi2, mut sxi3, mut sxi2yi, mut sxi4) =
        (0f32, 0f32, 0f32, 0f32, 0f32, 0f32, 0f32);
    for (&xi, &yi) in x.iter().zip(y.iter()) {
        let xi2 = xi * xi;
        let xi3 = xi2 * xi;
        let xi4 = xi3 * xi;
        let xiyi = xi * yi;
        let xi2yi = xi2 * yi;
        sxi += xi;
        sxi2 += xi2;
        sxiyi += xiyi;
        sxi2yi += xi2yi;
        syi += yi;
        sxi3 += xi3;
        sxi4 += xi4;
    }
    let sxx = sxi2 - sxi * sxi / count;
    let sxy = sxiyi - sxi * syi / count;
    let sxx2 = sxi3 - sxi * sxi2 / count;
    let sx2y = sxi2yi - sxi2 * syi / count;
    let sx2x2 = sxi4 - sxi2 * sxi2 / count;
    let denominator = sxx * sx2x2 - sxx2 * sxx2;
    if denominator == 0.0 {
        return None;
    }
    let a = (sx2y * sxx - sxy * sxx2) / denominator;
    let b = (sxy * sx2x2 - sx2y * sxx2) / denominator;
    let c = syi / count - b * sxi / count - a * sxi2 / count;
    Some([c, b, a])
}

/// Slope of the least-squares line through `(x, y)`, or `None` if the
/// x values are all equal.
fn slope_deg1(x: &[f32], y: &[f32]) -> Option<f32> {
    let count = x.len() as f32;
    let (mut sx, mut sy, mut sxy, mut sx2) = (0f32, 0f32, 0f32, 0f32);
    for (&xi, &yi) in x.iter().zip(y.iter()) {
        sx += xi;
        sy += yi;
        sxy += xi * yi;
        sx2 += xi * xi;
    }
    let sxx = sx2 - sx * sx / count;
    let sxy_c = sxy - sx * sy / count;
    if sxx == 0.0 {
        return None;
    }
    Some(sxy_c / sxx)
}

/// LSQ2 velocity (px/s) of one axis from samples newest first, or `None`
/// when there is not enough data (AOSP returns no estimate in that case).
fn velocity_of(samples: &[Sample], x_axis: bool) -> Option<f32> {
    let newest = samples.first()?.t;
    let mut pos = Vec::new();
    let mut time = Vec::new();
    for s in samples.iter().take(HISTORY_SIZE) {
        let age = newest - s.t;
        if age > HORIZON_NS {
            break;
        }
        pos.push(if x_axis { s.x } else { s.y });
        // AOSP time axis: seconds, negative, so the slope is px per second
        time.push(-(age as f32) * 0.000_000_001_f32);
    }
    match pos.len() {
        0 | 1 => None,
        2 => slope_deg1(&time, &pos),
        _ => solve_deg2(&time, &pos).map(|c| c[1]),
    }
}

/// `computeCurrentVelocity(units, maxVelocity)`: stores the velocities, in
/// units per second, so the getters can read them.
fn compute(vm: &mut Engine, r: ObjRef, units: i32, max_velocity: f32) -> Result<(), VmExit> {
    let samples = read_samples(vm, r);
    let scale = |v: f32| {
        let adjusted = v * units as f32 / 1000.0;
        // not `clamp`: it panics if a negative maxVelocity is passed
        adjusted.max(-max_velocity).min(max_velocity)
    };
    let vx = velocity_of(&samples, true).map(scale).unwrap_or(0.0);
    let vy = velocity_of(&samples, false).map(scale).unwrap_or(0.0);
    put(vm, r, "rd_vt_vx", Value::Float(vx))?;
    put(vm, r, "rd_vt_vy", Value::Float(vy))
}

/// Pointer 0 (or the active pointer, -1) reports the stored velocity.
fn velocity_for_pointer(vm: &Engine, r: ObjRef, field: &str, id: i32) -> f32 {
    if id == 0 || id == -1 {
        getf(vm, r, field)
    } else {
        0.0
    }
}

pub fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    _args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != VT || name != "obtain" || sig != "()Landroid/view/VelocityTracker;" {
        return Ok(None);
    }
    Ok(Some(Value::Obj(new_tracker(vm)?)))
}

pub fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if recv_class != VT {
        return Ok(None);
    }
    let pointer = if args.is_empty() { 0 } else { arg_i(args, 0) };
    Ok(Some(match (name, sig) {
        ("addMovement", _) => {
            if let Some(Value::Obj(ev)) = args.first() {
                // only MotionEvents of the framework model are tracked
                if vm.heap.get_field(*ev, "rd_action").is_ok() {
                    add_motion_event(vm, recv, *ev)?;
                }
            }
            Value::Null
        }
        ("clear", _) => {
            clear(vm, recv)?;
            Value::Null
        }
        // Pooling is not modelled: obtain() always allocates.
        ("recycle", _) => Value::Null,
        ("computeCurrentVelocity", _) => {
            let units = arg_i(args, 0);
            let max = if args.len() > 1 {
                arg_f(args, 1)
            } else {
                f32::MAX
            };
            compute(vm, recv, units, max)?;
            Value::Null
        }
        ("getXVelocity", _) => Value::Float(velocity_for_pointer(vm, recv, "rd_vt_vx", pointer)),
        ("getYVelocity", _) => Value::Float(velocity_for_pointer(vm, recv, "rd_vt_vy", pointer)),
        ("getAxisVelocity", _) => {
            let axis = arg_i(args, 0);
            let id = if args.len() > 1 { arg_i(args, 1) } else { 0 };
            let field = match axis {
                AXIS_X => "rd_vt_vx",
                AXIS_Y => "rd_vt_vy",
                _ => return Ok(Some(Value::Float(0.0))),
            };
            Value::Float(velocity_for_pointer(vm, recv, field, id))
        }
        _ => return Ok(None),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Samples at `t_ms` = 0, 10, …, newest first, with `f(t_ms)` as x.
    fn series(end_ms: i64, count: i64, f: impl Fn(f32) -> f32) -> Vec<Sample> {
        (0..count)
            .map(|k| {
                let t_ms = end_ms - 10 * k;
                Sample {
                    t: t_ms * 1_000_000,
                    x: f(t_ms as f32),
                    y: 0.0,
                }
            })
            .collect()
    }

    #[test]
    fn linear_motion_gives_its_speed() {
        // 2 px/ms is 2000 px/s
        let s = series(40, 5, |t| 2.0 * t);
        let v = velocity_of(&s, true).expect("estimate");
        assert!((v - 2000.0).abs() < 1.0, "got {v}");
    }

    #[test]
    fn two_samples_use_the_line_through_them() {
        let s = series(10, 2, |t| 2.0 * t);
        let v = velocity_of(&s, true).expect("estimate");
        assert!((v - 2000.0).abs() < 1.0, "got {v}");
    }

    #[test]
    fn quadratic_motion_recovers_the_instant_speed() {
        // t in ms: x = t_ms² = 1e6 * t_s²  ⇒  v(t_s) = 2e6 * t_s; at t = 0.04 s: 80000 px/s
        let s = series(40, 5, |t| t * t);
        let v = velocity_of(&s, true).expect("estimate");
        assert!((v - 80_000.0).abs() / 80_000.0 < 0.01, "got {v}");
    }

    #[test]
    fn samples_older_than_the_horizon_are_ignored() {
        // Old samples follow a different speed; only the last 100 ms count.
        let s = series(300, 31, |t| if t < 200.0 { -50.0 * t } else { 2.0 * t });
        let v = velocity_of(&s, true).expect("estimate");
        assert!((v - 2000.0).abs() < 1.0, "got {v}");
    }

    #[test]
    fn a_single_sample_has_no_velocity() {
        assert_eq!(velocity_of(&series(10, 1, |t| t), true), None);
        assert_eq!(velocity_of(&[], true), None);
    }

    #[test]
    fn action_constants_match_aosp() {
        assert_eq!(
            (ACTION_DOWN, ACTION_UP, ACTION_MOVE, ACTION_POINTER_DOWN),
            (0, 1, 2, 5)
        );
        assert_eq!(
            (ACTION_POINTER_UP, ACTION_HOVER_MOVE, ACTION_HOVER_ENTER),
            (6, 7, 9)
        );
    }
}
