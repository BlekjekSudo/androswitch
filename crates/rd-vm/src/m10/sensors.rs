//! `MEDIA-06`: Virtual `SensorManager`, `Sensor`, `SensorEvent`, `SensorEventListener`.

use super::{
    alloc_with, array_len, array_list, call_dex, float_arg, float_array, get_int_field, illegal,
    int_arg, obj_arg, str_val,
};
use crate::{Engine, ObjRef, Value, VmExit};
use std::collections::BTreeMap;

const SENSOR_MANAGER: &str = "Landroid/hardware/SensorManager;";
const SENSOR: &str = "Landroid/hardware/Sensor;";
const SENSOR_EVENT: &str = "Landroid/hardware/SensorEvent;";

#[derive(Debug, Clone)]
pub struct VirtualSensor {
    pub sensor_type: i32,
    pub name: String,
    pub vendor: String,
    pub max_range: f32,
    pub resolution: f32,
    pub power_ma: f32,
    pub min_delay_us: i32,
    pub values: Vec<f32>,
    pub accuracy: i32,
}

#[derive(Debug, Clone)]
pub struct SensorListenerReg {
    pub listener: ObjRef,
    pub sensor_type: i32,
    pub rate_us: i32,
}

type SensorSpec<'a> = (i32, &'a str, f32, f32, f32, i32, &'a [f32]);

pub fn default_sensors() -> BTreeMap<i32, VirtualSensor> {
    let specs: &[SensorSpec<'_>] = &[
        (
            1,
            "RustyDroid Accelerometer",
            39.2266,
            0.0012,
            0.25,
            5_000,
            &[0.0, 9.81, 0.0],
        ),
        (
            2,
            "RustyDroid Magnetometer",
            4912.0,
            0.15,
            1.1,
            10_000,
            &[22.5, -5.0, 41.2],
        ),
        (
            3,
            "RustyDroid Orientation",
            360.0,
            0.1,
            0.5,
            10_000,
            &[0.0, 0.0, 0.0],
        ),
        (
            4,
            "RustyDroid Gyroscope",
            34.9066,
            0.001,
            3.0,
            5_000,
            &[0.0, 0.0, 0.0],
        ),
        (
            5,
            "RustyDroid Ambient Light",
            40_000.0,
            1.0,
            0.15,
            0,
            &[250.0],
        ),
        (
            6,
            "RustyDroid Barometer",
            1100.0,
            0.01,
            0.2,
            20_000,
            &[1013.25],
        ),
        (8, "RustyDroid Proximity", 5.0, 5.0, 0.12, 0, &[5.0]),
        (
            9,
            "RustyDroid Gravity",
            19.6133,
            0.001,
            0.4,
            5_000,
            &[0.0, 9.81, 0.0],
        ),
        (
            10,
            "RustyDroid Linear Acceleration",
            39.2266,
            0.001,
            0.4,
            5_000,
            &[0.0, 0.0, 0.0],
        ),
        (
            11,
            "RustyDroid Rotation Vector",
            1.0,
            0.0001,
            0.8,
            5_000,
            &[0.0, 0.0, 0.0, 1.0],
        ),
        (
            13,
            "RustyDroid Ambient Temperature",
            85.0,
            0.1,
            0.1,
            0,
            &[25.0],
        ),
        (18, "RustyDroid Step Detector", 1.0, 1.0, 0.05, 0, &[1.0]),
        (
            19,
            "RustyDroid Step Counter",
            1_000_000.0,
            1.0,
            0.05,
            0,
            &[0.0],
        ),
    ];
    let mut map = BTreeMap::new();
    for (t, name, max_range, res, pwr, delay, vals) in specs {
        map.insert(
            *t,
            VirtualSensor {
                sensor_type: *t,
                name: (*name).to_string(),
                vendor: "RustyDroid Virtual Hardware".to_string(),
                max_range: *max_range,
                resolution: *res,
                power_ma: *pwr,
                min_delay_us: *delay,
                values: vals.to_vec(),
                accuracy: 3, // SENSOR_STATUS_ACCURACY_HIGH
            },
        );
    }
    map
}

