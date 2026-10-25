#![cfg(feature = "virtual-devices")]

use rd_testsupport::apkfix::*;
use rd_testsupport::vm::*;
use rd_vm::m10::{self, BiometricOutcome};
use rd_vm::{Engine, ObjRef, Value, VmConfig, VmExit};

const PKG: &str = "test.m10";
const MAIN: &str = "Ltest/m10/Main;";
const SENSOR_CB: &str = "Ltest/m10/SensorCb;";
const LOC_CB: &str = "Ltest/m10/LocCb;";
const FUSED_CB: &str = "Ltest/m10/FusedCb;";
const CAM_DEV_CB: &str = "Ltest/m10/CamDevCb;";
const CAM_SESS_CB: &str = "Ltest/m10/CamSessCb;";
const CAM_CAP_CB: &str = "Ltest/m10/CamCapCb;";
const IMG_CB: &str = "Ltest/m10/ImgCb;";
const BIO_FW_CB: &str = "Ltest/m10/BioFwCb;";
const BIO_AX_CB: &str = "Ltest/m10/BioAxCb;";
const AUDIO_CB: &str = "Ltest/m10/AudioCb;";
const MP_CB: &str = "Ltest/m10/MpCb;";
const MS_CB: &str = "Ltest/m10/MsCb;";
const PROJ_CB: &str = "Ltest/m10/ProjCb;";
const NET_CB: &str = "Ltest/m10/NetCb;";
const BLE_CB: &str = "Ltest/m10/BleCb;";
const NFC_CB: &str = "Ltest/m10/NfcCb;";
const BCAST_CB: &str = "Ltest/m10/BcastCb;";
const CLIP_CB: &str = "Ltest/m10/ClipCb;";

fn ins_words(params: &[&str]) -> u16 {
    let mut n = 1u16;
    for p in params {
        n += if *p == "J" || *p == "D" { 2 } else { 1 };
    }
    n
}

fn add_counter_methods(b: &mut DexBuilder, cls: &str, base: &str, methods: &[(&str, &[&str])]) {
    let c = b.class(cls, base);
    // Default constructor `<init>()V` returning void so DEX instances instantiate cleanly
    let ctor_code = b.code(1, 1, 0, op10x(0x0e));
    b.direct(c, "<init>", "V", vec![], ACC_PUBLIC, Some(ctor_code));

    let mut sorted = methods.to_vec();
    sorted.sort_by_key(|(n, _)| *n);
    for (name, params) in sorted {
        b.static_field(c, name, "I", SVal::Int(0));
        let field = b.field_idx(cls, "I", name);
        let mut units = op21c(0x60, 0, field);
        units.extend(op22b(0xd8, 0, 0, 1));
        units.extend(op21c(0x67, 0, field));
        units.extend(op10x(0x0e));
        let ins = ins_words(params);
        let code = b.code(ins + 1, ins, 0, units);
        b.r#virtual(c, name, "V", params.to_vec(), ACC_PUBLIC, Some(code));
    }
}

