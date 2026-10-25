//! Run the ordinary SDK catalog through the shared portable VM, retaining errors.
use as_framework::App;
use as_platform::{host::HostPlatform, host_fs::HostFileSystem};
use rd_vm::{Value, VmExit};
use serde_json::{json, Value as Json};
use std::{path::PathBuf, rc::Rc, time::Instant};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn execute(app: &mut App, index: i32, asynchronous: bool) -> Result<Value, VmExit> {
    const RUNNER: &str = "Lorg/androswitch/conformance/RunBook;";
    if !asynchronous {
        return app.vm.invoke_static(
            RUNNER,
            "execute",
            "(I)Ljava/lang/String;",
            &[Value::Int(index)],
        );
    }
    app.vm
        .invoke_static(RUNNER, "beginAsync", "(I)V", &[Value::Int(index)])?;
    let started = Instant::now();
    while started.elapsed().as_secs() < 5 {
        app.draw()?;
        let result = app
            .vm
            .invoke_static(RUNNER, "pollAsync", "()Ljava/lang/String;", &[])?;
        if let Value::Obj(object) = result {
            if !app.vm.heap.as_str(object)?.is_empty() {
                return Ok(Value::Obj(object));
            }
        } else if !matches!(result, Value::Null) {
            return Err(rd_vm::err::vm_error(
                "TEST_RESULT_TYPE",
                "asynchronous result is not a String",
            )
            .into());
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    Err(rd_vm::err::vm_error(
        "TEST_TIMEOUT",
        "asynchronous SDK case did not complete in five seconds",
    )
    .into())
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments.len() != 4 {
        return Err("usage: as-conformance APK CATALOG_JSON FRESH_STORAGE OUTPUT_JSON".into());
    }
    if !rd_vm::PORTABLE_CORE {
        return Err("conformance requires the no_std VM, separate from rd-vm/std".into());
    }
    let catalog: Json = serde_json::from_slice(&std::fs::read(&arguments[1])?)?;
    let rows = catalog["cases"].as_array().ok_or("missing catalog")?;
    let root = PathBuf::from(&arguments[2]);
    if root.exists() {
        return Err("use fresh storage; existing user files are never removed".into());
    }
    let fs = Rc::new(HostFileSystem::new(&root)?);
    let packages = as_package::PackageManager::new(fs.clone());
    packages.initialize()?;
    let record = packages.install(std::fs::read(&arguments[0])?, true, None)?;
    let platform = Rc::new(HostPlatform::default());
    let mut current = None;
    let mut results = Vec::new();
    let timer = Instant::now();
    for row in rows {
        if current.is_none() {
            let apk = packages.load_apk(&record.package, None)?.1;
            let mut app = App::new_with_options(
                apk,
                fs.clone(),
                platform.clone(),
                640,
                360,
                None,
                as_framework::AppOptions {
                    source_apk: Some(format!(
                        "installed/{}/{:020}/package.apk",
                        record.package, record.revision
                    )),
                    ..Default::default()
                },
            )
            .map_err(|e| e.to_string())?;
            app.android.bind_runtime_resources(platform.clone());
            app.launch()
                .map_err(|e| format!("suite UI bootstrap: {e}"))?;
            current = Some(app);
        }
        let app = current.as_mut().unwrap();
        let index = row["index"].as_i64().ok_or("invalid catalog index")?;
        let started = Instant::now();
        let asynchronous = matches!(
            row["id"].as_str(),
            Some("activity.roundtrip" | "activity.handler-order")
        );
        let execution = execute(app, index as i32, asynchronous);
        let mut item = json!({"id":row["id"],"group":row["group"],"kind":row["kind"],"family":row["family"],"status":"ERROR"});
        let mut reset = false;
        match execution {
            Ok(Value::Obj(object)) => {
                let value = app.vm.heap.as_str(object)?;
                let parts: Vec<_> = value.splitn(3, '\t').collect();
                if parts.len() != 3 {
                    return Err(format!("malformed case result: {value}").into());
                }
                item["status"] = parts[0].into();
                item["detail"] = parts[2].into();
            }
            Ok(other) => return Err(format!("invalid case return: {other:?}").into()),
            Err(error) => {
                item["status"] = match &error {
                    VmExit::Error(e)
                        if matches!(
                            e.code.as_str(),
                            "API_UNAVAILABLE"
                                | "NOT_FOUND"
                                | "PLATFORM_FIELD_UNAVAILABLE"
                                | "NATIVE_BACKEND_UNAVAILABLE"
                                | "DISPLAY_ORIENTATION_UNAVAILABLE"
                        ) =>
                    {
                        "UNSUPPORTED"
                    }
                    VmExit::Exception(e) if e.class == "Ljava/lang/NoClassDefFoundError;" => {
                        "BLOCKED"
                    }
                    VmExit::Exception(_) => "FAIL",
                    _ => "ERROR",
                }
                .into();
                item["detail"] = error.to_string().into();
                item["boundary"] =
                    "portable VM; no Android native/graphics/audio/worker backend".into();
                // A failed initializer or interrupted callback must not poison
                // the next independent case. Recreate the ordinary app VM.
                reset = true;
            }
        }
        item["elapsed_us"] = (started.elapsed().as_micros() as u64).into();
        eprintln!(
            "{}: {}",
            row["id"].as_str().unwrap(),
            item["status"].as_str().unwrap()
        );
        results.push(item);
        let output = json!({"schema_version":1,"suite":"AndroSwitch Conformance","backend":"portable_host","scope":"Declared catalog only; native JNI/GPU/audio need actual Horizon or Android backend; not complete Android coverage","catalog":arguments[1],"reference_api":26,"sensor_tests":false,"elapsed_seconds":timer.elapsed().as_secs_f64(),"total":rows.len(),"completed":results.len(),"cases":results});
        std::fs::write(&arguments[3], serde_json::to_vec_pretty(&output)?)?;
        if reset {
            current = None;
        }
    }
    println!(
        "Completed {} cases; inspect individual results: {}",
        results.len(),
        arguments[3]
    );
    Ok(())
}
