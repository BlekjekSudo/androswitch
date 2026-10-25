//! `MEDIA-07`: Virtual GPS / Network / Fused location (`LocationManager`, `Location`,
//! `LocationListener`, `FusedLocationProviderClient`, `LocationCallback`, `LocationResult`)
//! and deterministic GPX / waypoint route playback.

use super::{
    alloc, alloc_with, array_len, array_list, bool_arg, call_dex, check_any_perm, double_arg,
    float_arg, get_double_field, get_field, get_float_field, get_int_field, get_long_field,
    get_str_field, illegal, long_arg, obj_arg, opt_str_arg, put_field, str_val,
};
use crate::{Engine, ObjRef, Value, VmExit};

const LOCATION_MANAGER: &str = "Landroid/location/LocationManager;";
const LOCATION: &str = "Landroid/location/Location;";
const CRITERIA: &str = "Landroid/location/Criteria;";
const LOCATION_SERVICES: &str = "Lcom/google/android/gms/location/LocationServices;";
const FUSED_CLIENT: &str = "Lcom/google/android/gms/location/FusedLocationProviderClient;";
const LOCATION_REQUEST: &str = "Lcom/google/android/gms/location/LocationRequest;";
const LOCATION_REQUEST_BUILDER: &str = "Lcom/google/android/gms/location/LocationRequest$Builder;";
const LOCATION_CALLBACK: &str = "Lcom/google/android/gms/location/LocationCallback;";
const LOCATION_RESULT: &str = "Lcom/google/android/gms/location/LocationResult;";
const GMS_TASK: &str = "Lcom/google/android/gms/tasks/Task;";
const GMS_TASKS: &str = "Lcom/google/android/gms/tasks/Tasks;";

const LOCATION_PERMS: &[&str] = &[
    "android.permission.ACCESS_FINE_LOCATION",
    "android.permission.ACCESS_COARSE_LOCATION",
];

#[derive(Debug, Clone)]
pub struct LocationFix {
    pub provider: String,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: f64,
    pub accuracy: f32,
    pub speed: f32,
    pub bearing: f32,
    pub time_ms: i64,
    pub elapsed_ns: i64,
}