fn m10_apk() -> Vec<u8> {
    let mut arsc = ArscFix::new();
    let layout = arsc.add_layout("main");
    let id_title = arsc.add_id("title");
    let xml = AxNode::new(
        "LinearLayout",
        vec![a(true, "orientation", AxVal::Int(1))],
        vec![AxNode::new(
            "TextView",
            vec![
                a(true, "id", AxVal::Ref(id_title)),
                a(true, "text", AxVal::Str("M10 Ready".into())),
            ],
            vec![],
        )],
    );

    let mut b = DexBuilder::new();
    let setp = b.proto_idx("V", vec!["I".into()]);
    let set = b.method_idx("Landroid/app/Activity;", setp, "setContentView");
    let main = b.class(MAIN, "Landroid/app/Activity;");
    let mut on_create = vec![0x14u16, layout as u16, (layout >> 16) as u16];
    on_create.extend(op35c(0x6e, 2, set, [1, 0, 0, 0, 0]));
    on_create.extend(op10x(0x0e));
    let on_create_code = b.code(3, 2, 2, on_create);
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(on_create_code),
    );
    for (mname, ptype) in [
        (
            "onConfigurationChanged",
            "Landroid/content/res/Configuration;",
        ),
        ("onNewIntent", "Landroid/content/Intent;"),
    ] {
        b.static_field(main, mname, "I", SVal::Int(0));
        let fld = b.field_idx(MAIN, "I", mname);
        let mut u = op21c(0x60, 0, fld);
        u.extend(op22b(0xd8, 0, 0, 1));
        u.extend(op21c(0x67, 0, fld));
        u.extend(op10x(0x0e));
        let code = b.code(3, 2, 0, u);
        b.r#virtual(main, mname, "V", vec![ptype], ACC_PUBLIC, Some(code));
    }

    // SensorCb also reads `event.values[0]` in Dalvik bytecode (`iget-object` + `aget` + `sput`)!
    {
        let c = b.class(SENSOR_CB, "Ljava/lang/Object;");
        let ctor = b.code(1, 1, 0, op10x(0x0e));
        b.direct(c, "<init>", "V", vec![], ACC_PUBLIC, Some(ctor));
        b.static_field(c, "lastX", "F", SVal::Int(0));
        let f_x = b.field_idx(SENSOR_CB, "F", "lastX");
        b.static_field(c, "onAccuracyChanged", "I", SVal::Int(0));
        let f_acc = b.field_idx(SENSOR_CB, "I", "onAccuracyChanged");
        b.static_field(c, "onSensorChanged", "I", SVal::Int(0));
        let f_cnt = b.field_idx(SENSOR_CB, "I", "onSensorChanged");
        let f_vals = b.field_idx("Landroid/hardware/SensorEvent;", "[F", "values");

        let mut ua = op21c(0x60, 0, f_acc);
        ua.extend(op22b(0xd8, 0, 0, 1));
        ua.extend(op21c(0x67, 0, f_acc));
        ua.extend(op10x(0x0e));
        let ac_code = b.code(4, 3, 0, ua);
        b.r#virtual(
            c,
            "onAccuracyChanged",
            "V",
            vec!["Landroid/hardware/Sensor;", "I"],
            ACC_PUBLIC,
            Some(ac_code),
        );

        // regs: v0, v1, v2=this, v3=SensorEvent
        let mut u = op21c(0x60, 0, f_cnt); // sget v0, onSensorChanged
        u.extend(op22b(0xd8, 0, 0, 1)); // add-int/lit8 v0, v0, 1
        u.extend(op21c(0x67, 0, f_cnt)); // sput v0, onSensorChanged
        u.extend(op22c(0x54, 0, 3, f_vals)); // iget-object v0, v3, SensorEvent->values:[F
        u.extend(op11n(0x12, 1, 0)); // const/4 v1, 0
        u.extend(op23x(0x44, 0, 0, 1)); // aget v0, v0, v1
        u.extend(op21c(0x67, 0, f_x)); // sput v0, lastX
        u.extend(op10x(0x0e)); // return-void
        let sc_code = b.code(4, 2, 0, u);
        b.r#virtual(
            c,
            "onSensorChanged",
            "V",
            vec!["Landroid/hardware/SensorEvent;"],
            ACC_PUBLIC,
            Some(sc_code),
        );
    }

    add_counter_methods(
        &mut b,
        LOC_CB,
        "Ljava/lang/Object;",
        &[("onLocationChanged", &["Landroid/location/Location;"])],
    );
    add_counter_methods(
        &mut b,
        FUSED_CB,
        "Lcom/google/android/gms/location/LocationCallback;",
        &[(
            "onLocationResult",
            &["Lcom/google/android/gms/location/LocationResult;"],
        )],
    );
    add_counter_methods(
        &mut b,
        CAM_DEV_CB,
        "Landroid/hardware/camera2/CameraDevice$StateCallback;",
        &[
            ("onOpened", &["Landroid/hardware/camera2/CameraDevice;"]),
            ("onClosed", &["Landroid/hardware/camera2/CameraDevice;"]),
        ],
    );
    add_counter_methods(
        &mut b,
        CAM_SESS_CB,
        "Landroid/hardware/camera2/CameraCaptureSession$StateCallback;",
        &[(
            "onConfigured",
            &["Landroid/hardware/camera2/CameraCaptureSession;"],
        )],
    );
    add_counter_methods(
        &mut b,
        CAM_CAP_CB,
        "Landroid/hardware/camera2/CameraCaptureSession$CaptureCallback;",
        &[(
            "onCaptureCompleted",
            &[
                "Landroid/hardware/camera2/CameraCaptureSession;",
                "Landroid/hardware/camera2/CaptureRequest;",
                "Landroid/hardware/camera2/TotalCaptureResult;",
            ],
        )],
    );
    add_counter_methods(
        &mut b,
        IMG_CB,
        "Ljava/lang/Object;",
        &[("onImageAvailable", &["Landroid/media/ImageReader;"])],
    );
    add_counter_methods(
        &mut b,
        BIO_FW_CB,
        "Landroid/hardware/biometrics/BiometricPrompt$AuthenticationCallback;",
        &[
            (
                "onAuthenticationSucceeded",
                &["Landroid/hardware/biometrics/BiometricPrompt$AuthenticationResult;"],
            ),
            ("onAuthenticationFailed", &[]),
            ("onAuthenticationError", &["I", "Ljava/lang/CharSequence;"]),
        ],
    );
    add_counter_methods(
        &mut b,
        BIO_AX_CB,
        "Landroidx/biometric/BiometricPrompt$AuthenticationCallback;",
        &[
            (
                "onAuthenticationSucceeded",
                &["Landroidx/biometric/BiometricPrompt$AuthenticationResult;"],
            ),
            ("onAuthenticationFailed", &[]),
            ("onAuthenticationError", &["I", "Ljava/lang/CharSequence;"]),
        ],
    );
    add_counter_methods(
        &mut b,
        AUDIO_CB,
        "Ljava/lang/Object;",
        &[("onAudioFocusChange", &["I"])],
    );
    add_counter_methods(
        &mut b,
        MP_CB,
        "Ljava/lang/Object;",
        &[
            ("onPrepared", &["Landroid/media/MediaPlayer;"]),
            ("onCompletion", &["Landroid/media/MediaPlayer;"]),
            ("onSeekComplete", &["Landroid/media/MediaPlayer;"]),
        ],
    );
    add_counter_methods(
        &mut b,
        MS_CB,
        "Landroid/media/session/MediaSession$Callback;",
        &[
            ("onPlay", &[]),
            ("onPause", &[]),
            ("onStop", &[]),
            ("onSkipToNext", &[]),
            ("onSkipToPrevious", &[]),
            ("onSeekTo", &["J"]),
        ],
    );
    add_counter_methods(
        &mut b,
        PROJ_CB,
        "Landroid/media/projection/MediaProjection$Callback;",
        &[("onStop", &[])],
    );
    add_counter_methods(
        &mut b,
        NET_CB,
        "Landroid/net/ConnectivityManager$NetworkCallback;",
        &[
            ("onAvailable", &["Landroid/net/Network;"]),
            ("onLost", &["Landroid/net/Network;"]),
            (
                "onCapabilitiesChanged",
                &["Landroid/net/Network;", "Landroid/net/NetworkCapabilities;"],
            ),
        ],
    );
    add_counter_methods(
        &mut b,
        BLE_CB,
        "Landroid/bluetooth/le/ScanCallback;",
        &[("onScanResult", &["I", "Landroid/bluetooth/le/ScanResult;"])],
    );
    add_counter_methods(
        &mut b,
        NFC_CB,
        "Ljava/lang/Object;",
        &[("onTagDiscovered", &["Landroid/nfc/Tag;"])],
    );
    add_counter_methods(
        &mut b,
        BCAST_CB,
        "Landroid/content/BroadcastReceiver;",
        &[(
            "onReceive",
            &["Landroid/content/Context;", "Landroid/content/Intent;"],
        )],
    );
    add_counter_methods(
        &mut b,
        CLIP_CB,
        "Ljava/lang/Object;",
        &[("onPrimaryClipChanged", &[])],
    );

    let manifest = AxNode::new(
        "manifest",
        vec![a(false, "package", AxVal::Str(PKG.into()))],
        vec![
            AxNode::new(
                "uses-sdk",
                vec![
                    a(true, "minSdkVersion", AxVal::Int(26)),
                    a(true, "targetSdkVersion", AxVal::Int(34)),
                ],
                vec![],
            ),
            AxNode::new(
                "application",
                vec![],
                vec![AxNode::new(
                    "activity",
                    vec![
                        a(true, "name", AxVal::Str("test.m10.Main".into())),
                        a(true, "exported", AxVal::Bool(true)),
                    ],
                    vec![AxNode::new(
                        "intent-filter",
                        vec![],
                        vec![
                            AxNode::new(
                                "action",
                                vec![a(
                                    true,
                                    "name",
                                    AxVal::Str("android.intent.action.MAIN".into()),
                                )],
                                vec![],
                            ),
                            AxNode::new(
                                "category",
                                vec![a(
                                    true,
                                    "name",
                                    AxVal::Str("android.intent.category.LAUNCHER".into()),
                                )],
                                vec![],
                            ),
                        ],
                    )],
                )],
            ),
        ],
    );

    let mut z = ZipFix::new();
    z.add("AndroidManifest.xml", build_axml(&manifest));
    z.add("resources.arsc", arsc.build());
    z.add("classes.dex", b.finish());
    z.add("res/layout/main.xml", build_axml(&xml));
    z.build()
}

fn engine() -> Engine {
    let apk = rd_apk::Apk::from_bytes(m10_apk()).expect("parse m10 apk");
    let dex = rd_dex::Dex::parse(apk.zip.read("classes.dex").expect("classes.dex"))
        .expect("parse classes.dex");
    let mut e = Engine::new(vec![dex], VmConfig::default());
    e.set_resources(apk);
    e.launch_app(MAIN, PKG, vec![]).expect("launch Main");
    e
}

fn dex_obj(e: &mut Engine, cls: &str) -> ObjRef {
    e.ensure_initialized(cls).expect("init cls");
    e.new_dex_instance_pub(cls).expect("alloc dex cls")
}

fn host_obj(e: &mut Engine, cls: &str) -> ObjRef {
    let r = e
        .heap
        .alloc_instance(cls.into(), Vec::new())
        .expect("alloc");
    e.attach_host_state(cls, r).expect("attach");
    r
}

fn s(e: &mut Engine, text: &str) -> Value {
    Value::Obj(e.heap.alloc_string(text.into()).expect("str"))
}

fn count(e: &Engine, cls: &str, name: &str) -> i32 {
    match e.statics.get(&(cls.into(), name.into())) {
        Some(Value::Int(v)) => *v,
        _ => 0,
    }
}

fn icall(e: &mut Engine, cls: &str, name: &str, sig: &str, recv: ObjRef, args: &[Value]) -> Value {
    m10::call_instance(e, cls, name, sig, recv, args)
        .expect("m10 call ok")
        .unwrap_or_else(|| panic!("unhandled instance {cls}->{name}{sig}"))
}

