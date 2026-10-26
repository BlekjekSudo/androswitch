//! M10 virtual devices, sensors, GPS, camera2, biometrics, audio/media,
//! telephony/BLE/NFC, battery/power, and dynamic system configuration.
//!
//! Deterministic per-`Engine` virtual peripherals (`ADR 011`). Zero host OS
//! hardware access, zero thread-local storage, and GC-friendly heap payloads for
//! transient value objects.

pub mod biometrics;
pub mod camera;
pub mod connectivity;
pub mod location;
pub mod media;
pub mod sensors;
pub mod sysconfig;

use crate::framework::HostObj;
use crate::heap::ElemKind;
use crate::{Engine, ObjRef, Throwable, Value, VmExit};
use std::collections::{BTreeMap, BTreeSet};

pub use biometrics::{ActiveBiometricPrompt, BiometricOutcome, BiometricState};
pub use camera::{CameraFrameSpec, CameraSessionState, ImageReaderState, VirtualCamera};
pub use connectivity::{BleAdv, InboundSms, NetworkState, NfcTagSpec, OutboundSms, TelephonyState};
pub use location::{LocationFix, LocationListenerReg, LocationState};
pub use media::{
    AudioFocusEntry, AudioRecordState, AudioState, AudioTrackState, MediaPlayerEntry,
    MediaPlayerState, MediaProjectionEntry, MediaSessionEntry,
};
pub use sensors::{SensorListenerReg, VirtualSensor};
pub use sysconfig::{BatteryState, PowerState, SystemConfigState, WakeLockEntry};

#[derive(Clone)]
pub struct State {
    pub virtual_hardware_enabled: bool,
    pub monotonic_ns: i64,
    pub wall_clock_ms: i64,
    pub sensors: BTreeMap<i32, VirtualSensor>,
    pub sensor_listeners: Vec<SensorListenerReg>,
    pub location: LocationState,
    pub cameras: BTreeMap<String, VirtualCamera>,
    pub camera_devices: BTreeMap<ObjRef, String>,
    pub camera_callbacks: BTreeMap<ObjRef, ObjRef>,
    pub camera_sessions: BTreeMap<ObjRef, CameraSessionState>,
    pub image_readers: BTreeMap<ObjRef, ImageReaderState>,
    pub surface_to_reader: BTreeMap<ObjRef, ObjRef>,
    pub injected_camera_frames: BTreeMap<String, CameraFrameSpec>,
    pub biometrics: BiometricState,
    pub audio: AudioState,
    pub audio_records: BTreeMap<ObjRef, AudioRecordState>,
    pub audio_tracks: BTreeMap<ObjRef, AudioTrackState>,
    pub media_players: BTreeMap<ObjRef, MediaPlayerEntry>,
    pub media_sessions: BTreeMap<ObjRef, MediaSessionEntry>,
    pub media_projections: BTreeMap<ObjRef, MediaProjectionEntry>,
    pub telephony: TelephonyState,
    pub network: NetworkState,
    pub network_callbacks: Vec<ObjRef>,
    pub bluetooth_enabled: bool,
    pub bluetooth_name: String,
    pub bluetooth_address: String,
    pub ble_scan_callbacks: Vec<ObjRef>,
    pub ble_advertisements: Vec<BleAdv>,
    pub nfc_enabled: bool,
    pub nfc_reader_callback: Option<ObjRef>,
    pub nfc_last_tag: Option<NfcTagSpec>,
    pub battery: BatteryState,
    pub power: PowerState,
    pub wake_locks: BTreeMap<ObjRef, WakeLockEntry>,
    pub sysconfig: SystemConfigState,
    pub service_names_extra: BTreeSet<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            virtual_hardware_enabled: false,
            monotonic_ns: 1_000_000_000,
            wall_clock_ms: 1_792_886_400_000, // 2026-10-25T00:00:00Z deterministic base
            sensors: sensors::default_sensors(),
            sensor_listeners: Vec::new(),
            location: LocationState::default(),
            cameras: camera::default_cameras(),
            camera_devices: BTreeMap::new(),
            camera_callbacks: BTreeMap::new(),
            camera_sessions: BTreeMap::new(),
            image_readers: BTreeMap::new(),
            surface_to_reader: BTreeMap::new(),
            injected_camera_frames: BTreeMap::new(),
            biometrics: BiometricState::default(),
            audio: AudioState::default(),
            audio_records: BTreeMap::new(),
            audio_tracks: BTreeMap::new(),
            media_players: BTreeMap::new(),
            media_sessions: BTreeMap::new(),
            media_projections: BTreeMap::new(),
            telephony: TelephonyState::default(),
            network: NetworkState::default(),
            network_callbacks: Vec::new(),
            bluetooth_enabled: true,
            bluetooth_name: "Moto E5 Virtual".into(),
            bluetooth_address: "02:00:00:00:5A:10".into(),
            ble_scan_callbacks: Vec::new(),
            ble_advertisements: Vec::new(),
            nfc_enabled: true,
            nfc_reader_callback: None,
            nfc_last_tag: None,
            battery: BatteryState::default(),
            power: PowerState::default(),
            wake_locks: BTreeMap::new(),
            sysconfig: SystemConfigState::default(),
            service_names_extra: BTreeSet::new(),
        }
    }
}

