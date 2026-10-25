//! Guest-visible proofs for reconstructed system-service/value subsets.

mod common;

use common::*;
use rd_vm::err::VmExit;
use rd_vm::heap::ElemKind;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef};

const GESTURE: &str = "Landroid/gesture/Gesture;";
const GESTURE_POINT: &str = "Landroid/gesture/GesturePoint;";
const GESTURE_STROKE: &str = "Landroid/gesture/GestureStroke;";

fn instance(engine: &mut Engine, class: &str) -> ObjRef {
    engine
        .heap
        .alloc_instance(class.to_string(), Vec::new())
        .unwrap()
}
fn object(value: Value) -> ObjRef {
    let Value::Obj(reference) = value else {
        panic!("expected object, got {value:?}");
    };
    reference
}
fn call(engine: &mut Engine, recv: ObjRef, name: &str, sig: &str, args: &[Value]) -> Value {
    engine.invoke_instance(recv, name, sig, args).unwrap()
}
fn field(engine: &Engine, recv: ObjRef, name: &str) -> Value {
    engine.heap.get_field(recv, name).unwrap()
}
fn field_obj(engine: &Engine, recv: ObjRef, name: &str) -> ObjRef {
    object(field(engine, recv, name))
}
fn float_field(engine: &Engine, recv: ObjRef, name: &str) -> f32 {
    match field(engine, recv, name) {
        Value::Float(v) => v,
        other => panic!("expected float field {name}, got {other:?}"),
    }
}
fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.001,
        "expected {expected}, got {actual}"
    );
}
fn assert_throws(result: Result<Value, VmExit>, expected: &str) {
    match result {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, expected),
        other => panic!("expected {expected}, got {other:?}"),
    }
}
fn point(engine: &mut Engine, x: f32, y: f32, timestamp: i64) -> ObjRef {
    let point = instance(engine, GESTURE_POINT);
    call(
        engine,
        point,
        "<init>",
        "(FFJ)V",
        &[Value::Float(x), Value::Float(y), Value::Long(timestamp)],
    );
    point
}
fn stroke(engine: &mut Engine, points: &[(f32, f32, i64)]) -> ObjRef {
    let list = instance(engine, "Ljava/util/ArrayList;");
    call(engine, list, "<init>", "()V", &[]);
    for &(x, y, timestamp) in points {
        let point = point(engine, x, y, timestamp);
        call(
            engine,
            list,
            "add",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(point)],
        );
    }
    let stroke = instance(engine, GESTURE_STROKE);
    call(
        engine,
        stroke,
        "<init>",
        "(Ljava/util/ArrayList;)V",
        &[Value::Obj(list)],
    );
    stroke
}