fn scall(e: &mut Engine, cls: &str, name: &str, sig: &str, args: &[Value]) -> Value {
    m10::call_static(e, cls, name, sig, args)
        .expect("m10 static ok")
        .unwrap_or_else(|| panic!("unhandled static {cls}->{name}{sig}"))
}

#[test]
fn virtual_sensors_deliver_dex_events_and_survive_gc() {
    let mut e = engine();
    let sm = host_obj(&mut e, "Landroid/hardware/SensorManager;");
    let Value::Obj(accel) = icall(
        &mut e,
        "Landroid/hardware/SensorManager;",
        "getDefaultSensor",
        "(I)Landroid/hardware/Sensor;",
        sm,
        &[Value::Int(1)],
    ) else {
        panic!("expected accelerometer Sensor");
    };
    assert_eq!(
        icall(
            &mut e,
            "Landroid/hardware/Sensor;",
            "getType",
            "()I",
            accel,
            &[]
        ),
        Value::Int(1)
    );
    let cb = dex_obj(&mut e, SENSOR_CB);
    assert_eq!(
        icall(
            &mut e,
            "Landroid/hardware/SensorManager;",
            "registerListener",
            "(Landroid/hardware/SensorEventListener;Landroid/hardware/Sensor;I)Z",
            sm,
            &[Value::Obj(cb), Value::Obj(accel), Value::Int(3)]
        ),
        Value::Int(1)
    );
    // Force major GC while `cb` is only held by M10 sensor_listeners!
    let _ = e.gc();
    let delivered = e
        .m10_inject_sensor(1, vec![3.25, 9.81, -1.5], Some(2))
        .expect("inject accel");
    assert_eq!(delivered, 1);
    assert_eq!(count(&e, SENSOR_CB, "onSensorChanged"), 1);
    assert_eq!(count(&e, SENSOR_CB, "onAccuracyChanged"), 1);
    match e.statics.get(&(SENSOR_CB.into(), "lastX".into())) {
        Some(Value::Float(x)) => assert!((*x - 3.25).abs() < 1e-4),
        other => panic!("expected Float(3.25) in SensorCb.lastX, got {other:?}"),
    }

    icall(
        &mut e,
        "Landroid/hardware/SensorManager;",
        "unregisterListener",
        "(Landroid/hardware/SensorEventListener;)V",
        sm,
        &[Value::Obj(cb)],
    );
    assert_eq!(
        e.m10_inject_sensor(1, vec![0.0, 0.0, 0.0], None)
            .expect("inject after unregister"),
        0
    );
}

#[test]
fn location_manager_fused_client_haversine_and_route_playback() {
    let mut e = engine();
    let lm = host_obj(&mut e, "Landroid/location/LocationManager;");
    let gps = s(&mut e, "gps");
    let cb = dex_obj(&mut e, LOC_CB);
    icall(
        &mut e,
        "Landroid/location/LocationManager;",
        "requestLocationUpdates",
        "(Ljava/lang/String;JFLandroid/location/LocationListener;)V",
        lm,
        &[
            gps.clone(),
            Value::Long(1000),
            Value::Float(1.0),
            Value::Obj(cb),
        ],
    );

    let act = e.fw.current_activity.expect("activity");
    let Value::Obj(fused) = scall(
        &mut e,
        "Lcom/google/android/gms/location/LocationServices;",
        "getFusedLocationProviderClient",
        "(Landroid/app/Activity;)Lcom/google/android/gms/location/FusedLocationProviderClient;",
        &[Value::Obj(act)],
    ) else {
        panic!("expected FusedLocationProviderClient");
    };
    let Value::Obj(req) = scall(
        &mut e,
        "Lcom/google/android/gms/location/LocationRequest;",
        "create",
        "()Lcom/google/android/gms/location/LocationRequest;",
        &[],
    ) else {
        panic!("expected LocationRequest");
    };
    let fcb = dex_obj(&mut e, FUSED_CB);
    icall(
        &mut e,
        "Lcom/google/android/gms/location/FusedLocationProviderClient;",
        "requestLocationUpdates",
        "(Lcom/google/android/gms/location/LocationRequest;Lcom/google/android/gms/location/LocationCallback;Landroid/os/Looper;)Lcom/google/android/gms/tasks/Task;",
        fused,
        &[Value::Obj(req), Value::Obj(fcb), Value::Null],
    );

    // Load 2-waypoint route (-18.2444, -43.6003 Diamantina -> -19.9167, -43.9345 BH)
    let d1 = e
        .m10_set_location_route(vec![
            (-18.2444, -43.6003, 1280.0, 4.0),
            (-19.9167, -43.9345, 852.0, 12.0),
        ])
        .expect("set route");
    assert_eq!(d1, 2);
    let d2 = e.m10_step_location_route().expect("step route");
    assert_eq!(d2, 2);
    assert_eq!(count(&e, LOC_CB, "onLocationChanged"), 2);
    assert_eq!(count(&e, FUSED_CB, "onLocationResult"), 2);

    let Value::Obj(last) = icall(
        &mut e,
        "Landroid/location/LocationManager;",
        "getLastKnownLocation",
        "(Ljava/lang/String;)Landroid/location/Location;",
        lm,
        &[gps],
    ) else {
        panic!("expected last known Location");
    };
    let Value::Double(lat) = icall(
        &mut e,
        "Landroid/location/Location;",
        "getLatitude",
        "()D",
        last,
        &[],
    ) else {
        panic!("expected Double lat");
    };
    assert!((lat - (-19.9167)).abs() < 1e-4);

    // Verify Haversine distanceBetween (~189 km between Diamantina and Belo Horizonte)
    let out_arr = e
        .heap
        .alloc_array(rd_vm::heap::ElemKind::Float, 3, "[F")
        .expect("float[3]");
    *e.heap.array_elems_mut(out_arr).expect("elems") =
        vec![Value::Float(0.0), Value::Float(0.0), Value::Float(0.0)];
    scall(
        &mut e,
        "Landroid/location/Location;",
        "distanceBetween",
        "(DDDD[F)V",
        &[
            Value::Double(-18.2444),
            Value::Double(-43.6003),
            Value::Double(-19.9167),
            Value::Double(-43.9345),
            Value::Obj(out_arr),
        ],
    );
    let meters = match e.heap.as_array_elems(out_arr).expect("read").0[0] {
        Value::Float(v) => v,
        _ => 0.0,
    };
    assert!(
        meters > 180_000.0 && meters < 200_000.0,
        "unexpected Haversine distance: {meters} m"
    );
}