impl State {
    pub(crate) fn next_timestamp_ns(&mut self) -> i64 {
        self.monotonic_ns = self.monotonic_ns.saturating_add(16_666_667);
        self.monotonic_ns
    }

    pub(crate) fn next_wall_ms(&mut self) -> i64 {
        self.wall_clock_ms = self.wall_clock_ms.saturating_add(100);
        self.wall_clock_ms
    }

    fn collect_gc_roots(&self) -> Vec<ObjRef> {
        let mut r = Vec::new();
        for reg in &self.sensor_listeners {
            r.push(reg.listener);
        }
        for reg in &self.location.listeners {
            r.push(reg.listener);
        }
        r.extend(self.camera_devices.keys().copied());
        for (&k, &v) in &self.camera_callbacks {
            r.push(k);
            r.push(v);
        }
        for (&k, sess) in &self.camera_sessions {
            r.push(k);
            r.push(sess.device);
            r.extend(sess.surfaces.iter().copied());
        }
        for (&k, ir) in &self.image_readers {
            r.push(k);
            r.push(ir.surface);
            if let Some(l) = ir.listener {
                r.push(l);
            }
            r.extend(ir.queue.iter().copied());
        }
        for (&k, &v) in &self.surface_to_reader {
            r.push(k);
            r.push(v);
        }
        if let Some(p) = &self.biometrics.active_prompt {
            r.push(p.callback);
            if let Some(c) = p.crypto {
                r.push(c);
            }
        }
        for f in &self.audio.focus_stack {
            if let Some(l) = f.listener {
                r.push(l);
            }
        }
        r.extend(self.audio_records.keys().copied());
        r.extend(self.audio_tracks.keys().copied());
        for (&k, mp) in &self.media_players {
            r.push(k);
            if let Some(l) = mp.on_prepared {
                r.push(l);
            }
            if let Some(l) = mp.on_completion {
                r.push(l);
            }
            if let Some(l) = mp.on_seek_complete {
                r.push(l);
            }
            if let Some(l) = mp.on_error {
                r.push(l);
            }
        }
        for (&k, ms) in &self.media_sessions {
            r.push(k);
            if let Some(cb) = ms.callback {
                r.push(cb);
            }
            if let Some(ps) = ms.playback_state {
                r.push(ps);
            }
            if let Some(md) = ms.metadata {
                r.push(md);
            }
        }
        for (&k, pr) in &self.media_projections {
            r.push(k);
            r.extend(pr.callbacks.iter().copied());
            r.extend(pr.displays.iter().copied());
        }
        r.extend(self.network_callbacks.iter().copied());
        r.extend(self.ble_scan_callbacks.iter().copied());
        if let Some(cb) = self.nfc_reader_callback {
            r.push(cb);
        }
        r.extend(self.wake_locks.keys().copied());
        r.extend(self.sysconfig.clipboard_listeners.iter().copied());
        r
    }
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `gc.rs` scans `Debug` output of `Engine.fw` conservatively for `ObjRef`
        // integers; emit only genuine `ObjRef` roots so scalar config values
        // (sensor types, screen dimensions, battery level) never pin heap slots.
        let roots = self.collect_gc_roots();
        f.debug_struct("VirtualDeviceState")
            .field("roots", &roots)
            .finish()
    }
}