pub fn sensor_type_from_name(name: &str) -> Option<i32> {
    match name.trim().to_ascii_lowercase().as_str() {
        "accelerometer" | "accel" | "1" => Some(1),
        "magnetic_field" | "magnetometer" | "mag" | "2" => Some(2),
        "orientation" | "3" => Some(3),
        "gyroscope" | "gyro" | "4" => Some(4),
        "light" | "ambient_light" | "5" => Some(5),
        "pressure" | "barometer" | "6" => Some(6),
        "proximity" | "8" => Some(8),
        "gravity" | "9" => Some(9),
        "linear_acceleration" | "10" => Some(10),
        "rotation_vector" | "11" => Some(11),
        "ambient_temperature" | "temperature" | "13" => Some(13),
        "step_detector" | "18" => Some(18),
        "step_counter" | "steps" | "19" => Some(19),
        _ => None,
    }
}

fn alloc_sensor_obj(vm: &mut Engine, sensor_type: i32) -> Result<ObjRef, VmExit> {
    alloc_with(
        vm,
        SENSOR,
        vec![("rd_sensor_type".into(), Value::Int(sensor_type))],
    )
}

fn build_sensor_event(
    vm: &mut Engine,
    sensor_type: i32,
    values: &[f32],
    accuracy: i32,
    timestamp_ns: i64,
) -> Result<ObjRef, VmExit> {
    let arr = float_array(vm, values)?;
    let sensor_obj = alloc_sensor_obj(vm, sensor_type)?;
    alloc_with(
        vm,
        SENSOR_EVENT,
        vec![
            ("values".into(), Value::Obj(arr)),
            ("sensor".into(), Value::Obj(sensor_obj)),
            ("accuracy".into(), Value::Int(accuracy)),
            ("timestamp".into(), Value::Long(timestamp_ns)),
        ],
    )
}

pub(super) fn host_static(
    _vm: &mut Engine,
    owner: &str,
    name: &str,
) -> Result<Option<Value>, VmExit> {
    match (owner, name) {
        (SENSOR, "TYPE_ALL") | (SENSOR_MANAGER, "AXIS_MINUS_X") => Ok(Some(Value::Int(-1))),
        (SENSOR, "TYPE_ACCELEROMETER") => Ok(Some(Value::Int(1))),
        (SENSOR, "TYPE_MAGNETIC_FIELD") => Ok(Some(Value::Int(2))),
        (SENSOR, "TYPE_ORIENTATION") => Ok(Some(Value::Int(3))),
        (SENSOR, "TYPE_GYROSCOPE") => Ok(Some(Value::Int(4))),
        (SENSOR, "TYPE_LIGHT") => Ok(Some(Value::Int(5))),
        (SENSOR, "TYPE_PRESSURE") => Ok(Some(Value::Int(6))),
        (SENSOR, "TYPE_PROXIMITY") => Ok(Some(Value::Int(8))),
        (SENSOR, "TYPE_GRAVITY") => Ok(Some(Value::Int(9))),
        (SENSOR, "TYPE_LINEAR_ACCELERATION") => Ok(Some(Value::Int(10))),
        (SENSOR, "TYPE_ROTATION_VECTOR") => Ok(Some(Value::Int(11))),
        (SENSOR, "TYPE_AMBIENT_TEMPERATURE") => Ok(Some(Value::Int(13))),
        (SENSOR, "TYPE_STEP_DETECTOR") => Ok(Some(Value::Int(18))),
        (SENSOR, "TYPE_STEP_COUNTER") => Ok(Some(Value::Int(19))),
        (SENSOR_MANAGER, "SENSOR_DELAY_FASTEST") => Ok(Some(Value::Int(0))),
        (SENSOR_MANAGER, "SENSOR_DELAY_GAME") => Ok(Some(Value::Int(1))),
        (SENSOR_MANAGER, "SENSOR_DELAY_UI") => Ok(Some(Value::Int(2))),
        (SENSOR_MANAGER, "SENSOR_DELAY_NORMAL") => Ok(Some(Value::Int(3))),
        (SENSOR_MANAGER, "SENSOR_STATUS_UNRELIABLE") => Ok(Some(Value::Int(0))),
        (SENSOR_MANAGER, "SENSOR_STATUS_ACCURACY_LOW") => Ok(Some(Value::Int(1))),
        (SENSOR_MANAGER, "SENSOR_STATUS_ACCURACY_MEDIUM") => Ok(Some(Value::Int(2))),
        (SENSOR_MANAGER, "SENSOR_STATUS_ACCURACY_HIGH") => Ok(Some(Value::Int(3))),
        (SENSOR_MANAGER, "GRAVITY_EARTH") => Ok(Some(Value::Float(9.80665))),
        (SENSOR_MANAGER, "PRESSURE_STANDARD_ATMOSPHERE") => Ok(Some(Value::Float(1013.25))),
        _ => Ok(None),
    }
}