#[test]
fn camera2_session_imagereader_and_bytebuffer_pipeline() {
    let mut e = engine();
    let cm = host_obj(&mut e, "Landroid/hardware/camera2/CameraManager;");
    let Value::Obj(ids) = icall(
        &mut e,
        "Landroid/hardware/camera2/CameraManager;",
        "getCameraIdList",
        "()[Ljava/lang/String;",
        cm,
        &[],
    ) else {
        panic!("expected String[] camera ids");
    };
    assert_eq!(e.heap.as_array_elems(ids).expect("ids").0.len(), 2);

    // Create ImageReader(64, 64, JPEG=256, 2) + listener
    let Value::Obj(reader) = scall(
        &mut e,
        "Landroid/media/ImageReader;",
        "newInstance",
        "(IIII)Landroid/media/ImageReader;",
        &[
            Value::Int(64),
            Value::Int(64),
            Value::Int(256),
            Value::Int(2),
        ],
    ) else {
        panic!("expected ImageReader");
    };
    let img_cb = dex_obj(&mut e, IMG_CB);
    icall(
        &mut e,
        "Landroid/media/ImageReader;",
        "setOnImageAvailableListener",
        "(Landroid/media/ImageReader$OnImageAvailableListener;Landroid/os/Handler;)V",
        reader,
        &[Value::Obj(img_cb), Value::Null],
    );
    let Value::Obj(surface) = icall(
        &mut e,
        "Landroid/media/ImageReader;",
        "getSurface",
        "()Landroid/view/Surface;",
        reader,
        &[],
    ) else {
        panic!("expected Surface");
    };

    // Open camera "0" -> onOpened
    let cam0 = s(&mut e, "0");
    let dev_cb = dex_obj(&mut e, CAM_DEV_CB);
    icall(
        &mut e,
        "Landroid/hardware/camera2/CameraManager;",
        "openCamera",
        "(Ljava/lang/String;Landroid/hardware/camera2/CameraDevice$StateCallback;Landroid/os/Handler;)V",
        cm,
        &[cam0, Value::Obj(dev_cb), Value::Null],
    );
    assert_eq!(count(&e, CAM_DEV_CB, "onOpened"), 1);
    let dev = *e.fw.vdev.camera_devices.keys().next().expect("open device");

    // Build CaptureRequest targeting surface + createCaptureSession -> onConfigured
    let Value::Obj(req_builder) = icall(
        &mut e,
        "Landroid/hardware/camera2/CameraDevice;",
        "createCaptureRequest",
        "(I)Landroid/hardware/camera2/CaptureRequest$Builder;",
        dev,
        &[Value::Int(1)],
    ) else {
        panic!("expected CaptureRequest$Builder");
    };
    icall(
        &mut e,
        "Landroid/hardware/camera2/CaptureRequest$Builder;",
        "addTarget",
        "(Landroid/view/Surface;)V",
        req_builder,
        &[Value::Obj(surface)],
    );
    let Value::Obj(cap_req) = icall(
        &mut e,
        "Landroid/hardware/camera2/CaptureRequest$Builder;",
        "build",
        "()Landroid/hardware/camera2/CaptureRequest;",
        req_builder,
        &[],
    ) else {
        panic!("expected CaptureRequest");
    };

    let surf_list = host_obj(&mut e, "Ljava/util/ArrayList;");
    e.fw.objects.insert(
        surf_list,
        rd_vm::framework::HostObj::List {
            elems: vec![Value::Obj(surface)],
        },
    );
    let sess_cb = dex_obj(&mut e, CAM_SESS_CB);
    icall(
        &mut e,
        "Landroid/hardware/camera2/CameraDevice;",
        "createCaptureSession",
        "(Ljava/util/List;Landroid/hardware/camera2/CameraCaptureSession$StateCallback;Landroid/os/Handler;)V",
        dev,
        &[Value::Obj(surf_list), Value::Obj(sess_cb), Value::Null],
    );
    assert_eq!(count(&e, CAM_SESS_CB, "onConfigured"), 1);
    let sess = *e.fw.vdev.camera_sessions.keys().next().expect("session");

    let cap_cb = dex_obj(&mut e, CAM_CAP_CB);
    icall(
        &mut e,
        "Landroid/hardware/camera2/CameraCaptureSession;",
        "setRepeatingRequest",
        "(Landroid/hardware/camera2/CaptureRequest;Landroid/hardware/camera2/CameraCaptureSession$CaptureCallback;Landroid/os/Handler;)I",
        sess,
        &[Value::Obj(cap_req), Value::Obj(cap_cb), Value::Null],
    );
    assert_eq!(count(&e, CAM_CAP_CB, "onCaptureCompleted"), 1);
    assert_eq!(count(&e, IMG_CB, "onImageAvailable"), 1);

    // Inject custom camera frame from agent (`AG-10`) and read via ImageReader -> Plane -> ByteBuffer
    let custom = vec![0xFF, 0xD8, b'M', b'1', b'0', 0xFF, 0xD9];
    assert_eq!(
        e.m10_inject_camera_frame("0", 64, 64, 256, custom.clone())
            .expect("inject frame"),
        1
    );
    assert_eq!(count(&e, IMG_CB, "onImageAvailable"), 2);

    let Value::Obj(img) = icall(
        &mut e,
        "Landroid/media/ImageReader;",
        "acquireLatestImage",
        "()Landroid/media/Image;",
        reader,
        &[],
    ) else {
        panic!("expected Image");
    };
    let Value::Obj(planes) = icall(
        &mut e,
        "Landroid/media/Image;",
        "getPlanes",
        "()[Landroid/media/Image$Plane;",
        img,
        &[],
    ) else {
        panic!("expected Plane[]");
    };
    let Value::Obj(plane0) = e.heap.as_array_elems(planes).expect("planes").0[0] else {
        panic!("expected plane0");
    };
    let Value::Obj(bb) = icall(
        &mut e,
        "Landroid/media/Image$Plane;",
        "getBuffer",
        "()Ljava/nio/ByteBuffer;",
        plane0,
        &[],
    ) else {
        panic!("expected ByteBuffer");
    };
    assert_eq!(
        icall(&mut e, "Ljava/nio/ByteBuffer;", "remaining", "()I", bb, &[]),
        Value::Int(custom.len() as i32)
    );
    let dst = e
        .heap
        .alloc_array(rd_vm::heap::ElemKind::Int, custom.len(), "[B")
        .expect("byte[]");
    *e.heap.array_elems_mut(dst).expect("elems") = vec![Value::Int(0); custom.len()];
    icall(
        &mut e,
        "Ljava/nio/ByteBuffer;",
        "get",
        "([B)Ljava/nio/ByteBuffer;",
        bb,
        &[Value::Obj(dst)],
    );
    let got: Vec<u8> = e
        .heap
        .as_array_elems(dst)
        .expect("read dst")
        .0
        .iter()
        .map(|v| v.as_int().unwrap_or(0) as i8 as u8)
        .collect();
    assert_eq!(got, custom);

    icall(
        &mut e,
        "Landroid/hardware/camera2/CameraDevice;",
        "close",
        "()V",
        dev,
        &[],
    );
    assert_eq!(count(&e, CAM_DEV_CB, "onClosed"), 1);
}