impl Engine {
    /// Enables M10 virtual peripherals (`ADR 011`) ahead of Appendix C's
    /// absent-hardware / no-service default profile in `intrinsics.rs`.
    pub fn m10_enable_virtual_devices(&mut self) {
        self.fw.vdev.virtual_hardware_enabled = true;
    }
}

pub(super) fn error(class: &str, msg: impl Into<String>) -> VmExit {
    Throwable::new(class, msg).into()
}

pub(super) fn illegal(msg: impl Into<String>) -> VmExit {
    error("Ljava/lang/IllegalArgumentException;", msg)
}

pub(super) fn illegal_state(msg: impl Into<String>) -> VmExit {
    error("Ljava/lang/IllegalStateException;", msg)
}

pub(super) fn security(msg: impl Into<String>) -> VmExit {
    error("Ljava/lang/SecurityException;", msg)
}

pub(super) fn npe(msg: impl Into<String>) -> VmExit {
    error("Ljava/lang/NullPointerException;", msg)
}

pub(super) fn check_perm(vm: &Engine, perm: &str) -> Result<(), VmExit> {
    if vm.permission_granted(perm) {
        Ok(())
    } else {
        Err(security(format!("Permission denied: {perm}")))
    }
}

pub(super) fn check_any_perm(vm: &Engine, perms: &[&str]) -> Result<(), VmExit> {
    if perms.iter().any(|p| vm.permission_granted(p)) {
        Ok(())
    } else {
        Err(security(format!(
            "Permission denied: requires one of {}",
            perms.join(", ")
        )))
    }
}

pub(super) fn alloc(vm: &mut Engine, class: &str) -> Result<ObjRef, VmExit> {
    Ok(vm.heap.alloc_instance(class.into(), Vec::new())?)
}

pub(super) fn alloc_with(
    vm: &mut Engine,
    class: &str,
    fields: Vec<(String, Value)>,
) -> Result<ObjRef, VmExit> {
    Ok(vm.heap.alloc_instance(class.into(), fields)?)
}

pub(super) fn str_val(vm: &mut Engine, s: impl Into<String>) -> Result<Value, VmExit> {
    Ok(Value::Obj(vm.heap.alloc_string(s.into())?))
}

pub(super) fn put_field(vm: &mut Engine, r: ObjRef, name: &str, val: Value) -> Result<(), VmExit> {
    crate::err::put_field_result(vm.heap.put_field(r, name, val))?;
    Ok(())
}

pub(super) fn get_field(vm: &Engine, r: ObjRef, name: &str) -> Option<Value> {
    vm.heap.get_field(r, name).ok()
}

pub(super) fn get_int_field(vm: &Engine, r: ObjRef, name: &str, default: i32) -> i32 {
    match get_field(vm, r, name) {
        Some(Value::Int(v)) => v,
        _ => default,
    }
}

pub(super) fn get_long_field(vm: &Engine, r: ObjRef, name: &str, default: i64) -> i64 {
    match get_field(vm, r, name) {
        Some(Value::Long(v)) => v,
        Some(Value::Int(v)) => i64::from(v),
        _ => default,
    }
}

pub(super) fn get_float_field(vm: &Engine, r: ObjRef, name: &str, default: f32) -> f32 {
    match get_field(vm, r, name) {
        Some(Value::Float(v)) => v,
        _ => default,
    }
}

