//! Issue #229 (parte 2): `FusedLocationProviderClient` tinha duas fontes de
//! verdade. O fix do host (`gms_set_location_fix`) ia para `gms.location_fix`,
//! mas o cliente fused, que é despachado primeiro pelo M10, lia só
//! `vdev.location`. Aqui o caminho é o mesmo do bytecode: intrínsecos com a
//! ordem M10 → androidx → GMS.

use rd_vm::intrinsics::{call_instance_intrinsic, call_static_intrinsic};
use rd_vm::value::Value;
use rd_vm::{Engine, VmConfig};

const LOCATION_SERVICES: &str = "Lcom/google/android/gms/location/LocationServices;";
const FUSED: &str = "Lcom/google/android/gms/location/FusedLocationProviderClient;";
const FUSED_SIG: &str = "()Lcom/google/android/gms/location/FusedLocationProviderClient;";
const TASK: &str = "Lcom/google/android/gms/tasks/Task;";
const LOCATION: &str = "Landroid/location/Location;";

fn engine() -> Engine {
    let mut e = Engine::new(Vec::new(), VmConfig::default());
    // Mesmo estado do app em execução com hardware virtual ligado (padrão do `rd app run`).
    e.fw.vdev.virtual_hardware_enabled = true;
    e
}

fn fused(e: &mut Engine) -> rd_vm::heap::ObjRef {
    match call_static_intrinsic(
        e,
        LOCATION_SERVICES,
        "getFusedLocationProviderClient",
        FUSED_SIG,
        &[],
    )
    .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("fused client: {other:?}"),
    }
}

/// `getLastLocation().getResult().getLatitude()`, como um app faria.
fn last_latitude(e: &mut Engine) -> Option<f64> {
    let f = fused(e);
    let task =
        match call_instance_intrinsic(e, FUSED, "getLastLocation", &format!("(){TASK};"), f, &[])
            .unwrap()
        {
            Some(Value::Obj(r)) => r,
            other => panic!("getLastLocation: {other:?}"),
        };
    let loc = match call_instance_intrinsic(e, TASK, "getResult", "()Ljava/lang/Object;", task, &[])
        .unwrap()
    {
        Some(Value::Obj(r)) => r,
        other => panic!("getResult: {other:?}"),
    };
    match call_instance_intrinsic(e, LOCATION, "getLatitude", "()D", loc, &[]).unwrap() {
        Some(Value::Double(d)) => Some(d),
        _ => None,
    }
}

#[test]
fn host_fix_is_what_dex_fused_callers_see() {
    let mut e = engine();
    e.gms_set_location_fix(-23.55, -46.63, 8.0);
    let lat = last_latitude(&mut e).expect("latitude");
    assert!(
        (lat + 23.55).abs() < 1e-9,
        "fix do host ignorado: lat={lat}"
    );
}

#[test]
fn last_write_wins_between_agent_and_host() {
    let mut e = engine();
    e.gms_set_location_fix(-23.55, -46.63, 8.0);
    e.m10_set_location(-22.9, -43.2, None, None, None, None)
        .unwrap();
    let lat = last_latitude(&mut e).expect("latitude");
    assert!(
        (lat + 22.9).abs() < 1e-9,
        "agente não sobrescreveu o host: lat={lat}"
    );

    e.gms_set_location_fix(-19.92, -43.94, 5.0);
    let lat = last_latitude(&mut e).expect("latitude");
    assert!(
        (lat + 19.92).abs() < 1e-9,
        "host não sobrescreveu o agente: lat={lat}"
    );
}