#[test]
fn biometrics_framework_and_androidx_callbacks() {
    let mut e = engine();
    let bm = host_obj(&mut e, "Landroid/hardware/biometrics/BiometricManager;");
    assert_eq!(
        icall(
            &mut e,
            "Landroid/hardware/biometrics/BiometricManager;",
            "canAuthenticate",
            "()I",
            bm,
            &[]
        ),
        Value::Int(0)
    );

    let prompt = host_obj(&mut e, "Landroid/hardware/biometrics/BiometricPrompt;");
    let fw_cb = dex_obj(&mut e, BIO_FW_CB);
    icall(
        &mut e,
        "Landroid/hardware/biometrics/BiometricPrompt;",
        "authenticate",
        "(Landroid/os/CancellationSignal;Ljava/util/concurrent/Executor;Landroid/hardware/biometrics/BiometricPrompt$AuthenticationCallback;)V",
        prompt,
        &[Value::Null, Value::Null, Value::Obj(fw_cb)],
    );
    assert!(e.fw.vdev.biometrics.active_prompt.is_some());
    // Failed attempt keeps prompt open; success consumes prompt
    assert!(e
        .m10_trigger_biometric(BiometricOutcome::Failed, false)
        .expect("fail"));
    assert_eq!(count(&e, BIO_FW_CB, "onAuthenticationFailed"), 1);
    assert!(e
        .m10_trigger_biometric(BiometricOutcome::Success, false)
        .expect("succeed"));
    assert_eq!(count(&e, BIO_FW_CB, "onAuthenticationSucceeded"), 1);
    assert!(e.fw.vdev.biometrics.active_prompt.is_none());

    // AndroidX BiometricPrompt + lockout error
    let ax_prompt = host_obj(&mut e, "Landroidx/biometric/BiometricPrompt;");
    let ax_cb = dex_obj(&mut e, BIO_AX_CB);
    let act = e.fw.current_activity.expect("act");
    icall(
        &mut e,
        "Landroidx/biometric/BiometricPrompt;",
        "<init>",
        "(Landroidx/fragment/app/FragmentActivity;Landroidx/biometric/BiometricPrompt$AuthenticationCallback;)V",
        ax_prompt,
        &[Value::Obj(act), Value::Obj(ax_cb)],
    );
    let info = host_obj(&mut e, "Landroidx/biometric/BiometricPrompt$PromptInfo;");
    icall(
        &mut e,
        "Landroidx/biometric/BiometricPrompt;",
        "authenticate",
        "(Landroidx/biometric/BiometricPrompt$PromptInfo;)V",
        ax_prompt,
        &[Value::Obj(info)],
    );
    assert!(e
        .m10_trigger_biometric(
            BiometricOutcome::Error {
                code: 7,
                message: "Lockout".into()
            },
            false
        )
        .expect("lockout"));
    assert_eq!(count(&e, BIO_AX_CB, "onAuthenticationError"), 1);

    // Hardware absent => BIOMETRIC_ERROR_NO_HARDWARE (12)
    e.m10_set_biometric_hardware(false, false);
    assert_eq!(
        icall(
            &mut e,
            "Landroid/hardware/biometrics/BiometricManager;",
            "canAuthenticate",
            "()I",
            bm,
            &[]
        ),
        Value::Int(12)
    );
}

#[test]
fn audio_focus_pcm_mediaplayer_mediasession_and_projection() {
    let mut e = engine();
    let am = host_obj(&mut e, "Landroid/media/AudioManager;");
    let cb1 = dex_obj(&mut e, AUDIO_CB);
    let cb2 = dex_obj(&mut e, AUDIO_CB);
    assert_eq!(
        icall(
            &mut e,
            "Landroid/media/AudioManager;",
            "requestAudioFocus",
            "(Landroid/media/AudioManager$OnAudioFocusChangeListener;II)I",
            am,
            &[Value::Obj(cb1), Value::Int(3), Value::Int(1)]
        ),
        Value::Int(1)
    );
    // Second listener steals focus -> cb1 receives onAudioFocusChange(AUDIOFOCUS_LOSS)
    assert_eq!(
        icall(
            &mut e,
            "Landroid/media/AudioManager;",
            "requestAudioFocus",
            "(Landroid/media/AudioManager$OnAudioFocusChangeListener;II)I",
            am,
            &[Value::Obj(cb2), Value::Int(3), Value::Int(1)]
        ),
        Value::Int(1)
    );
    assert_eq!(count(&e, AUDIO_CB, "onAudioFocusChange"), 1);

    // AudioRecord reads agent-injected PCM; AudioTrack writes PCM
    e.m10_push_audio_input(&[0x11, 0x22, 0x33, 0x44]);
    let rec = host_obj(&mut e, "Landroid/media/AudioRecord;");
    icall(
        &mut e,
        "Landroid/media/AudioRecord;",
        "<init>",
        "(IIIII)V",
        rec,
        &[
            Value::Int(1),
            Value::Int(16_000),
            Value::Int(16),
            Value::Int(2),
            Value::Int(2048),
        ],
    );
    icall(
        &mut e,
        "Landroid/media/AudioRecord;",
        "startRecording",
        "()V",
        rec,
        &[],
    );
    let pcm_buf = e
        .heap
        .alloc_array(rd_vm::heap::ElemKind::Int, 4, "[B")
        .expect("pcm[4]");
    *e.heap.array_elems_mut(pcm_buf).expect("elems") = vec![Value::Int(0); 4];
    assert_eq!(
        icall(
            &mut e,
            "Landroid/media/AudioRecord;",
            "read",
            "([BII)I",
            rec,
            &[Value::Obj(pcm_buf), Value::Int(0), Value::Int(4)]
        ),
        Value::Int(4)
    );
    assert_eq!(
        e.heap.as_array_elems(pcm_buf).expect("elems").0[0],
        Value::Int(0x11)
    );

    let trk = host_obj(&mut e, "Landroid/media/AudioTrack;");
    icall(
        &mut e,
        "Landroid/media/AudioTrack;",
        "<init>",
        "(IIIIII)V",
        trk,
        &[
            Value::Int(3),
            Value::Int(48_000),
            Value::Int(4),
            Value::Int(2),
            Value::Int(4096),
            Value::Int(1),
        ],
    );
    icall(
        &mut e,
        "Landroid/media/AudioTrack;",
        "play",
        "()V",
        trk,
        &[],
    );
    assert_eq!(
        icall(
            &mut e,
            "Landroid/media/AudioTrack;",
            "write",
            "([BII)I",
            trk,
            &[Value::Obj(pcm_buf), Value::Int(0), Value::Int(4)]
        ),
        Value::Int(4)
    );
    assert_eq!(e.fw.vdev.audio_tracks[&trk].bytes_written, 4);

    // MediaPlayer state machine + callbacks
    let mp = host_obj(&mut e, "Landroid/media/MediaPlayer;");
    icall(
        &mut e,
        "Landroid/media/MediaPlayer;",
        "<init>",
        "()V",
        mp,
        &[],
    );
    let mp_cb = dex_obj(&mut e, MP_CB);
    icall(
        &mut e,
        "Landroid/media/MediaPlayer;",
        "setOnPreparedListener",
        "(Landroid/media/MediaPlayer$OnPreparedListener;)V",
        mp,
        &[Value::Obj(mp_cb)],
    );
    icall(
        &mut e,
        "Landroid/media/MediaPlayer;",
        "setOnCompletionListener",
        "(Landroid/media/MediaPlayer$OnCompletionListener;)V",
        mp,
        &[Value::Obj(mp_cb)],
    );
    icall(
        &mut e,
        "Landroid/media/MediaPlayer;",
        "setOnSeekCompleteListener",
        "(Landroid/media/MediaPlayer$OnSeekCompleteListener;)V",
        mp,
        &[Value::Obj(mp_cb)],
    );
    let url = s(&mut e, "file:///music.mp3");
    icall(
        &mut e,
        "Landroid/media/MediaPlayer;",
        "setDataSource",
        "(Ljava/lang/String;)V",
        mp,
        &[url],
    );
    icall(
        &mut e,
        "Landroid/media/MediaPlayer;",
        "prepareAsync",
        "()V",
        mp,
        &[],
    );
    assert_eq!(count(&e, MP_CB, "onPrepared"), 1);
    icall(
        &mut e,
        "Landroid/media/MediaPlayer;",
        "start",
        "()V",
        mp,
        &[],
    );
    icall(
        &mut e,
        "Landroid/media/MediaPlayer;",
        "seekTo",
        "(I)V",
        mp,
        &[Value::Int(12_500)],
    );
    assert_eq!(count(&e, MP_CB, "onSeekComplete"), 1);
    assert_eq!(e.m10_complete_media_players().expect("complete"), 1);
    assert_eq!(count(&e, MP_CB, "onCompletion"), 1);

    // MediaSession + wide `onSeekTo(J)V` callback via `m10_media_action`
    let ms = host_obj(&mut e, "Landroid/media/session/MediaSession;");
    let tag = s(&mut e, "player");
    let act = e.fw.current_activity.expect("act");
    icall(
        &mut e,
        "Landroid/media/session/MediaSession;",
        "<init>",
        "(Landroid/content/Context;Ljava/lang/String;)V",
        ms,
        &[Value::Obj(act), tag],
    );
    let ms_cb = dex_obj(&mut e, MS_CB);
    icall(
        &mut e,
        "Landroid/media/session/MediaSession;",
        "setCallback",
        "(Landroid/media/session/MediaSession$Callback;)V",
        ms,
        &[Value::Obj(ms_cb)],
    );
    icall(
        &mut e,
        "Landroid/media/session/MediaSession;",
        "setActive",
        "(Z)V",
        ms,
        &[Value::Int(1)],
    );
    assert_eq!(e.m10_media_action("play", None).expect("play"), 1);
    assert_eq!(e.m10_media_action("next", None).expect("next"), 1);
    assert_eq!(e.m10_media_action("seek", Some(42_000)).expect("seek"), 1);
    assert_eq!(count(&e, MS_CB, "onPlay"), 1);
    assert_eq!(count(&e, MS_CB, "onSkipToNext"), 1);
    assert_eq!(count(&e, MS_CB, "onSeekTo"), 1);

    // MediaProjection -> VirtualDisplay -> RGBA_8888 ImageReader capture + stop callback
    let mpm = host_obj(&mut e, "Landroid/media/projection/MediaProjectionManager;");
    let Value::Obj(intent) = icall(
        &mut e,
        "Landroid/media/projection/MediaProjectionManager;",
        "createScreenCaptureIntent",
        "()Landroid/content/Intent;",
        mpm,
        &[],
    ) else {
        panic!("expected capture Intent");
    };
    let Value::Obj(proj) = icall(
        &mut e,
        "Landroid/media/projection/MediaProjectionManager;",
        "getMediaProjection",
        "(ILandroid/content/Intent;)Landroid/media/projection/MediaProjection;",
        mpm,
        &[Value::Int(-1), Value::Obj(intent)],
    ) else {
        panic!("expected MediaProjection");
    };
    let proj_cb = dex_obj(&mut e, PROJ_CB);
    icall(
        &mut e,
        "Landroid/media/projection/MediaProjection;",
        "registerCallback",
        "(Landroid/media/projection/MediaProjection$Callback;Landroid/os/Handler;)V",
        proj,
        &[Value::Obj(proj_cb), Value::Null],
    );
    let Value::Obj(ir) = scall(
        &mut e,
        "Landroid/media/ImageReader;",
        "newInstance",
        "(IIII)Landroid/media/ImageReader;",
        &[Value::Int(32), Value::Int(32), Value::Int(1), Value::Int(2)],
    ) else {
        panic!("expected RGBA ImageReader");
    };
    let Value::Obj(ir_surf) = icall(
        &mut e,
        "Landroid/media/ImageReader;",
        "getSurface",
        "()Landroid/view/Surface;",
        ir,
        &[],
    ) else {
        panic!("expected Surface");
    };
    let vd_name = s(&mut e, "screen-cap");
    icall(
        &mut e,
        "Landroid/media/projection/MediaProjection;",
        "createVirtualDisplay",
        "(Ljava/lang/String;IIIILandroid/view/Surface;Landroid/hardware/display/VirtualDisplay$Callback;Landroid/os/Handler;)Landroid/hardware/display/VirtualDisplay;",
        proj,
        &[
            vd_name,
            Value::Int(32),
            Value::Int(32),
            Value::Int(280),
            Value::Int(0),
            Value::Obj(ir_surf),
            Value::Null,
            Value::Null,
        ],
    );
    let cap_img = icall(
        &mut e,
        "Landroid/media/ImageReader;",
        "acquireLatestImage",
        "()Landroid/media/Image;",
        ir,
        &[],
    );
    assert!(matches!(cap_img, Value::Obj(_)));
    icall(
        &mut e,
        "Landroid/media/projection/MediaProjection;",
        "stop",
        "()V",
        proj,
        &[],
    );
    assert_eq!(count(&e, PROJ_CB, "onStop"), 1);
}