pub(super) fn get_double_field(vm: &Engine, r: ObjRef, name: &str, default: f64) -> f64 {
    match get_field(vm, r, name) {
        Some(Value::Double(v)) => v,
        Some(Value::Float(v)) => f64::from(v),
        _ => default,
    }
}

pub(super) fn get_str_field(vm: &Engine, r: ObjRef, name: &str) -> Option<String> {
    match get_field(vm, r, name) {
        Some(Value::Obj(s)) => vm.heap.as_str(s).ok().map(str::to_string),
        _ => None,
    }
}

pub(super) fn obj_arg(args: &[Value], i: usize) -> Option<ObjRef> {
    match args.get(i) {
        Some(Value::Obj(r)) => Some(*r),
        _ => None,
    }
}

pub(super) fn int_arg(args: &[Value], i: usize) -> i32 {
    args.get(i).and_then(|v| v.as_int().ok()).unwrap_or(0)
}

pub(super) fn bool_arg(args: &[Value], i: usize) -> bool {
    int_arg(args, i) != 0
}

pub(super) fn long_arg(args: &[Value], i: usize) -> i64 {
    match args.get(i) {
        Some(Value::Long(v)) => *v,
        Some(Value::Int(v)) => i64::from(*v),
        _ => 0,
    }
}

pub(super) fn float_arg(args: &[Value], i: usize) -> f32 {
    match args.get(i) {
        Some(Value::Float(v)) => *v,
        Some(Value::Int(v)) => *v as f32,
        _ => 0.0,
    }
}

pub(super) fn double_arg(args: &[Value], i: usize) -> f64 {
    match args.get(i) {
        Some(Value::Double(v)) => *v,
        Some(Value::Float(v)) => f64::from(*v),
        Some(Value::Int(v)) => f64::from(*v),
        _ => 0.0,
    }
}

pub(super) fn opt_str_arg(vm: &Engine, args: &[Value], i: usize) -> Result<Option<String>, VmExit> {
    match args.get(i) {
        None | Some(Value::Null) | Some(Value::Int(0)) => Ok(None),
        Some(Value::Obj(r)) => Ok(Some(vm.heap.as_str(*r)?.to_string())),
        _ => Ok(None),
    }
}

pub(super) fn req_str_arg(vm: &Engine, args: &[Value], i: usize) -> Result<String, VmExit> {
    opt_str_arg(vm, args, i)?.ok_or_else(|| npe("expected non-null String"))
}

pub(super) fn array_list(vm: &mut Engine, elems: Vec<Value>) -> Result<Value, VmExit> {
    let r = alloc(vm, "Ljava/util/ArrayList;")?;
    vm.fw.objects.insert(r, HostObj::List { elems });
    Ok(Value::Obj(r))
}

pub(super) fn obj_array(vm: &mut Engine, class: &str, elems: Vec<Value>) -> Result<Value, VmExit> {
    let r = vm.heap.alloc_array(ElemKind::Obj, elems.len(), class)?;
    *vm.heap.array_elems_mut(r)? = elems;
    Ok(Value::Obj(r))
}

pub(super) fn array_len(vm: &Engine, arr: ObjRef) -> Result<usize, VmExit> {
    Ok(vm.heap.as_array_elems(arr)?.0.len())
}

pub(super) fn byte_array(vm: &mut Engine, bytes: &[u8]) -> Result<ObjRef, VmExit> {
    let r = vm.heap.alloc_array(ElemKind::Int, bytes.len(), "[B")?;
    let elems: Vec<Value> = bytes
        .iter()
        .map(|b| Value::Int(i32::from(*b as i8)))
        .collect();
    *vm.heap.array_elems_mut(r)? = elems;
    Ok(r)
}

pub(super) fn read_byte_array(vm: &Engine, arr: ObjRef) -> Result<Vec<u8>, VmExit> {
    let (elems, _) = vm.heap.as_array_elems(arr)?;
    let mut out = Vec::with_capacity(elems.len());
    for v in elems {
        let b = v.as_int().unwrap_or(0) as i8 as u8;
        out.push(b);
    }
    Ok(out)
}

