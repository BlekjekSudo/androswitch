use as_framework::App;
use as_platform::{fs::FileSystem, host::HostPlatform, host_fs::HostFileSystem};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, rc::Rc, time::Duration};
#[derive(Deserialize)]
struct Request {
    session_id: String,
    events: Vec<Event>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Event {
    Wait { ms: u64 },
    Touch { action: i32, x: f32, y: f32 },
    Key { code: i32 },
    Gc,
    Frame,
}
#[derive(Serialize)]
struct Snapshot {
    step: usize,
    frame_hash: String,
    views: Vec<as_framework::View>,
    lifecycle: Vec<as_framework::Lifecycle>,
    finished: bool,
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1)
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("usage: as-ui-probe APK REQUEST STORAGE".into());
    }
    let request: Request = serde_json::from_slice(&fs::read(&args[1])?)?;
    let storage = Rc::new(HostFileSystem::new(&args[2])?);
    let pm = as_package::PackageManager::new(storage.clone());
    let installed = pm.install(fs::read(&args[0])?, true, None)?;
    let apk = pm.load_apk(&installed.package, Some(installed.revision))?.1;
    let mut app = App::new_with_options(
        apk,
        storage as Rc<dyn FileSystem>,
        Rc::new(HostPlatform::default()),
        640,
        360,
        None,
        as_framework::AppOptions {
            source_apk: Some(format!(
                "installed/{}/{:020}/package.apk",
                installed.package, installed.revision
            )),
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())?;
    app.launch().map_err(|e| e.to_string())?;
    let mut snapshots = vec![];
    for (i, event) in std::iter::once(Event::Frame)
        .chain(request.events)
        .enumerate()
    {
        match event {
            Event::Wait { ms } => std::thread::sleep(Duration::from_millis(ms.min(30000))),
            Event::Touch { action, x, y } => app
                .android
                .touch(&mut app.vm, action, x, y)
                .map_err(|e| e.to_string())?,
            Event::Key { code } => app
                .android
                .key(&mut app.vm, code)
                .map_err(|e| e.to_string())?,
            Event::Gc => {
                app.vm.gc();
            }
            Event::Frame => {}
        }
        app.draw().map_err(|e| e.to_string())?;
        snapshots.push(Snapshot {
            step: i,
            frame_hash: format!("{:016x}", app.frame.checksum()),
            views: app.android.views(),
            lifecycle: app.android.lifecycle(),
            finished: app.android.finished(),
        });
    }
    let output = PathBuf::from(&args[2]).join("last-frame.ppm");
    let mut ppm = format!("P6\n{} {}\n255\n", app.frame.width, app.frame.height).into_bytes();
    for pixel in &app.frame.pixels {
        ppm.extend_from_slice(&[(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8]);
    }
    fs::write(output, ppm)?;
    println!(
        "{}",
        serde_json::json!({"schema_version":1,"session_id":request.session_id,"portable_vm":rd_vm::PORTABLE_CORE,"snapshots":snapshots})
    );
    Ok(())
}