#[test]
fn connectivity_telephony_sms_ble_and_nfc_end_to_end() {
    let mut e = engine();
    let cm = host_obj(&mut e, "Landroid/net/ConnectivityManager;");
    let net_cb = dex_obj(&mut e, NET_CB);
    icall(
        &mut e,
        "Landroid/net/ConnectivityManager;",
        "registerDefaultNetworkCallback",
        "(Landroid/net/ConnectivityManager$NetworkCallback;)V",
        cm,
        &[Value::Obj(net_cb)],
    );
    assert_eq!(count(&e, NET_CB, "onAvailable"), 1);
    e.m10_set_network(false, "none", false, true)
        .expect("airplane");
    assert_eq!(count(&e, NET_CB, "onLost"), 1);
    e.m10_set_network(true, "cellular", true, false)
        .expect("cellular");
    assert_eq!(count(&e, NET_CB, "onAvailable"), 2);
    assert_eq!(count(&e, NET_CB, "onCapabilitiesChanged"), 1);

    // Outbound + inbound SMS
    let Value::Obj(sms_mgr) = scall(
        &mut e,
        "Landroid/telephony/SmsManager;",
        "getDefault",
        "()Landroid/telephony/SmsManager;",
        &[],
    ) else {
        panic!("expected SmsManager");
    };
    let dest = s(&mut e, "+5538999991234");
    let body = s(&mut e, "Hello from RustyDroid");
    icall(
        &mut e,
        "Landroid/telephony/SmsManager;",
        "sendTextMessage",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Landroid/app/PendingIntent;Landroid/app/PendingIntent;)V",
        sms_mgr,
        &[dest, Value::Null, body, Value::Null, Value::Null],
    );
    assert_eq!(e.fw.vdev.telephony.sent_sms.len(), 1);

    let bcast = dex_obj(&mut e, BCAST_CB);
    e.fw.m8.receivers.push(bcast);
    assert_eq!(
        e.m10_receive_sms("+551188887777", "OTP 424242")
            .expect("sms"),
        1
    );
    assert_eq!(count(&e, BCAST_CB, "onReceive"), 1);

    // BLE scanner + advertisement injection
    let Value::Obj(bt_adapter) = scall(
        &mut e,
        "Landroid/bluetooth/BluetoothAdapter;",
        "getDefaultAdapter",
        "()Landroid/bluetooth/BluetoothAdapter;",
        &[],
    ) else {
        panic!("expected BluetoothAdapter");
    };
    let Value::Obj(scanner) = icall(
        &mut e,
        "Landroid/bluetooth/BluetoothAdapter;",
        "getBluetoothLeScanner",
        "()Landroid/bluetooth/le/BluetoothLeScanner;",
        bt_adapter,
        &[],
    ) else {
        panic!("expected BluetoothLeScanner");
    };
    let ble_cb = dex_obj(&mut e, BLE_CB);
    icall(
        &mut e,
        "Landroid/bluetooth/le/BluetoothLeScanner;",
        "startScan",
        "(Landroid/bluetooth/le/ScanCallback;)V",
        scanner,
        &[Value::Obj(ble_cb)],
    );
    assert_eq!(
        e.m10_inject_ble_advertisement("AA:11:22:33:44:55", "HeartRate", -48, vec![1, 2, 3])
            .expect("ble adv"),
        1
    );
    assert_eq!(count(&e, BLE_CB, "onScanResult"), 1);

    // NFC: first dispatch via Activity.onNewIntent, then via ReaderCallback
    assert_eq!(
        e.m10_inject_nfc_tag(vec![0x04, 1, 2, 3], Some("Pay 10 BRL".into()), None)
            .expect("nfc intent"),
        2 // Main.onNewIntent + BcastCb.onReceive
    );
    assert_eq!(count(&e, MAIN, "onNewIntent"), 1);

    let act = e.fw.current_activity.expect("act");
    let Value::Obj(nfc_adapter) = scall(
        &mut e,
        "Landroid/nfc/NfcAdapter;",
        "getDefaultAdapter",
        "(Landroid/content/Context;)Landroid/nfc/NfcAdapter;",
        &[Value::Obj(act)],
    ) else {
        panic!("expected NfcAdapter");
    };
    let nfc_cb = dex_obj(&mut e, NFC_CB);
    icall(
        &mut e,
        "Landroid/nfc/NfcAdapter;",
        "enableReaderMode",
        "(Landroid/app/Activity;Landroid/nfc/NfcAdapter$ReaderCallback;ILandroid/os/Bundle;)V",
        nfc_adapter,
        &[
            Value::Obj(act),
            Value::Obj(nfc_cb),
            Value::Int(1),
            Value::Null,
        ],
    );
    assert_eq!(
        e.m10_inject_nfc_tag(
            vec![0x04, 9, 9, 9],
            None,
            Some("https://rustydroid.dev".into())
        )
        .expect("nfc reader"),
        1
    );
    assert_eq!(count(&e, NFC_CB, "onTagDiscovered"), 1);
}