pub(super) fn float_array(vm: &mut Engine, vals: &[f32]) -> Result<ObjRef, VmExit> {
    let r = vm.heap.alloc_array(ElemKind::Float, vals.len(), "[F")?;
    let elems: Vec<Value> = vals.iter().map(|f| Value::Float(*f)).collect();
    *vm.heap.array_elems_mut(r)? = elems;
    Ok(r)
}

pub(super) fn call_dex(
    vm: &mut Engine,
    recv: ObjRef,
    name: &str,
    sig: &str,
    args: Vec<Value>,
) -> Result<Option<Value>, VmExit> {
    let c = vm.heap.class_of(recv)?.to_string();
    let Some((d, def, m)) = vm.cp.resolve_method(&c, name, sig) else {
        return Ok(None);
    };
    let mut ins = Vec::with_capacity(1 + args.len() * 2);
    ins.push(Value::Obj(recv));
    for a in args {
        let wide = matches!(a, Value::Long(_) | Value::Double(_));
        ins.push(a);
        if wide {
            ins.push(Value::WideHi);
        }
    }
    vm.call(d, def, &m, ins).map(Some)
}

pub(super) fn system_service_singleton(vm: &mut Engine, cls: &str) -> Result<ObjRef, VmExit> {
    let k = ("Lrd/SystemServices;".to_string(), cls.to_string());
    if let Some(Value::Obj(r)) = vm.statics.get(&k) {
        return Ok(*r);
    }
    let r = alloc(vm, cls)?;
    vm.statics.insert(k, Value::Obj(r));
    Ok(r)
}

const EXTRA_SERVICES: &[(&str, &str)] = &[
    ("bluetooth", "Landroid/bluetooth/BluetoothManager;"),
    (
        "biometric",
        "Landroid/hardware/biometrics/BiometricManager;",
    ),
    (
        "fingerprint",
        "Landroid/hardware/fingerprint/FingerprintManager;",
    ),
    (
        "media_projection",
        "Landroid/media/projection/MediaProjectionManager;",
    ),
    ("nfc", "Landroid/nfc/NfcManager;"),
];

pub fn host_static(vm: &mut Engine, owner: &str, name: &str) -> Result<Option<Value>, VmExit> {
    vm.with_host_roots(|vm| host_static_rooted(vm, owner, name))
}

fn host_static_rooted(vm: &mut Engine, owner: &str, name: &str) -> Result<Option<Value>, VmExit> {
    if owner == "Landroid/content/Context;" {
        let svc = match name {
            "BLUETOOTH_SERVICE" => Some("bluetooth"),
            "BIOMETRIC_SERVICE" => Some("biometric"),
            "FINGERPRINT_SERVICE" => Some("fingerprint"),
            "MEDIA_PROJECTION_SERVICE" => Some("media_projection"),
            "NFC_SERVICE" => Some("nfc"),
            "SENSOR_SERVICE" => Some("sensor"),
            "LOCATION_SERVICE" => Some("location"),
            "CAMERA_SERVICE" => Some("camera"),
            "AUDIO_SERVICE" => Some("audio"),
            "TELEPHONY_SERVICE" => Some("telephony"),
            "WIFI_SERVICE" => Some("wifi"),
            "BATTERY_SERVICE" => Some("batterymanager"),
            "UI_MODE_SERVICE" => Some("uimode"),
            "POWER_SERVICE" => Some("power"),
            "CLIPBOARD_SERVICE" => Some("clipboard"),
            "MEDIA_SESSION_SERVICE" => Some("media_session"),
            "LOCALE_SERVICE" => Some("locale"),
            "CONNECTIVITY_SERVICE" => Some("connectivity"),
            _ => None,
        };
        if let Some(s) = svc {
            return str_val(vm, s).map(Some);
        }
    }
    if let Some(v) = sensors::host_static(vm, owner, name)? {
        return Ok(Some(v));
    }
    if let Some(v) = location::host_static(vm, owner, name)? {
        return Ok(Some(v));
    }
    if let Some(v) = camera::host_static(vm, owner, name)? {
        return Ok(Some(v));
    }
    if let Some(v) = biometrics::host_static(vm, owner, name)? {
        return Ok(Some(v));
    }
    if let Some(v) = media::host_static(vm, owner, name)? {
        return Ok(Some(v));
    }
    if let Some(v) = connectivity::host_static(vm, owner, name)? {
        return Ok(Some(v));
    }
    sysconfig::host_static(vm, owner, name)
}

