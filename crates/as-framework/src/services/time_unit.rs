//! API 26 TimeUnit conversions and canonical enum constants.
//!
//! Independent implementation of documented unit ratios, Java truncation and
//! saturation. No upstream implementation code is copied. Blocking helpers
//! (sleep/timedWait/timedJoin), Duration overloads and generic Enum.valueOf's
//! host-enum discovery are outside this contract.
use super::*;

pub(super) const CLASS: &str = "Ljava/util/concurrent/TimeUnit;";
const ARRAY: &str = "[Ljava/util/concurrent/TimeUnit;";
const NAMES: [&str; 7] = [
    "NANOSECONDS",
    "MICROSECONDS",
    "MILLISECONDS",
    "SECONDS",
    "MINUTES",
    "HOURS",
    "DAYS",
];
const NANOS: [i64; 7] = [
    1,
    1_000,
    1_000_000,
    1_000_000_000,
    60_000_000_000,
    3_600_000_000_000,
    86_400_000_000_000,
];

#[derive(Default)]
pub(super) struct TimeUnits {
    constants: RefCell<[Option<ObjRef>; 7]>,
}

/// Every pair has an integral scale ratio. Dividing before multiplying is
/// unnecessary and would lose precision; no intermediate nanosecond product
/// is formed. Rust signed integer division truncates toward zero, as Java does.
fn convert(duration: i64, source: usize, target: usize) -> i64 {
    if source > target {
        duration.saturating_mul(NANOS[source] / NANOS[target])
    } else {
        duration / (NANOS[target] / NANOS[source])
    }
}

fn argument(args: &[Value], index: usize) -> Result<&Value> {
    args.get(index)
        .ok_or_else(|| error("INVALID_ARGUMENT", "TimeUnit argument count"))
}

impl TimeUnits {
    pub(super) fn metadata(class: &str, name: &str, sig: &str) -> Option<HostMethodKind> {
        use HostMethodKind::{Instance, Static};
        match (class, name, sig) {
            (CLASS, "values", "()[Ljava/util/concurrent/TimeUnit;")
            | (CLASS, "valueOf", "(Ljava/lang/String;)Ljava/util/concurrent/TimeUnit;") => {
                Some(Static)
            }
            (CLASS, "convert", "(JLjava/util/concurrent/TimeUnit;)J")
            | (
                CLASS,
                "toNanos" | "toMicros" | "toMillis" | "toSeconds" | "toMinutes" | "toHours"
                | "toDays",
                "(J)J",
            ) => Some(Instance),
            _ => None,
        }
    }

    fn constant(&self, vm: &mut Engine, index: usize) -> Result<ObjRef> {
        if let Some(value) = self.constants.borrow()[index] {
            return Ok(value);
        }
        // Enum's real private name/ordinal fields live on the budgeted heap;
        // inherited name/ordinal/compareTo/getDeclaringClass use that state.
        // This intrinsic invokes no Java/native callback or collection, so the
        // partially constructed object cannot escape or cross VM suspension.
        let reference = shell(vm, CLASS)?;
        let name = vm.heap.alloc_string(NAMES[index].into())?;
        rd_vm::intrinsics::call_instance_intrinsic_mref(
            vm,
            CLASS,
            "Ljava/lang/Enum;",
            "<init>",
            "(Ljava/lang/String;I)V",
            reference,
            &[Value::Obj(name), Value::Int(index as i32)],
        )?
        .ok_or_else(|| unavailable("TimeUnit Enum initialization"))?;
        self.constants.borrow_mut()[index] = Some(reference);
        Ok(reference)
    }

    fn index(&self, vm: &Engine, reference: ObjRef) -> Result<usize> {
        if vm.heap.class_of(reference)? != CLASS {
            return Err(exception(
                "Ljava/lang/ClassCastException;",
                "TimeUnit argument has another type",
            ));
        }
        self.constants
            .borrow()
            .iter()
            .position(|value| *value == Some(reference))
            .ok_or_else(|| error("TIME_UNIT_STATE_UNINITIALIZED", "noncanonical TimeUnit"))
    }

    pub(super) fn is_seconds(&self, reference: ObjRef) -> bool {
        self.constants.borrow()[3] == Some(reference)
    }

    pub(super) fn roots(&self) -> Vec<ObjRef> {
        self.constants.borrow().iter().copied().flatten().collect()
    }

    pub(super) fn static_field(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
    ) -> Result<Option<Value>> {
        if class != CLASS || sig != CLASS {
            return Ok(None);
        }
        let Some(index) = NAMES.iter().position(|value| *value == name) else {
            return Ok(None);
        };
        Ok(Some(Value::Obj(self.constant(vm, index)?)))
    }

    pub(super) fn call_static(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if class != CLASS {
            return Ok(None);
        }
        let value = match (name, sig) {
            ("valueOf", "(Ljava/lang/String;)Ljava/util/concurrent/TimeUnit;") => {
                let wanted = text(vm, argument(args, 0)?)?;
                let index = NAMES
                    .iter()
                    .position(|name| *name == wanted)
                    .ok_or_else(|| {
                        invalid(&format!(
                            "No enum constant java.util.concurrent.TimeUnit.{wanted}"
                        ))
                    })?;
                Value::Obj(self.constant(vm, index)?)
            }
            ("values", "()[Ljava/util/concurrent/TimeUnit;") => {
                for index in 0..NAMES.len() {
                    self.constant(vm, index)?;
                }
                // values() owns a fresh mutable array, with canonical members.
                let array =
                    vm.heap
                        .alloc_array_zeroed(rd_vm::heap::ElemKind::Obj, NAMES.len(), ARRAY)?;
                for (element, reference) in vm
                    .heap
                    .array_elems_mut(array)?
                    .iter_mut()
                    .zip(self.constants.borrow().iter())
                {
                    *element = Value::Obj(reference.ok_or_else(|| {
                        error("TIME_UNIT_STATE_UNINITIALIZED", "constant construction")
                    })?);
                }
                Value::Obj(array)
            }
            _ => return Ok(None),
        };
        Ok(Some(value))
    }

    pub(super) fn call(
        &self,
        vm: &Engine,
        actual: &str,
        reference: ObjRef,
        name: &str,
        sig: &str,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if actual != CLASS {
            return Ok(None);
        }
        let target = match (name, sig) {
            ("convert", "(JLjava/util/concurrent/TimeUnit;)J") => {
                let source = self.index(vm, obj(argument(args, 1)?)?)?;
                let target = self.index(vm, reference)?;
                return Ok(Some(Value::Long(convert(
                    argument(args, 0)?.as_long()?,
                    source,
                    target,
                ))));
            }
            ("toNanos", "(J)J") => 0,
            ("toMicros", "(J)J") => 1,
            ("toMillis", "(J)J") => 2,
            ("toSeconds", "(J)J") => 3,
            ("toMinutes", "(J)J") => 4,
            ("toHours", "(J)J") => 5,
            ("toDays", "(J)J") => 6,
            _ => return Ok(None),
        };
        Ok(Some(Value::Long(convert(
            argument(args, 0)?.as_long()?,
            self.index(vm, reference)?,
            target,
        ))))
    }
}
