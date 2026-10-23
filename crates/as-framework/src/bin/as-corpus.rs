//! Host-only corpus/benchmark tool over the same no_std VM/framework as NRO.
use as_framework::App;
use as_platform::{host::HostPlatform, host_fs::HostFileSystem};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{path::PathBuf, rc::Rc, time::Instant};
#[derive(Deserialize)]
struct Case {
    id: String,
    apk: String,
    expected: String,
    #[serde(default)]
    label_prefix: String,
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 4 {
        return Err("usage: as-corpus CORPUS_JSON STORAGE_ROOT OUTPUT_JSON CYCLES".into());
    }
    if !rd_vm::PORTABLE_CORE {
        return Err("run as-framework separately from rd-vm/std".into());
    }
    let cycles = args[3].parse::<usize>()?;
    if !(1..=200).contains(&cycles) {
        return Err("cycles must be 1..200".into());
    }
    let cases: Vec<Case> = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let root = PathBuf::from(&args[1]);
    if root.exists() {
        return Err("fresh storage root required; previous user data will not be removed".into());
    }
    let fs = Rc::new(HostFileSystem::new(&root)?);
    let pm = as_package::PackageManager::new(fs.clone());
    pm.initialize()?;
    let mut results = Vec::<Value>::new();
    let mut passed = true;
    for (index, case) in cases.iter().enumerate() {
        if !matches!(case.expected.as_str(), "run" | "evaluate") {
            return Err("unknown corpus expectation".into());
        }
        let path = format!("apks/corpus-{index}.apk");
        std::fs::copy(&case.apk, root.join(&path))?;
        let record = match pm.install_file(&path, true, None) {
            Ok(record) => record,
            Err(e) => {
                passed &= case.expected != "run";
                results
                    .push(json!({"id":case.id,"status":"blocked_install","error":e.to_string()}));
                continue;
            }
        };
        let mut sessions = vec![];
        let mut failure = None;
        for cycle in 0..if case.expected == "run" { cycles } else { 1 } {
            let started = Instant::now();
            let apk = pm.load_apk(&record.package, None)?.1;
            let opened = Instant::now();
            let app = App::new(
                apk,
                fs.clone(),
                Rc::new(HostPlatform::default()),
                640,
                360,
                None,
            );
            let mut app = match app {
                Ok(app) => app,
                Err(e) => {
                    failure = Some(e.to_string());
                    break;
                }
            };
            if let Err(e) = app.launch() {
                let _ = app.android.abort_startup();
                failure = Some(e.to_string());
                break;
            }
            let launched = Instant::now();
            let labels: Vec<_> = app
                .android
                .views()
                .iter()
                .filter(|v| !v.text.is_empty())
                .map(|v| v.text.clone())
                .collect();
            if !case.label_prefix.is_empty()
                && !labels.iter().any(|v| v.starts_with(&case.label_prefix))
            {
                return Err(format!("{}: missing expected UI: {labels:?}", case.id).into());
            }
            let mut frame_us = Vec::new();
            for _ in 0..30 {
                let t = Instant::now();
                app.draw().map_err(|e| e.to_string())?;
                frame_us.push(t.elapsed().as_secs_f64() * 1e6);
            }
            app.vm.gc();
            app.draw().map_err(|e| e.to_string())?;
            sessions.push(json!({"cycle":cycle,"load_apk_ms":opened.duration_since(started).as_secs_f64()*1e3,"vm_startup_ms":launched.duration_since(opened).as_secs_f64()*1e3,"frame_us":frame_us,"managed_heap_bytes_after_gc":app.vm.heap.used(),"frame_hash":format!("{:016x}",app.frame.checksum()),"labels":labels}));
            app.android.stop(&mut app.vm).map_err(|e| e.to_string())?;
        }
        if failure.is_some() && case.expected == "run" {
            passed = false;
        }
        results.push(json!({"id":case.id,"package":record.package,"apk_sha256":record.apk_sha256,"expected":case.expected,"status":if failure.is_some(){"blocked_execution"}else{"passed_host_execution"},"error":failure,"sessions":sessions}));
    }
    let report = json!({"schema_version":1,"backend":"portable-host","architecture":std::env::consts::ARCH,"os":std::env::consts::OS,"viewport":[640,360],"cycles":cycles,"status":if passed{"passed_required_host_cases"}else{"failed_required_host_cases"},"measurement_scope":"CPU software UI and managed VM heap only; excludes native Horizon GPU/audio and host side-table/C allocations; host timings are not Switch FPS","cases":results});
    std::fs::write(&args[2], serde_json::to_vec_pretty(&report)?)?;
    if !passed {
        return Err("required corpus case failed; inspect output JSON".into());
    }
    println!("{}", args[2]);
    Ok(())
}