impl Default for LocationFix {
    fn default() -> Self {
        Self {
            provider: "gps".into(),
            latitude: -23.5505,
            longitude: -46.6333,
            altitude: 760.0,
            accuracy: 5.0,
            speed: 0.0,
            bearing: 0.0,
            time_ms: 1_792_886_400_000,
            elapsed_ns: 1_000_000_000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LocationListenerReg {
    pub listener: ObjRef,
    pub provider: String,
    pub fused_callback: bool,
    pub single_shot: bool,
}

#[derive(Debug, Clone)]
pub struct LocationState {
    pub gps_enabled: bool,
    pub network_enabled: bool,
    pub current: LocationFix,
    pub route: Vec<LocationFix>,
    pub route_cursor: usize,
    pub listeners: Vec<LocationListenerReg>,
}

impl Default for LocationState {
    fn default() -> Self {
        Self {
            gps_enabled: true,
            network_enabled: true,
            current: LocationFix::default(),
            route: Vec::new(),
            route_cursor: 0,
            listeners: Vec::new(),
        }
    }
}

fn build_location_obj(vm: &mut Engine, fix: &LocationFix) -> Result<ObjRef, VmExit> {
    let prov = str_val(vm, fix.provider.clone())?;
    alloc_with(
        vm,
        LOCATION,
        vec![
            ("rd_provider".into(), prov),
            ("rd_lat".into(), Value::Double(fix.latitude)),
            ("rd_lon".into(), Value::Double(fix.longitude)),
            ("rd_alt".into(), Value::Double(fix.altitude)),
            ("rd_has_alt".into(), Value::Int(1)),
            ("rd_acc".into(), Value::Float(fix.accuracy)),
            ("rd_has_acc".into(), Value::Int(1)),
            ("rd_speed".into(), Value::Float(fix.speed)),
            ("rd_has_speed".into(), Value::Int(1)),
            ("rd_bearing".into(), Value::Float(fix.bearing)),
            ("rd_has_bearing".into(), Value::Int(1)),
            ("rd_time".into(), Value::Long(fix.time_ms)),
            ("rd_elapsed".into(), Value::Long(fix.elapsed_ns)),
        ],
    )
}

fn completed_task(vm: &mut Engine, result: Value) -> Result<ObjRef, VmExit> {
    alloc_with(
        vm,
        GMS_TASK,
        vec![
            ("rd_complete".into(), Value::Int(1)),
            ("rd_success".into(), Value::Int(1)),
            ("rd_result".into(), result),
        ],
    )
}

fn haversine_distance_and_bearing(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> (f32, f32, f32) {
    let r = 6_371_008.8_f64;
    let p1 = lat1.to_radians();
    let p2 = lat2.to_radians();
    let dp = (lat2 - lat1).to_radians();
    let dl = (lon2 - lon1).to_radians();
    let a = (dp * 0.5).sin().powi(2) + p1.cos() * p2.cos() * (dl * 0.5).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).max(0.0).sqrt());
    let dist = (r * c) as f32;

    let y = dl.sin() * p2.cos();
    let x = p1.cos() * p2.sin() - p1.sin() * p2.cos() * dl.cos();
    let initial = y.atan2(x).to_degrees() as f32;

    let y2 = (-dl).sin() * p1.cos();
    let x2 = p2.cos() * p1.sin() - p2.sin() * p1.cos() * (-dl).cos();
    let rev = ((y2.atan2(x2).to_degrees() + 180.0) % 360.0) as f32;
    (dist, initial, rev)
}

pub(super) fn host_static(
    vm: &mut Engine,
    owner: &str,
    name: &str,
) -> Result<Option<Value>, VmExit> {
    match (owner, name) {
        (LOCATION_MANAGER, "GPS_PROVIDER") => str_val(vm, "gps").map(Some),
        (LOCATION_MANAGER, "NETWORK_PROVIDER") => str_val(vm, "network").map(Some),
        (LOCATION_MANAGER, "PASSIVE_PROVIDER") => str_val(vm, "passive").map(Some),
        (LOCATION_MANAGER, "FUSED_PROVIDER") => str_val(vm, "fused").map(Some),
        (LOCATION_MANAGER, "KEY_LOCATION_CHANGED") => str_val(vm, "location").map(Some),
        (CRITERIA, "ACCURACY_FINE") => Ok(Some(Value::Int(1))),
        (CRITERIA, "ACCURACY_COARSE") => Ok(Some(Value::Int(2))),
        (LOCATION_REQUEST, "PRIORITY_HIGH_ACCURACY") => Ok(Some(Value::Int(100))),
        (LOCATION_REQUEST, "PRIORITY_BALANCED_POWER_ACCURACY") => Ok(Some(Value::Int(102))),
        (LOCATION_REQUEST, "PRIORITY_LOW_POWER") => Ok(Some(Value::Int(104))),
        (LOCATION_REQUEST, "PRIORITY_NO_POWER") => Ok(Some(Value::Int(105))),
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
    match (class, name) {
        (LOCATION, "distanceBetween") => {
            let lat1 = double_arg(args, 0);
            let lon1 = double_arg(args, 1);
            let lat2 = double_arg(args, 2);
            let lon2 = double_arg(args, 3);
            let Some(arr) = obj_arg(args, 4) else {
                return Err(illegal("results array is null or length < 1"));
            };
            let len = array_len(vm, arr).unwrap_or(0);
            if len < 1 {
                return Err(illegal("results array length < 1"));
            }
            let (dist, b1, b2) = haversine_distance_and_bearing(lat1, lon1, lat2, lon2);
            let elems = vm.heap.array_elems_mut(arr)?;
            elems[0] = Value::Float(dist);
            if elems.len() >= 2 {
                elems[1] = Value::Float(b1);
            }
            if elems.len() >= 3 {
                elems[2] = Value::Float(b2);
            }
            Ok(Some(Value::Null))
        }
        (LOCATION_SERVICES, "getFusedLocationProviderClient") => {
            Ok(Some(Value::Obj(alloc(vm, FUSED_CLIENT)?)))
        }
        (LOCATION_REQUEST, "create") => Ok(Some(Value::Obj(alloc(vm, LOCATION_REQUEST)?))),
        (LOCATION_RESULT, "create") => {
            let list_val = args.first().cloned().unwrap_or(Value::Null);
            let r = alloc_with(vm, LOCATION_RESULT, vec![("rd_locations".into(), list_val)])?;
            Ok(Some(Value::Obj(r)))
        }
        (GMS_TASKS, "forResult") => {
            let res = args.first().cloned().unwrap_or(Value::Null);
            Ok(Some(Value::Obj(completed_task(vm, res)?)))
        }
        _ => Ok(None),
    }
}

pub(super) fn instance_call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match class {
        LOCATION_MANAGER => match name {
            "isLocationEnabled" => {
                let on = vm.fw.vdev.location.gps_enabled || vm.fw.vdev.location.network_enabled;
                Ok(Some(Value::Int(i32::from(on))))
            }
            "isProviderEnabled" => {
                let p = opt_str_arg(vm, args, 0)?.unwrap_or_default();
                let on = match p.as_str() {
                    "gps" | "fused" => vm.fw.vdev.location.gps_enabled,
                    "network" => vm.fw.vdev.location.network_enabled,
                    "passive" => true,
                    _ => false,
                };
                Ok(Some(Value::Int(i32::from(on))))
            }
            "getAllProviders" | "getProviders" => {
                let enabled_only = name == "getProviders" && bool_arg(args, 0);
                let mut provs = Vec::new();
                if !enabled_only || vm.fw.vdev.location.gps_enabled {
                    provs.push(str_val(vm, "gps")?);
                    provs.push(str_val(vm, "fused")?);
                }
                if !enabled_only || vm.fw.vdev.location.network_enabled {
                    provs.push(str_val(vm, "network")?);
                }
                provs.push(str_val(vm, "passive")?);
                Ok(Some(array_list(vm, provs)?))
            }
            "getBestProvider" => {
                if vm.fw.vdev.location.gps_enabled {
                    Ok(Some(str_val(vm, "gps")?))
                } else if vm.fw.vdev.location.network_enabled {
                    Ok(Some(str_val(vm, "network")?))
                } else {
                    Ok(Some(Value::Null))
                }
            }
            "getLastKnownLocation" => {
                check_any_perm(vm, LOCATION_PERMS)?;
                let prov = opt_str_arg(vm, args, 0)?.unwrap_or_else(|| "gps".into());
                let enabled = match prov.as_str() {
                    "gps" | "fused" => vm.fw.vdev.location.gps_enabled,
                    "network" => vm.fw.vdev.location.network_enabled,
                    "passive" => true,
                    _ => false,
                };
                if !enabled {
                    return Ok(Some(Value::Null));
                }
                let mut fix = vm.fw.vdev.location.current.clone();
                fix.provider = prov;
                Ok(Some(Value::Obj(build_location_obj(vm, &fix)?)))
            }
            "requestLocationUpdates" | "requestSingleUpdate" => {
                check_any_perm(vm, LOCATION_PERMS)?;
                let single_shot = name == "requestSingleUpdate";
                let provider = opt_str_arg(vm, args, 0)?.unwrap_or_else(|| "gps".into());
                // Find the LocationListener object argument across overloads:
                // (String, long, float, LocationListener[, Looper]) or
                // (String, long, float, Executor, LocationListener) or
                // (String, LocationListener, Looper)
                let listener = args.iter().rev().find_map(|v| match v {
                    Value::Obj(r) => {
                        let c = vm.heap.class_of(*r).ok().unwrap_or("");
                        if c != "Ljava/lang/String;" && c != "Landroid/os/Looper;" && c != CRITERIA
                        {
                            Some(*r)
                        } else {
                            None
                        }
                    }
                    _ => None,
                });
                if let Some(l) = listener {
                    let exists = vm
                        .fw
                        .vdev
                        .location
                        .listeners
                        .iter()
                        .any(|reg| reg.listener == l && !reg.fused_callback);
                    if !exists {
                        vm.fw.vdev.location.listeners.push(LocationListenerReg {
                            listener: l,
                            provider,
                            fused_callback: false,
                            single_shot,
                        });
                    }
                }
                Ok(Some(Value::Null))
            }
            "getCurrentLocation" => {
                check_any_perm(vm, LOCATION_PERMS)?;
                let prov = opt_str_arg(vm, args, 0)?.unwrap_or_else(|| "gps".into());
                let mut fix = vm.fw.vdev.location.current.clone();
                fix.provider = prov;
                let loc = build_location_obj(vm, &fix)?;
                if let Some(consumer) = args.last().and_then(|v| match v {
                    Value::Obj(r) => Some(*r),
                    _ => None,
                }) {
                    let _ = call_dex(
                        vm,
                        consumer,
                        "accept",
                        "(Ljava/lang/Object;)V",
                        vec![Value::Obj(loc)],
                    )?;
                }
                Ok(Some(Value::Null))
            }
            "removeUpdates" => {
                if let Some(l) = obj_arg(args, 0) {
                    vm.fw.vdev.location.listeners.retain(|r| r.listener != l);
                }
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        },
        LOCATION => match name {
            "<init>" => {
                if sig == "(Landroid/location/Location;)V" {
                    if let Some(src) = obj_arg(args, 0) {
                        for k in [
                            "rd_provider",
                            "rd_lat",
                            "rd_lon",
                            "rd_alt",
                            "rd_has_alt",
                            "rd_acc",
                            "rd_has_acc",
                            "rd_speed",
                            "rd_has_speed",
                            "rd_bearing",
                            "rd_has_bearing",
                            "rd_time",
                            "rd_elapsed",
                        ] {
                            if let Some(v) = get_field(vm, src, k) {
                                put_field(vm, recv, k, v)?;
                            }
                        }
                    }
                } else {
                    let p = opt_str_arg(vm, args, 0)?.unwrap_or_default();
                    let pv = str_val(vm, p)?;
                    put_field(vm, recv, "rd_provider", pv)?;
                }
                Ok(Some(Value::Null))
            }
            "getProvider" => Ok(Some(
                get_field(vm, recv, "rd_provider").unwrap_or(Value::Null),
            )),
            "setProvider" => {
                let p = args.first().cloned().unwrap_or(Value::Null);
                put_field(vm, recv, "rd_provider", p)?;
                Ok(Some(Value::Null))
            }
            "getLatitude" => Ok(Some(Value::Double(get_double_field(
                vm, recv, "rd_lat", 0.0,
            )))),
            "setLatitude" => {
                put_field(vm, recv, "rd_lat", Value::Double(double_arg(args, 0)))?;
                Ok(Some(Value::Null))
            }
            "getLongitude" => Ok(Some(Value::Double(get_double_field(
                vm, recv, "rd_lon", 0.0,
            )))),
            "setLongitude" => {
                put_field(vm, recv, "rd_lon", Value::Double(double_arg(args, 0)))?;
                Ok(Some(Value::Null))
            }
            "getAltitude" => Ok(Some(Value::Double(get_double_field(
                vm, recv, "rd_alt", 0.0,
            )))),
            "setAltitude" => {
                put_field(vm, recv, "rd_alt", Value::Double(double_arg(args, 0)))?;
                put_field(vm, recv, "rd_has_alt", Value::Int(1))?;
                Ok(Some(Value::Null))
            }
            "hasAltitude" => Ok(Some(Value::Int(get_int_field(vm, recv, "rd_has_alt", 0)))),
            "getAccuracy" => Ok(Some(Value::Float(get_float_field(vm, recv, "rd_acc", 0.0)))),
            "setAccuracy" => {
                put_field(vm, recv, "rd_acc", Value::Float(float_arg(args, 0)))?;
                put_field(vm, recv, "rd_has_acc", Value::Int(1))?;
                Ok(Some(Value::Null))
            }
            "hasAccuracy" => Ok(Some(Value::Int(get_int_field(vm, recv, "rd_has_acc", 0)))),
            "getSpeed" => Ok(Some(Value::Float(get_float_field(
                vm, recv, "rd_speed", 0.0,
            )))),
            "setSpeed" => {
                put_field(vm, recv, "rd_speed", Value::Float(float_arg(args, 0)))?;
                put_field(vm, recv, "rd_has_speed", Value::Int(1))?;
                Ok(Some(Value::Null))
            }
            "hasSpeed" => Ok(Some(Value::Int(get_int_field(vm, recv, "rd_has_speed", 0)))),
            "getBearing" => Ok(Some(Value::Float(get_float_field(
                vm,
                recv,
                "rd_bearing",
                0.0,
            )))),
            "setBearing" => {
                put_field(vm, recv, "rd_bearing", Value::Float(float_arg(args, 0)))?;
                put_field(vm, recv, "rd_has_bearing", Value::Int(1))?;
                Ok(Some(Value::Null))
            }
            "hasBearing" => Ok(Some(Value::Int(get_int_field(
                vm,
                recv,
                "rd_has_bearing",
                0,
            )))),
            "getTime" => Ok(Some(Value::Long(get_long_field(vm, recv, "rd_time", 0)))),
            "setTime" => {
                put_field(vm, recv, "rd_time", Value::Long(long_arg(args, 0)))?;
                Ok(Some(Value::Null))
            }
            "getElapsedRealtimeNanos" => {
                Ok(Some(Value::Long(get_long_field(vm, recv, "rd_elapsed", 0))))
            }
            "setElapsedRealtimeNanos" => {
                put_field(vm, recv, "rd_elapsed", Value::Long(long_arg(args, 0)))?;
                Ok(Some(Value::Null))
            }
            "isFromMockProvider" | "isMock" => Ok(Some(Value::Int(0))),
            "distanceTo" => {
                let Some(other) = obj_arg(args, 0) else {
                    return Err(illegal("other Location is null"));
                };
                let (d, _, _) = haversine_distance_and_bearing(
                    get_double_field(vm, recv, "rd_lat", 0.0),
                    get_double_field(vm, recv, "rd_lon", 0.0),
                    get_double_field(vm, other, "rd_lat", 0.0),
                    get_double_field(vm, other, "rd_lon", 0.0),
                );
                Ok(Some(Value::Float(d)))
            }
            "bearingTo" => {
                let Some(other) = obj_arg(args, 0) else {
                    return Err(illegal("other Location is null"));
                };
                let (_, b, _) = haversine_distance_and_bearing(
                    get_double_field(vm, recv, "rd_lat", 0.0),
                    get_double_field(vm, recv, "rd_lon", 0.0),
                    get_double_field(vm, other, "rd_lat", 0.0),
                    get_double_field(vm, other, "rd_lon", 0.0),
                );
                Ok(Some(Value::Float(b)))
            }
            "toString" => {
                let p = get_str_field(vm, recv, "rd_provider").unwrap_or_else(|| "gps".into());
                let lat = get_double_field(vm, recv, "rd_lat", 0.0);
                let lon = get_double_field(vm, recv, "rd_lon", 0.0);
                Ok(Some(str_val(
                    vm,
                    format!("Location[{p} {lat:.5},{lon:.5}]"),
                )?))
            }
            _ => Ok(None),
        },
        CRITERIA => match name {
            "<init>"
            | "setAccuracy"
            | "setPowerRequirement"
            | "setAltitudeRequired"
            | "setSpeedRequired"
            | "setBearingRequired"
            | "setCostAllowed" => Ok(Some(Value::Null)),
            _ => Ok(None),
        },
        LOCATION_CALLBACK => match name {
            "<init>" => Ok(Some(Value::Null)),
            _ => Ok(None),
        },
        LOCATION_REQUEST | LOCATION_REQUEST_BUILDER => match name {
            "<init>" => Ok(Some(Value::Null)),
            "build" => Ok(Some(Value::Obj(alloc(vm, LOCATION_REQUEST)?))),
            "setInterval"
            | "setFastestInterval"
            | "setPriority"
            | "setMinUpdateIntervalMillis"
            | "setMinUpdateDistanceMeters"
            | "setMaxUpdates"
            | "setGranularity"
            | "setWaitForAccurateLocation" => Ok(Some(Value::Obj(recv))),
            _ => Ok(None),
        },
        LOCATION_RESULT => match name {
            "getLastLocation" => {
                if let Some(Value::Obj(list_ref)) = get_field(vm, recv, "rd_locations") {
                    if let Some(crate::framework::HostObj::List { elems }) =
                        vm.fw.objects.get(&list_ref)
                    {
                        return Ok(Some(elems.last().cloned().unwrap_or(Value::Null)));
                    }
                }
                Ok(Some(Value::Null))
            }
            "getLocations" => Ok(Some(
                get_field(vm, recv, "rd_locations").unwrap_or(Value::Null),
            )),
            _ => Ok(None),
        },
        FUSED_CLIENT => match name {
            "getLastLocation" | "getCurrentLocation" => {
                check_any_perm(vm, LOCATION_PERMS)?;
                let mut fix = vm.fw.vdev.location.current.clone();
                fix.provider = "fused".into();
                let loc = build_location_obj(vm, &fix)?;
                Ok(Some(Value::Obj(completed_task(vm, Value::Obj(loc))?)))
            }
            "requestLocationUpdates" => {
                check_any_perm(vm, LOCATION_PERMS)?;
                // (LocationRequest, LocationCallback, Looper) or (LocationRequest, Executor, LocationCallback)
                let cb = args.iter().skip(1).find_map(|v| match v {
                    Value::Obj(r) => {
                        let c = vm.heap.class_of(*r).ok().unwrap_or("");
                        if c != "Landroid/os/Looper;" {
                            Some(*r)
                        } else {
                            None
                        }
                    }
                    _ => None,
                });
                if let Some(cb_ref) = cb {
                    let exists = vm
                        .fw
                        .vdev
                        .location
                        .listeners
                        .iter()
                        .any(|r| r.listener == cb_ref && r.fused_callback);
                    if !exists {
                        vm.fw.vdev.location.listeners.push(LocationListenerReg {
                            listener: cb_ref,
                            provider: "fused".into(),
                            fused_callback: true,
                            single_shot: false,
                        });
                    }
                }
                Ok(Some(Value::Obj(completed_task(vm, Value::Null)?)))
            }
            "removeLocationUpdates" => {
                if let Some(cb_ref) = obj_arg(args, 0) {
                    vm.fw
                        .vdev
                        .location
                        .listeners
                        .retain(|r| r.listener != cb_ref);
                }
                Ok(Some(Value::Obj(completed_task(vm, Value::Null)?)))
            }
            "flushLocations" => Ok(Some(Value::Obj(completed_task(vm, Value::Null)?))),
            _ => Ok(None),
        },
        GMS_TASK => match name {
            "isComplete" => Ok(Some(Value::Int(get_int_field(vm, recv, "rd_complete", 1)))),
            "isSuccessful" => Ok(Some(Value::Int(get_int_field(vm, recv, "rd_success", 1)))),
            "isCanceled" => Ok(Some(Value::Int(0))),
            "getResult" => Ok(Some(
                get_field(vm, recv, "rd_result").unwrap_or(Value::Null),
            )),
            "getException" => Ok(Some(Value::Null)),
            "addOnSuccessListener" => {
                if get_int_field(vm, recv, "rd_success", 1) != 0 {
                    if let Some(listener) = args.last().and_then(|v| match v {
                        Value::Obj(r) => Some(*r),
                        _ => None,
                    }) {
                        let res = get_field(vm, recv, "rd_result").unwrap_or(Value::Null);
                        let _ = call_dex(
                            vm,
                            listener,
                            "onSuccess",
                            "(Ljava/lang/Object;)V",
                            vec![res],
                        )?;
                    }
                }
                Ok(Some(Value::Obj(recv)))
            }
            "addOnCompleteListener" => {
                if get_int_field(vm, recv, "rd_complete", 1) != 0 {
                    if let Some(listener) = args.last().and_then(|v| match v {
                        Value::Obj(r) => Some(*r),
                        _ => None,
                    }) {
                        let _ = call_dex(
                            vm,
                            listener,
                            "onComplete",
                            "(Lcom/google/android/gms/tasks/Task;)V",
                            vec![Value::Obj(recv)],
                        )?;
                    }
                }
                Ok(Some(Value::Obj(recv)))
            }
            "addOnFailureListener" | "addOnCanceledListener" => Ok(Some(Value::Obj(recv))),
            _ => Ok(None),
        },
        _ => Ok(None),
    }
}

impl Engine {
    /// Update the virtual GPS/Fused fix and synchronously deliver callbacks to
    /// registered `LocationListener`s and `LocationCallback`s.
    pub fn m10_set_location(
        &mut self,
        latitude: f64,
        longitude: f64,
        altitude: Option<f64>,
        accuracy: Option<f32>,
        speed: Option<f32>,
        bearing: Option<f32>,
    ) -> Result<usize, VmExit> {
        if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
            return Err(illegal(
                "latitude must be in [-90,90] and longitude in [-180,180]",
            ));
        }
        let time_ms = self.fw.vdev.next_wall_ms();
        let elapsed_ns = self.fw.vdev.next_timestamp_ns();
        let prev = self.fw.vdev.location.current.clone();
        let fix = LocationFix {
            provider: "gps".into(),
            latitude,
            longitude,
            altitude: altitude.unwrap_or(prev.altitude),
            accuracy: accuracy.unwrap_or(prev.accuracy).max(0.1),
            speed: speed.unwrap_or(0.0).max(0.0),
            bearing: bearing.unwrap_or(0.0),
            time_ms,
            elapsed_ns,
        };
        self.fw.vdev.location.current = fix.clone();
        self.m10_deliver_location_fix(&fix)
    }

    /// Load a deterministic GPX/waypoint sequence and advance to its first fix (if non-empty).
    pub fn m10_set_location_route(
        &mut self,
        waypoints: Vec<(f64, f64, f64, f32)>,
    ) -> Result<usize, VmExit> {
        let mut route = Vec::with_capacity(waypoints.len());
        for (lat, lon, alt, speed) in waypoints {
            if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
                return Err(illegal("invalid route waypoint coordinates"));
            }
            route.push(LocationFix {
                provider: "gps".into(),
                latitude: lat,
                longitude: lon,
                altitude: alt,
                accuracy: 5.0,
                speed,
                bearing: 0.0,
                time_ms: self.fw.vdev.wall_clock_ms,
                elapsed_ns: self.fw.vdev.monotonic_ns,
            });
        }
        self.fw.vdev.location.route = route;
        self.fw.vdev.location.route_cursor = 0;
        self.m10_step_location_route()
    }

    /// Advance one waypoint along the configured route and deliver callbacks.
    pub fn m10_step_location_route(&mut self) -> Result<usize, VmExit> {
        let idx = self.fw.vdev.location.route_cursor;
        let Some(wp) = self.fw.vdev.location.route.get(idx).cloned() else {
            return Ok(0);
        };
        self.fw.vdev.location.route_cursor = idx + 1;
        self.m10_set_location(
            wp.latitude,
            wp.longitude,
            Some(wp.altitude),
            Some(wp.accuracy),
            Some(wp.speed),
            Some(wp.bearing),
        )
    }

    fn m10_deliver_location_fix(&mut self, fix: &LocationFix) -> Result<usize, VmExit> {
        let regs = self.fw.vdev.location.listeners.clone();
        if regs.is_empty() {
            return Ok(0);
        }
        self.fw.vdev.location.listeners.retain(|r| !r.single_shot);
        self.with_host_roots(|vm| {
            let mut count = 0usize;
            for reg in regs {
                let mut reg_fix = fix.clone();
                reg_fix.provider = reg.provider.clone();
                let loc_obj = build_location_obj(vm, &reg_fix)?;
                if reg.fused_callback {
                    let list = array_list(vm, vec![Value::Obj(loc_obj)])?;
                    let res = alloc_with(vm, LOCATION_RESULT, vec![("rd_locations".into(), list)])?;
                    if call_dex(
                        vm,
                        reg.listener,
                        "onLocationResult",
                        "(Lcom/google/android/gms/location/LocationResult;)V",
                        vec![Value::Obj(res)],
                    )?
                    .is_some()
                    {
                        count += 1;
                    }
                } else if call_dex(
                    vm,
                    reg.listener,
                    "onLocationChanged",
                    "(Landroid/location/Location;)V",
                    vec![Value::Obj(loc_obj)],
                )?
                .is_some()
                {
                    count += 1;
                }
            }
            Ok(count)
        })
    }
}