#[test]
fn sysconfig_battery_wakelock_darkmode_locale_orientation_and_clipboard() {
    let mut e = engine();
    let act = e.fw.current_activity.expect("act");

    // Sticky ACTION_BATTERY_CHANGED via Context.registerReceiver(null, filter)
    let filter = host_obj(&mut e, "Landroid/content/IntentFilter;");
    let bat_action = s(&mut e, "android.intent.action.BATTERY_CHANGED");
    icall(
        &mut e,
        "Landroid/content/IntentFilter;",
        "<init>",
        "(Ljava/lang/String;)V",
        filter,
        &[bat_action],
    );
    e.m10_set_battery(42, false, Some(0), Some(315), Some(true))
        .expect("set battery");
    let Value::Obj(sticky) = icall(
        &mut e,
        "Landroid/app/Activity;",
        "registerReceiver",
        "(Landroid/content/BroadcastReceiver;Landroid/content/IntentFilter;)Landroid/content/Intent;",
        act,
        &[Value::Null, Value::Obj(filter)],
    ) else {
        panic!("expected sticky battery Intent");
    };
    let lvl_key = s(&mut e, "level");
    let lvl = rd_vm::framework::call_host_instance(
        &mut e,
        "Landroid/content/Intent;",
        "getIntExtra",
        "(Ljava/lang/String;I)I",
        sticky,
        &[lvl_key, Value::Int(-1)],
    )
    .expect("getIntExtra")
    .expect("some");
    assert_eq!(lvl, Value::Int(42));

    // PowerManager + WakeLock ref counting
    let pm = host_obj(&mut e, "Landroid/os/PowerManager;");
    assert_eq!(
        icall(
            &mut e,
            "Landroid/os/PowerManager;",
            "isPowerSaveMode",
            "()Z",
            pm,
            &[]
        ),
        Value::Int(1)
    );
    let wl_tag = s(&mut e, "m10:sync");
    let Value::Obj(wl) = icall(
        &mut e,
        "Landroid/os/PowerManager;",
        "newWakeLock",
        "(ILjava/lang/String;)Landroid/os/PowerManager$WakeLock;",
        pm,
        &[Value::Int(1), wl_tag],
    ) else {
        panic!("expected WakeLock");
    };
    icall(
        &mut e,
        "Landroid/os/PowerManager$WakeLock;",
        "acquire",
        "()V",
        wl,
        &[],
    );
    assert_eq!(
        icall(
            &mut e,
            "Landroid/os/PowerManager$WakeLock;",
            "isHeld",
            "()Z",
            wl,
            &[]
        ),
        Value::Int(1)
    );
    icall(
        &mut e,
        "Landroid/os/PowerManager$WakeLock;",
        "release",
        "()V",
        wl,
        &[],
    );
    assert_eq!(
        icall(
            &mut e,
            "Landroid/os/PowerManager$WakeLock;",
            "isHeld",
            "()Z",
            wl,
            &[]
        ),
        Value::Int(0)
    );

    // Dynamic dark_mode, locale, font_scale, orientation -> Activity.onConfigurationChanged
    e.m10_set_dark_mode(true).expect("dark");
    e.m10_set_locale("pt-BR").expect("locale");
    e.m10_set_font_scale(1.3).expect("font_scale");
    e.m10_set_orientation("landscape").expect("landscape");
    assert_eq!(count(&e, MAIN, "onConfigurationChanged"), 4);
    assert_eq!(e.fw.vdev.sysconfig.width_px, 1440);
    assert_eq!(e.fw.vdev.sysconfig.height_px, 720);

    // Clipboard shared between guest ClipData and agent m10_get_clipboard/m10_set_clipboard
    let clip_key = s(&mut e, "clipboard");
    let Value::Obj(cm) = rd_vm::m8::call_m8_instance(
        &mut e,
        "Landroid/app/Activity;",
        "getSystemService",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        act,
        &[clip_key],
    )
    .expect("getSystemService")
    .expect("clipboard") else {
        panic!("expected ClipboardManager");
    };
    let clip_cb = dex_obj(&mut e, CLIP_CB);
    icall(
        &mut e,
        "Landroid/content/ClipboardManager;",
        "addPrimaryClipChangedListener",
        "(Landroid/content/ClipboardManager$OnPrimaryClipChangedListener;)V",
        cm,
        &[Value::Obj(clip_cb)],
    );
    e.m10_set_clipboard("otp", "987654").expect("set clip");
    assert_eq!(count(&e, CLIP_CB, "onPrimaryClipChanged"), 1);
    assert_eq!(
        e.m10_get_clipboard().expect("get clip").as_deref(),
        Some("987654")
    );
}

#[test]
fn denied_permissions_raise_security_exception_cleanly() {
    let mut e = engine();
    e.set_granted_permissions(Vec::<String>::new());

    let cm = host_obj(&mut e, "Landroid/hardware/camera2/CameraManager;");
    let cam0 = s(&mut e, "0");
    let err = m10::call_instance(
        &mut e,
        "Landroid/hardware/camera2/CameraManager;",
        "openCamera",
        "(Ljava/lang/String;Landroid/hardware/camera2/CameraDevice$StateCallback;Landroid/os/Handler;)V",
        cm,
        &[cam0, Value::Null, Value::Null],
    )
    .expect_err("openCamera without CAMERA must fail");
    match err {
        VmExit::Exception(t) => assert_eq!(t.class, "Ljava/lang/SecurityException;"),
        other => panic!("expected SecurityException, got {other:?}"),
    }

    let lm = host_obj(&mut e, "Landroid/location/LocationManager;");
    let gps = s(&mut e, "gps");
    let err2 = m10::call_instance(
        &mut e,
        "Landroid/location/LocationManager;",
        "getLastKnownLocation",
        "(Ljava/lang/String;)Landroid/location/Location;",
        lm,
        &[gps],
    )
    .expect_err("getLastKnownLocation without location perm must fail");
    match err2 {
        VmExit::Exception(t) => assert_eq!(t.class, "Ljava/lang/SecurityException;"),
        other => panic!("expected SecurityException, got {other:?}"),
    }
}