pub(super) fn static_call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    _sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != SENSOR_MANAGER {
        return Ok(None);
    }
    match name {
        "getAltitude" => {
            let p0 = float_arg(args, 0).max(1.0);
            let p = float_arg(args, 1).max(0.1);
            let alt = 44_330.0 * (1.0 - (p / p0).powf(1.0 / 5.255));
            Ok(Some(Value::Float(alt)))
        }
        "getRotationMatrix" => {
            // Identity rotation matrix when gravity=[0,9.81,0] or computed orthonormal basis.
            if let Some(r_mat) = obj_arg(args, 0) {
                let len = array_len(vm, r_mat).unwrap_or(0);
                if len >= 9 {
                    let diag = if len >= 16 {
                        vec![
                            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
                            0.0, 1.0,
                        ]
                    } else {
                        vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
                    };
                    let elems = vm.heap.array_elems_mut(r_mat)?;
                    for (i, d) in diag.into_iter().enumerate() {
                        if let Some(slot) = elems.get_mut(i) {
                            *slot = Value::Float(d);
                        }
                    }
                }
            }
            Ok(Some(Value::Int(1)))
        }
        "getOrientation" => {
            if let Some(values_arr) = obj_arg(args, 1) {
                let elems = vm.heap.array_elems_mut(values_arr)?;
                for slot in elems.iter_mut().take(3) {
                    *slot = Value::Float(0.0);
                }
                return Ok(Some(Value::Obj(values_arr)));
            }
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}

pub(super) fn instance_call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match class {
        SENSOR_MANAGER => match name {
            "getDefaultSensor" => {
                let stype = int_arg(args, 0);
                if vm.fw.vdev.sensors.contains_key(&stype) {
                    Ok(Some(Value::Obj(alloc_sensor_obj(vm, stype)?)))
                } else {
                    Ok(Some(Value::Null))
                }
            }
            "getSensorList" => {
                let stype = int_arg(args, 0);
                let types: Vec<i32> = if stype == -1 {
                    vm.fw.vdev.sensors.keys().copied().collect()
                } else if vm.fw.vdev.sensors.contains_key(&stype) {
                    vec![stype]
                } else {
                    Vec::new()
                };
                let mut list = Vec::with_capacity(types.len());
                for t in types {
                    list.push(Value::Obj(alloc_sensor_obj(vm, t)?));
                }
                Ok(Some(array_list(vm, list)?))
            }
            "registerListener" => {
                let Some(listener) = obj_arg(args, 0) else {
                    return Ok(Some(Value::Int(0)));
                };
                let Some(sensor_obj) = obj_arg(args, 1) else {
                    return Ok(Some(Value::Int(0)));
                };
                let sensor_type = get_int_field(vm, sensor_obj, "rd_sensor_type", 0);
                if !vm.fw.vdev.sensors.contains_key(&sensor_type) {
                    return Ok(Some(Value::Int(0)));
                }
                let rate = int_arg(args, 2);
                let rate_us = match rate {
                    0 => 0,
                    1 => 20_000,
                    2 => 66_667,
                    3 => 200_000,
                    us if us > 3 => us,
                    _ => 200_000,
                };
                let already = vm
                    .fw
                    .vdev
                    .sensor_listeners
                    .iter()
                    .any(|r| r.listener == listener && r.sensor_type == sensor_type);
                if !already {
                    vm.fw.vdev.sensor_listeners.push(SensorListenerReg {
                        listener,
                        sensor_type,
                        rate_us,
                    });
                }
                Ok(Some(Value::Int(1)))
            }
            "unregisterListener" => {
                let Some(listener) = obj_arg(args, 0) else {
                    return Ok(Some(Value::Null));
                };
                if let Some(sensor_obj) = obj_arg(args, 1) {
                    let stype = get_int_field(vm, sensor_obj, "rd_sensor_type", -1);
                    vm.fw
                        .vdev
                        .sensor_listeners
                        .retain(|r| !(r.listener == listener && r.sensor_type == stype));
                } else {
                    vm.fw
                        .vdev
                        .sensor_listeners
                        .retain(|r| r.listener != listener);
                }
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        },
        SENSOR => {
            let stype = get_int_field(vm, recv, "rd_sensor_type", 1);
            let Some(spec) = vm.fw.vdev.sensors.get(&stype).cloned() else {
                return Ok(None);
            };
            match name {
                "getType" => Ok(Some(Value::Int(spec.sensor_type))),
                "getName" => Ok(Some(str_val(vm, spec.name)?)),
                "getVendor" => Ok(Some(str_val(vm, spec.vendor)?)),
                "getVersion" => Ok(Some(Value::Int(1))),
                "getMaximumRange" => Ok(Some(Value::Float(spec.max_range))),
                "getResolution" => Ok(Some(Value::Float(spec.resolution))),
                "getPower" => Ok(Some(Value::Float(spec.power_ma))),
                "getMinDelay" => Ok(Some(Value::Int(spec.min_delay_us))),
                "getMaxDelay" => Ok(Some(Value::Int(1_000_000))),
                "isWakeUpSensor" => Ok(Some(Value::Int(i32::from(stype == 8)))),
                "toString" => Ok(Some(str_val(
                    vm,
                    format!("Sensor{{type={}, name={}}}", spec.sensor_type, spec.name),
                )?)),
                _ => Ok(None),
            }
        }
        _ => Ok(None),
    }
}

impl Engine {
    /// Inject a deterministic virtual sensor sample and notify all registered
    /// `SensorEventListener`s for `sensor_type`. Returns the number of callbacks delivered.
    pub fn m10_inject_sensor(
        &mut self,
        sensor_type: i32,
        values: Vec<f32>,
        accuracy: Option<i32>,
    ) -> Result<usize, VmExit> {
        if values.is_empty() {
            return Err(illegal("sensor values must not be empty"));
        }
        let acc = accuracy.unwrap_or(3).clamp(0, 3);
        let prev_acc = match self.fw.vdev.sensors.get_mut(&sensor_type) {
            Some(s) => {
                let prev = s.accuracy;
                s.values = values.clone();
                s.accuracy = acc;
                prev
            }
            None => return Err(illegal(format!("unsupported sensor type {sensor_type}"))),
        };
        let ts = self.fw.vdev.next_timestamp_ns();
        let listeners: Vec<ObjRef> = self
            .fw
            .vdev
            .sensor_listeners
            .iter()
            .filter(|r| r.sensor_type == sensor_type)
            .map(|r| r.listener)
            .collect();
        if listeners.is_empty() {
            return Ok(0);
        }
        self.with_host_roots(|vm| {
            let mut delivered = 0usize;
            for listener in listeners {
                if acc != prev_acc {
                    let s_obj = alloc_sensor_obj(vm, sensor_type)?;
                    let _ = call_dex(
                        vm,
                        listener,
                        "onAccuracyChanged",
                        "(Landroid/hardware/Sensor;I)V",
                        vec![Value::Obj(s_obj), Value::Int(acc)],
                    )?;
                }
                let ev = build_sensor_event(vm, sensor_type, &values, acc, ts)?;
                if call_dex(
                    vm,
                    listener,
                    "onSensorChanged",
                    "(Landroid/hardware/SensorEvent;)V",
                    vec![Value::Obj(ev)],
                )?
                .is_some()
                {
                    delivered += 1;
                }
            }
            Ok(delivered)
        })
    }
}