#[test]
fn gesture_geometry_clones_ids_and_obb_follow_aosp() {
    let mut engine = engine_of(&DexBuilder::new());

    let original = point(&mut engine, 1.5, -2.5, 99);
    let point_clone = object(call(
        &mut engine,
        original,
        "clone",
        "()Ljava/lang/Object;",
        &[],
    ));
    assert_ne!(point_clone, original);
    assert_eq!(field(&engine, point_clone, "timestamp"), Value::Long(99));

    let stroke_a = stroke(
        &mut engine,
        &[(0.0, 0.0, 10), (3.0, 4.0, 20), (3.0, 9.0, 30)],
    );
    assert_close(float_field(&engine, stroke_a, "length"), 10.0);
    let points = field_obj(&engine, stroke_a, "points");
    let (values, kind) = engine.heap.as_array_elems(points).unwrap();
    assert_eq!(kind, ElemKind::Float);
    assert_eq!(
        values,
        [
            Value::Float(0.0),
            Value::Float(0.0),
            Value::Float(3.0),
            Value::Float(4.0),
            Value::Float(3.0),
            Value::Float(9.0)
        ]
        .as_slice()
    );
    let bbox = field_obj(&engine, stroke_a, "boundingBox");
    assert_eq!(float_field(&engine, bbox, "left"), 0.0);
    assert_eq!(float_field(&engine, bbox, "right"), 3.0);
    assert_eq!(float_field(&engine, bbox, "bottom"), 9.0);
    let stroke_clone = object(call(
        &mut engine,
        stroke_a,
        "clone",
        "()Ljava/lang/Object;",
        &[],
    ));
    let cloned_points = field_obj(&engine, stroke_clone, "points");
    engine.heap.array_elems_mut(points).unwrap()[0] = Value::Float(77.0);
    assert_eq!(
        engine.heap.as_array_elems(cloned_points).unwrap().0[0],
        Value::Float(0.0)
    );

    let diagonal = stroke(&mut engine, &[(0.0, 0.0, 0), (10.0, 10.0, 1)]);
    let obb = object(call(
        &mut engine,
        diagonal,
        "computeOrientedBoundingBox",
        "()Landroid/gesture/OrientedBoundingBox;",
        &[],
    ));
    assert_close(float_field(&engine, obb, "orientation"), 45.0);
    assert_close(float_field(&engine, obb, "centerX"), 5.0);
    assert_close(float_field(&engine, obb, "width"), 14.142_136);
    assert_close(float_field(&engine, obb, "height"), 0.0);
    assert_close(float_field(&engine, obb, "squareness"), 0.0);

    let empty = stroke(&mut engine, &[]);
    assert_eq!(field(&engine, empty, "boundingBox"), Value::Null);
    assert_throws(
        engine.invoke_instance(empty, "clone", "()Ljava/lang/Object;", &[]),
        "Ljava/lang/NullPointerException;",
    );

    let first = instance(&mut engine, GESTURE);
    call(&mut engine, first, "<init>", "()V", &[]);
    let second = instance(&mut engine, GESTURE);
    call(&mut engine, second, "<init>", "()V", &[]);
    let Value::Long(first_id) = call(&mut engine, first, "getID", "()J", &[]) else {
        panic!("id");
    };
    let Value::Long(second_id) = call(&mut engine, second, "getID", "()J", &[]) else {
        panic!("id");
    };
    assert_eq!(second_id - first_id, 1);

    let stroke_b = stroke(&mut engine, &[(-2.0, 2.0, 0), (6.0, 2.0, 1), (6.0, 5.0, 2)]);
    call(
        &mut engine,
        first,
        "addStroke",
        "(Landroid/gesture/GestureStroke;)V",
        &[Value::Obj(stroke_a)],
    );
    call(
        &mut engine,
        first,
        "addStroke",
        "(Landroid/gesture/GestureStroke;)V",
        &[Value::Obj(stroke_b)],
    );
    assert_eq!(
        call(&mut engine, first, "getStrokesCount", "()I", &[]),
        Value::Int(2)
    );
    assert_eq!(
        call(&mut engine, first, "getLength", "()F", &[]),
        Value::Float(21.0)
    );
    let gesture_bbox = object(call(
        &mut engine,
        first,
        "getBoundingBox",
        "()Landroid/graphics/RectF;",
        &[],
    ));
    assert_eq!(float_field(&engine, gesture_bbox, "left"), -2.0);
    assert_eq!(float_field(&engine, gesture_bbox, "top"), 0.0);
    assert_eq!(float_field(&engine, gesture_bbox, "right"), 6.0);
    assert_eq!(float_field(&engine, gesture_bbox, "bottom"), 9.0);

    // AOSP appends the null stroke before dereferencing its bounding box.
    assert_throws(
        engine.invoke_instance(
            first,
            "addStroke",
            "(Landroid/gesture/GestureStroke;)V",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_eq!(
        call(&mut engine, first, "getStrokesCount", "()I", &[]),
        Value::Int(3)
    );

    // Separate gesture proves int truncation after each fractional stroke.
    let fractional = instance(&mut engine, GESTURE);
    call(&mut engine, fractional, "<init>", "()V", &[]);
    for _ in 0..2 {
        let segment = stroke(&mut engine, &[(0.0, 0.0, 0), (1.0, 1.0, 1)]);
        call(
            &mut engine,
            fractional,
            "addStroke",
            "(Landroid/gesture/GestureStroke;)V",
            &[Value::Obj(segment)],
        );
    }
    assert_eq!(
        call(&mut engine, fractional, "getLength", "()F", &[]),
        Value::Float(2.0)
    );
}

const CONTEXT: &str = "Landroid/content/Context;";
const SENSOR_MANAGER: &str = "Landroid/hardware/SensorManager;";
const SENSOR: &str = "Landroid/hardware/Sensor;";
const CONSUMER_IR: &str = "Landroid/hardware/ConsumerIrManager;";
const CARRIER_RANGE: &str = "Landroid/hardware/ConsumerIrManager$CarrierFrequencyRange;";

fn text(engine: &mut Engine, value: &str) -> Value {
    Value::Obj(engine.heap.alloc_string(value.to_string()).unwrap())
}
fn text_of(engine: &Engine, value: &Value) -> String {
    match value {
        Value::Obj(reference) => engine.heap.as_str(*reference).unwrap().to_string(),
        other => panic!("expected String, got {other:?}"),
    }
}
fn call_static(engine: &mut Engine, class: &str, name: &str, sig: &str, args: &[Value]) -> Value {
    engine.invoke_static(class, name, sig, args).unwrap()
}
fn float_array(engine: &mut Engine, values: &[f32]) -> ObjRef {
    let array = engine
        .heap
        .alloc_array_zeroed(ElemKind::Float, values.len(), "[F")
        .unwrap();
    let values: Vec<Value> = values.iter().copied().map(Value::Float).collect();
    engine
        .heap
        .array_elems_mut(array)
        .unwrap()
        .clone_from_slice(&values);
    array
}
fn floats(engine: &Engine, array: ObjRef) -> Vec<f32> {
    engine
        .heap
        .as_array_elems(array)
        .unwrap()
        .0
        .iter()
        .map(|value| match value {
            Value::Float(v) => *v,
            other => panic!("expected float, got {other:?}"),
        })
        .collect()
}
fn assert_floats_close(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (a, e) in actual.iter().zip(expected.iter()) {
        assert_close(*a, *e);
    }
}
fn system_service(engine: &mut Engine, name: &str) -> ObjRef {
    let context = instance(engine, CONTEXT);
    let service_name = text(engine, name);
    object(call(
        engine,
        context,
        "getSystemService",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        &[service_name],
    ))
}
fn list_size(engine: &mut Engine, value: Value) -> i32 {
    match call(engine, object(value), "size", "()I", &[]) {
        Value::Int(n) => n,
        other => panic!("expected size, got {other:?}"),
    }
}

#[test]
fn sensorless_hardware_profile_and_sensor_math_follow_aosp() {
    let mut engine = engine_of(&DexBuilder::new());
    let manager = system_service(&mut engine, "sensor");
    assert_eq!(engine.heap.class_of(manager).unwrap(), SENSOR_MANAGER);
    let sensors = call(
        &mut engine,
        manager,
        "getSensorList",
        "(I)Ljava/util/List;",
        &[Value::Int(-1)],
    );
    assert_eq!(list_size(&mut engine, sensors), 0);
    assert_eq!(
        call(
            &mut engine,
            manager,
            "getDefaultSensor",
            "(I)Landroid/hardware/Sensor;",
            &[Value::Int(1)]
        ),
        Value::Null
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "isDynamicSensorDiscoverySupported",
            "()Z",
            &[]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            manager,
            "registerListener",
            "(Landroid/hardware/SensorEventListener;Landroid/hardware/Sensor;I)Z",
            &[Value::Null, Value::Null, Value::Int(3)]
        ),
        Value::Int(0)
    );
    assert_throws(
        engine.invoke_instance(
            manager,
            "flush",
            "(Landroid/hardware/SensorEventListener;)Z",
            &[Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    let r = float_array(&mut engine, &[0.0; 9]);
    let inclination = float_array(&mut engine, &[0.0; 9]);
    let gravity = float_array(&mut engine, &[0.0, 0.0, 9.81]);
    let magnetic = float_array(&mut engine, &[0.0, 20.0, -40.0]);
    assert_eq!(
        call_static(
            &mut engine,
            SENSOR_MANAGER,
            "getRotationMatrix",
            "([F[F[F[F)Z",
            &[
                Value::Obj(r),
                Value::Obj(inclination),
                Value::Obj(gravity),
                Value::Obj(magnetic)
            ]
        ),
        Value::Int(1)
    );
    assert_floats_close(
        &floats(&engine, r),
        &[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    );
    let Value::Float(inclination_angle) = call_static(
        &mut engine,
        SENSOR_MANAGER,
        "getInclination",
        "([F)F",
        &[Value::Obj(inclination)],
    ) else {
        panic!("inclination");
    };
    assert_close(inclination_angle, -1.107_149);

    let orientation = float_array(&mut engine, &[9.0; 3]);
    let returned = call_static(
        &mut engine,
        SENSOR_MANAGER,
        "getOrientation",
        "([F[F)[F",
        &[Value::Obj(r), Value::Obj(orientation)],
    );
    assert_eq!(object(returned), orientation);
    assert_floats_close(&floats(&engine, orientation), &[0.0, 0.0, 0.0]);

    let remapped = float_array(&mut engine, &[0.0; 9]);
    assert_eq!(
        call_static(
            &mut engine,
            SENSOR_MANAGER,
            "remapCoordinateSystem",
            "([FII[F)Z",
            &[
                Value::Obj(r),
                Value::Int(2),
                Value::Int(0x81),
                Value::Obj(remapped)
            ]
        ),
        Value::Int(1)
    );
    assert_floats_close(
        &floats(&engine, remapped),
        &[0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
    );
    // The in-place 3x3 path uses AOSP's fixed 16-element temporary and fails.
    assert_eq!(
        call_static(
            &mut engine,
            SENSOR_MANAGER,
            "remapCoordinateSystem",
            "([FII[F)Z",
            &[Value::Obj(r), Value::Int(1), Value::Int(2), Value::Obj(r)]
        ),
        Value::Int(0)
    );

    let Value::Float(altitude) = call_static(
        &mut engine,
        SENSOR_MANAGER,
        "getAltitude",
        "(FF)F",
        &[Value::Float(1013.25), Value::Float(900.0)],
    ) else {
        panic!("altitude");
    };
    assert!((altitude - 988.65).abs() < 0.1, "altitude={altitude}");

    let vector = float_array(&mut engine, &[0.0, 0.0, std::f32::consts::FRAC_1_SQRT_2]);
    let quaternion = float_array(&mut engine, &[0.0; 4]);
    call_static(
        &mut engine,
        SENSOR_MANAGER,
        "getQuaternionFromVector",
        "([F[F)V",
        &[Value::Obj(quaternion), Value::Obj(vector)],
    );
    assert_floats_close(
        &floats(&engine, quaternion),
        &[
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            0.0,
            std::f32::consts::FRAC_1_SQRT_2,
        ],
    );
    let matrix = float_array(&mut engine, &[0.0; 9]);
    call_static(
        &mut engine,
        SENSOR_MANAGER,
        "getRotationMatrixFromVector",
        "([F[F)V",
        &[Value::Obj(matrix), Value::Obj(vector)],
    );
    assert_floats_close(
        &floats(&engine, matrix),
        &[0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
    );

    let sensor = instance(&mut engine, SENSOR);
    let sensor_name = text(&mut engine, "Virtual Accel");
    engine.heap.put_field(sensor, "mName", sensor_name).unwrap();
    let vendor = text(&mut engine, "RustyDroid");
    engine.heap.put_field(sensor, "mVendor", vendor).unwrap();
    engine
        .heap
        .put_field(sensor, "mType", Value::Int(11))
        .unwrap();
    engine
        .heap
        .put_field(sensor, "mFlags", Value::Int(4069))
        .unwrap();
    engine
        .heap
        .put_field(sensor, "mVersion", Value::Int(7))
        .unwrap();
    call(
        &mut engine,
        sensor,
        "setRange",
        "(FF)V",
        &[Value::Float(39.24), Value::Float(0.01)],
    );
    assert_eq!(
        call(&mut engine, sensor, "getReportingMode", "()I", &[]),
        Value::Int(2)
    );
    assert_eq!(
        call(
            &mut engine,
            sensor,
            "getHighestDirectReportRateLevel",
            "()I",
            &[]
        ),
        Value::Int(3)
    );
    assert_eq!(
        call(
            &mut engine,
            sensor,
            "isDirectChannelTypeSupported",
            "(I)Z",
            &[Value::Int(2)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call_static(
            &mut engine,
            SENSOR,
            "getMaxLengthValuesArray",
            "(Landroid/hardware/Sensor;I)I",
            &[Value::Obj(sensor), Value::Int(17)]
        ),
        Value::Int(3)
    );
    let rendered = call(&mut engine, sensor, "toString", "()Ljava/lang/String;", &[]);
    assert!(text_of(&engine, &rendered).contains("Virtual Accel"));

    let ir = system_service(&mut engine, "consumer_ir");
    assert_eq!(engine.heap.class_of(ir).unwrap(), CONSUMER_IR);
    assert_eq!(
        call(&mut engine, ir, "hasIrEmitter", "()Z", &[]),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut engine,
            ir,
            "getCarrierFrequencies",
            "()[Landroid/hardware/ConsumerIrManager$CarrierFrequencyRange;",
            &[]
        ),
        Value::Null
    );
    let range = instance(&mut engine, CARRIER_RANGE);
    call(
        &mut engine,
        range,
        "<init>",
        "(Landroid/hardware/ConsumerIrManager;II)V",
        &[Value::Obj(ir), Value::Int(30_000), Value::Int(60_000)],
    );
    assert_eq!(
        call(&mut engine, range, "getMinFrequency", "()I", &[]),
        Value::Int(30_000)
    );
    assert_eq!(
        call(&mut engine, range, "getMaxFrequency", "()I", &[]),
        Value::Int(60_000)
    );
}