pub fn call_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    vm.with_host_roots(|vm| call_static_rooted(vm, class, name, sig, args))
}

fn call_static_rooted(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if let Some(v) = sensors::static_call(vm, class, name, sig, args)? {
        return Ok(Some(v));
    }
    if let Some(v) = location::static_call(vm, class, name, sig, args)? {
        return Ok(Some(v));
    }
    if let Some(v) = camera::static_call(vm, class, name, sig, args)? {
        return Ok(Some(v));
    }
    if let Some(v) = biometrics::static_call(vm, class, name, sig, args)? {
        return Ok(Some(v));
    }
    if let Some(v) = media::static_call(vm, class, name, sig, args)? {
        return Ok(Some(v));
    }
    if let Some(v) = connectivity::static_call(vm, class, name, sig, args)? {
        return Ok(Some(v));
    }
    sysconfig::static_call(vm, class, name, sig, args)
}

pub fn call_instance(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    vm.with_host_roots(|vm| call_instance_rooted(vm, class, name, sig, recv, args))
}

fn is_context_class(class: &str) -> bool {
    matches!(
        class,
        "Landroid/content/Context;"
            | "Landroid/app/Activity;"
            | "Landroid/app/Application;"
            | "Landroid/content/ContextWrapper;"
            | "Landroid/view/ContextThemeWrapper;"
            | "Landroid/app/Service;"
    )
}

fn call_instance_rooted(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if is_context_class(class) {
        if name == "getSystemServiceName" && sig == "(Ljava/lang/Class;)Ljava/lang/String;" {
            let cls = obj_arg(args, 0)
                .and_then(|c| get_str_field(vm, c, "class_name"))
                .unwrap_or_default();
            if let Some((svc_name, _)) = EXTRA_SERVICES.iter().find(|(_, c)| *c == cls) {
                return str_val(vm, *svc_name).map(Some);
            }
        }
        if name == "getSystemService" {
            let key = if sig.contains("Ljava/lang/Class;") {
                obj_arg(args, 0)
                    .and_then(|c| get_str_field(vm, c, "class_name"))
                    .unwrap_or_default()
            } else {
                opt_str_arg(vm, args, 0)?.unwrap_or_default()
            };
            if let Some((_, cls)) = EXTRA_SERVICES.iter().find(|(n, c)| *n == key || *c == key) {
                let r = system_service_singleton(vm, cls)?;
                return Ok(Some(Value::Obj(r)));
            }
        }
        if name == "registerReceiver" {
            if let Some(v) = sysconfig::maybe_sticky_battery_receiver(vm, args)? {
                return Ok(Some(v));
            }
        }
    }

    if let Some(v) = sensors::instance_call(vm, class, name, sig, recv, args)? {
        return Ok(Some(v));
    }
    if let Some(v) = location::instance_call(vm, class, name, sig, recv, args)? {
        return Ok(Some(v));
    }
    if let Some(v) = camera::instance_call(vm, class, name, sig, recv, args)? {
        return Ok(Some(v));
    }
    if let Some(v) = biometrics::instance_call(vm, class, name, sig, recv, args)? {
        return Ok(Some(v));
    }
    if let Some(v) = media::instance_call(vm, class, name, sig, recv, args)? {
        return Ok(Some(v));
    }
    if let Some(v) = connectivity::instance_call(vm, class, name, sig, recv, args)? {
        return Ok(Some(v));
    }
    sysconfig::instance_call(vm, class, name, sig, recv, args)
}
