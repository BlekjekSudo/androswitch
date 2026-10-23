//! Synchronous C ABI for the product runner. Each token owns one live App.
//! Pointers stay within the C-owned loop; native callbacks use the existing VM lease.
use crate::{device_fs::LibnxFileSystem, device_jni};
use alloc::{
    boxed::Box,
    rc::Rc,
    string::{String, ToString},
    vec::Vec,
};
use as_framework::App;
use as_platform::{fs, Clock};
use serde::Deserialize;
#[allow(unsafe_code)]
extern "C" {
    fn as_platform_micros() -> u64;
}
struct HorizonClock;
impl Clock for HorizonClock {
    fn monotonic_micros(&self) -> u64 {
        #[allow(unsafe_code)]
        unsafe {
            as_platform_micros()
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Launch {
    schema_version: u32,
    package: String,
    revision: u64,
    session_id: String,
    events: Option<Vec<Event>>,
}
#[derive(Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Event {
    Wait { ms: u64 },
    Touch { action: i32, x: f32, y: f32 },
    Key { code: i32 },
    Gc,
    Frame,
}
struct Running {
    app: App,
    native: Option<Rc<device_jni::Session>>,
    fs: Rc<LibnxFileSystem>,
    request: Launch,
    cursor: usize,
    pending: Option<usize>,
    deadline: Option<u64>,
    snapshots: Vec<serde_json::Value>,
    reported: bool,
    presents: u64,
}
fn save_error(message: &str) {
    if let Ok(fs) = LibnxFileSystem::new("sdmc:/switch/androswitch") {
        let _ = fs::mkdirs(&fs, "logs");
        let data = serde_json::to_vec(
            &serde_json::json!({"schema_version":1,"status":"failed","error":message}),
        )
        .unwrap_or_default();
        let _ = fs::write_all(&fs, "logs/session-report.json", &data);
    }
}
fn open() -> Result<Running, String> {
    let fs = Rc::new(LibnxFileSystem::new("sdmc:/switch/androswitch").map_err(|e| e.to_string())?);
    let bytes =
        fs::read_all(fs.as_ref(), "runtime/launch.json", 64 << 10).map_err(|e| e.to_string())?;
    let request: Launch = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if request.schema_version != 1
        || request.session_id.is_empty()
        || request.session_id.len() > 128
        || request.events.as_ref().is_some_and(|e| e.len() > 128)
    {
        return Err("INVALID_LAUNCH_REQUEST".into());
    }
    let pm = as_package::PackageManager::new(fs.clone());
    let (record, bytes) = pm
        .load(&request.package, Some(request.revision))
        .map_err(|e| e.to_string())?;
    let apk = rd_apk::Apk::from_bytes(bytes).map_err(|e| e.to_string())?;
    let mut app = App::new(
        apk,
        fs.clone(),
        Rc::new(HorizonClock),
        640,
        360,
        record.entry_activity.as_deref(),
    )
    .map_err(|e| e.to_string())?;
    let native = if app.android.apk.native_libs.is_empty() {
        None
    } else {
        let native = device_jni::Session::new(&app.android.apk).map_err(|e| e.to_string())?;
        native.bind_vm(&mut app.vm);
        app.vm.bind_native(native.clone());
        Some(native)
    };
    device_jni::lock();
    let result = app.launch().map_err(|e| e.to_string());
    device_jni::unlock();
    result?;
    let pending = request.events.as_ref().map(|_| 0);
    Ok(Running {
        app,
        native,
        fs,
        request,
        cursor: 0,
        pending,
        deadline: None,
        snapshots: Vec::new(),
        reported: false,
        presents: 0,
    })
}
impl Running {
    fn event(&mut self, event: Event) -> Result<(), String> {
        let result = match event {
            Event::Touch { action, x, y } => self.app.android.touch(&mut self.app.vm, action, x, y),
            Event::Key { code } => self.app.android.key(&mut self.app.vm, code),
            Event::Gc => {
                self.app.vm.gc();
                Ok(())
            }
            Event::Frame | Event::Wait { .. } => Ok(()),
        };
        result.map_err(|e| e.to_string())?;
        self.app.draw().map_err(|e| e.to_string())
    }
    fn poll(&mut self) -> Result<i32, String> {
        if self.pending.is_some() {
            return Ok(0);
        }
        if let Some(events) = &self.request.events {
            if self.cursor >= events.len() {
                if !self.reported {
                    let report = serde_json::json!({"schema_version":1,"status":"passed_execution","session_id":self.request.session_id,"package":self.request.package,"revision":self.request.revision,"physical_device":"unclassified","snapshots":self.snapshots,"presented_frames":self.presents});
                    let data = serde_json::to_vec(&report).map_err(|e| e.to_string())?;
                    fs::write_all(self.fs.as_ref(), "logs/session-report.json", &data)
                        .map_err(|e| e.to_string())?;
                    self.reported = true;
                }
                return Ok(2);
            }
            let event = events[self.cursor].clone();
            if let Event::Wait { ms } = event {
                let due = *self
                    .deadline
                    .get_or_insert(self.app.android.now() + ms.min(30000));
                if self.app.android.now() < due {
                    self.app.draw().map_err(|e| e.to_string())?;
                    return Ok(0);
                }
                self.deadline = None;
            }
            self.event(event)?;
            self.cursor += 1;
            self.pending = Some(self.cursor);
        } else {
            self.app.draw().map_err(|e| e.to_string())?;
        }
        Ok(self.app.android.finished() as i32)
    }
}
/// # Safety
/// No pointers are accepted. The returned token must be used only by its owning C loop.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn as_runtime_open() -> u64 {
    match open() {
        Ok(app) => Box::into_raw(Box::new(app)) as u64,
        Err(e) => {
            save_error(&e);
            0
        }
    }
}
/// # Safety
/// `token` is a live, uniquely accessed token from as_runtime_open.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_runtime_step(token: u64) -> i32 {
    let running = unsafe { &mut *(token as *mut Running) };
    device_jni::lock();
    let result = running.poll();
    device_jni::unlock();
    match result {
        Ok(code) => code,
        Err(e) => {
            save_error(&e);
            -1
        }
    }
}
/// # Safety
/// `token` is live. The returned pixels remain valid until the next step/event.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_runtime_pixels(token: u64) -> *const u32 {
    unsafe { &*(token as *const Running) }
        .app
        .frame
        .pixels
        .as_ptr()
}
/// # Safety
/// `token` is live; called once after an actual framebufferEnd submission.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_runtime_presented(token: u64, checksum: u64) {
    let running = unsafe { &mut *(token as *mut Running) };
    running.presents += 1;
    if let Some(step) = running.pending.take() {
        running.snapshots.push(serde_json::json!({"step":step,"frame_hash":alloc::format!("{:016x}",running.app.frame.checksum()),"presented_hash":alloc::format!("{checksum:016x}"),"views":running.app.android.views(),"lifecycle":running.app.android.lifecycle(),"finished":running.app.android.finished()}));
    }
}
/// # Safety
/// `token` is live. Only the test driver's final frame requests a capture.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_runtime_capture(token: u64) -> bool {
    let r = unsafe { &*(token as *const Running) };
    r.request
        .events
        .as_ref()
        .is_some_and(|e| r.pending == Some(e.len()))
}
/// # Safety
/// `token` is live and uniquely accessed; UI events use the same Android path as E2E events.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_runtime_input(token: u64, kind: i32, a: f32, b: f32) -> i32 {
    let running = unsafe { &mut *(token as *mut Running) };
    device_jni::lock();
    let result = running.event(if kind == 4 {
        Event::Key { code: a as i32 }
    } else {
        Event::Touch {
            action: kind,
            x: a,
            y: b,
        }
    });
    device_jni::unlock();
    match result {
        Ok(()) => 0,
        Err(e) => {
            save_error(&e);
            -1
        }
    }
}
/// # Safety
/// Consume a live token once. Native sessions remain process-owned until M7 quiescent teardown.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_runtime_close(token: u64) {
    let running = unsafe { Box::from_raw(token as *mut Running) };
    device_jni::lock();
    if let Some(native) = &running.native {
        native.close_vm();
    }
    drop(running);
    device_jni::unlock();
}