#[test]
fn guest_dex_invokes_virtual_devices_through_interpreter() {
    let mut b = DexBuilder::new();
    let probe = b.class("Ltest/probe/Probe;", "Ljava/lang/Object;");
    let s_sensor = b.intern("sensor") as u16;
    let s_camera = b.intern("camera") as u16;
    let t_sm = b.type_idx("Landroid/hardware/SensorManager;");
    let t_cm = b.type_idx("Landroid/hardware/camera2/CameraManager;");
    let p_get_svc = b.proto_idx("Ljava/lang/Object;", vec!["Ljava/lang/String;".into()]);
    let m_get_svc = b.method_idx("Landroid/app/Activity;", p_get_svc, "getSystemService");
    let p_def_sensor = b.proto_idx("Landroid/hardware/Sensor;", vec!["I".into()]);
    let m_def_sensor = b.method_idx(
        "Landroid/hardware/SensorManager;",
        p_def_sensor,
        "getDefaultSensor",
    );
    let p_cam_ids = b.proto_idx("[Ljava/lang/String;", vec![]);
    let m_cam_ids = b.method_idx(
        "Landroid/hardware/camera2/CameraManager;",
        p_cam_ids,
        "getCameraIdList",
    );
    let p_bt_def = b.proto_idx("Landroid/bluetooth/BluetoothAdapter;", vec![]);
    let m_bt_def = b.method_idx(
        "Landroid/bluetooth/BluetoothAdapter;",
        p_bt_def,
        "getDefaultAdapter",
    );
    let p_bt_en = b.proto_idx("Z", vec![]);
    let m_bt_en = b.method_idx("Landroid/bluetooth/BluetoothAdapter;", p_bt_en, "isEnabled");
    let p_nfc_def = b.proto_idx(
        "Landroid/nfc/NfcAdapter;",
        vec!["Landroid/content/Context;".into()],
    );
    let m_nfc_def = b.method_idx("Landroid/nfc/NfcAdapter;", p_nfc_def, "getDefaultAdapter");
    let p_nfc_en = b.proto_idx("Z", vec![]);
    let m_nfc_en = b.method_idx("Landroid/nfc/NfcAdapter;", p_nfc_en, "isEnabled");

    // static int probe(Activity v4):
    //   v0 = v4.getSystemService("sensor"); check-cast SensorManager; v0 = v0.getDefaultSensor(1)
    //   v1 = v4.getSystemService("camera"); check-cast CameraManager; v1 = v1.getCameraIdList().length
    //   v2 = BluetoothAdapter.getDefaultAdapter().isEnabled()
    //   v3 = NfcAdapter.getDefaultAdapter(v4).isEnabled()
    //   return v1 + v2 + v3 + (v0 != null ? 10 : 0)  ==> 2 + 1 + 1 + 10 = 14
    let mut u = Vec::new();
    u.extend(op21c(0x1a, 0, s_sensor)); // const-string v0, "sensor"
    u.extend(op35c(0x6e, 2, m_get_svc, [4, 0, 0, 0, 0])); // invoke-virtual {v4, v0}, getSystemService
    u.extend(op11x(0x0c, 0)); // move-result-object v0
    u.extend(op21c(0x1f, 0, t_sm)); // check-cast v0, SensorManager
    u.extend(op11n(0x12, 1, 1)); // const/4 v1, 1
    u.extend(op35c(0x6e, 2, m_def_sensor, [0, 1, 0, 0, 0])); // invoke-virtual {v0, v1}, getDefaultSensor
    u.extend(op11x(0x0c, 0)); // move-result-object v0

    u.extend(op21c(0x1a, 1, s_camera)); // const-string v1, "camera"
    u.extend(op35c(0x6e, 2, m_get_svc, [4, 1, 0, 0, 0])); // invoke-virtual {v4, v1}, getSystemService
    u.extend(op11x(0x0c, 1)); // move-result-object v1
    u.extend(op21c(0x1f, 1, t_cm)); // check-cast v1, CameraManager
    u.extend(op35c(0x6e, 1, m_cam_ids, [1, 0, 0, 0, 0])); // invoke-virtual {v1}, getCameraIdList
    u.extend(op11x(0x0c, 1)); // move-result-object v1
    u.push(0x1121); // array-length v1, v1

    u.extend(op35c(0x71, 0, m_bt_def, [0, 0, 0, 0, 0])); // invoke-static {}, BluetoothAdapter.getDefaultAdapter
    u.extend(op11x(0x0c, 2)); // move-result-object v2
    u.extend(op35c(0x6e, 1, m_bt_en, [2, 0, 0, 0, 0])); // invoke-virtual {v2}, isEnabled
    u.extend(op11x(0x0a, 2)); // move-result v2

    u.extend(op35c(0x71, 1, m_nfc_def, [4, 0, 0, 0, 0])); // invoke-static {v4}, NfcAdapter.getDefaultAdapter
    u.extend(op11x(0x0c, 3)); // move-result-object v3
    u.extend(op35c(0x6e, 1, m_nfc_en, [3, 0, 0, 0, 0])); // invoke-virtual {v3}, isEnabled
    u.extend(op11x(0x0a, 3)); // move-result v3

    u.extend(op23x(0x90, 1, 1, 2)); // add-int v1, v1, v2
    u.extend(op23x(0x90, 1, 1, 3)); // add-int v1, v1, v3
                                    // if-eqz v0, +3 (skip adding 10 if Sensor is null)
    u.push(0x38);
    u.push(3);
    u.extend(op22b(0xd8, 1, 1, 10)); // add-int/lit8 v1, v1, 10
    u.extend(op11x(0x0f, 1)); // return v1

    let code = b.code(5, 1, 2, u);
    b.direct(
        probe,
        "probe",
        "I",
        vec!["Landroid/app/Activity;"],
        ACC_PUBLIC | ACC_STATIC,
        Some(code),
    );

    let mut arsc = ArscFix::new();
    let layout = arsc.add_layout("main");
    let main = b.class("Ltest/probe/Main;", "Landroid/app/Activity;");
    let p_scv = b.proto_idx("V", vec!["I".into()]);
    let m_scv = b.method_idx("Landroid/app/Activity;", p_scv, "setContentView");
    let mut oc = op21s(0x13, 0, (layout & 0xffff) as i16);
    if (layout >> 16) != 0 {
        oc = vec![
            0x0014,
            (layout & 0xffff) as u16,
            ((layout >> 16) & 0xffff) as u16,
        ];
    }
    oc.extend(op35c(0x6e, 2, m_scv, [1, 0, 0, 0, 0]));
    oc.extend(op10x(0x0e));
    let oc_code = b.code(3, 2, 2, oc);
    b.r#virtual(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(oc_code),
    );

    let apk_bytes = build_apk(
        "test.probe",
        "test.probe.Main",
        arsc.build(),
        vec![
            (
                "res/layout/main.xml",
                build_axml(&AxNode::new(
                    "LinearLayout",
                    vec![
                        a(true, "layout_width", AxVal::Int(-1)),
                        a(true, "layout_height", AxVal::Int(-1)),
                    ],
                    vec![],
                )),
            ),
            ("classes.dex", b.finish()),
        ],
    );
    let apk = rd_apk::Apk::from_bytes(apk_bytes).expect("apk");
    let dex = rd_dex::Dex::parse(apk.zip.read("classes.dex").expect("dex")).expect("parse");
    let mut e = Engine::new(vec![dex], VmConfig::default());
    e.set_resources(apk);
    e.m10_enable_virtual_devices();
    e.launch_app("Ltest/probe/Main;", "test.probe", vec![])
        .expect("launch");
    let act = e.fw.current_activity.expect("activity");
    let res = e
        .invoke_static(
            "Ltest/probe/Probe;",
            "probe",
            "(Landroid/app/Activity;)I",
            &[Value::Obj(act)],
        )
        .expect("probe");
    assert_eq!(res, Value::Int(14));
}
