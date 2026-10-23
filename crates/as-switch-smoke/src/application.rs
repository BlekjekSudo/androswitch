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
use as_platform::{
    fs::{self, FileSystem},
    Clock,
};
use serde::{Deserialize, Serialize};
#[allow(unsafe_code)]
extern "C" {
    fn as_platform_micros() -> u64;
    fn as_activity_create(package: *const core::ffi::c_char, object: u64) -> u64;
    fn as_activity_stop(activity: u64) -> i32;
    fn as_activity_dispose(activity: u64);
    fn as_media_input(kind: i32, code: i32, x: f32, y: f32);
    fn as_media_reset() -> i32;
    fn as_media_quiesce() -> i32;
    fn as_media_finished() -> i32;
    fn as_media_error() -> i32;
    fn as_media_capture(enabled: i32);
    fn as_media_frames() -> u64;
    fn as_media_audio_frames() -> u64;
    fn as_native_quiesce(timeout: u64) -> i32;
    fn as_native_reset() -> i32;
    fn as_teardown_phase(phase: *const core::ffi::c_char);
    fn as_audio_open(rate: i32, channels: i32, format: i32) -> i64;
    fn as_audio_start(id: u64) -> i32;
    fn as_audio_write(id: u64, data: *const u8, frames: usize, timeout: i64) -> i64;
    fn as_audio_stop(id: u64) -> i32;
    fn as_audio_close(id: u64) -> i32;
}
struct HorizonClock;
struct HorizonAudio;
fn audio_error(op: &str, rc: i64) -> rd_vm::VmExit {
    rd_vm::err::vm_error("HORIZON_AUDIO_ERROR", alloc::format!("{op}: {rc:#x}")).into()
}
impl as_framework::PcmOutput for HorizonAudio {
    fn open(&self, rate: i32, channels: i32, format: i32) -> Result<u64, rd_vm::VmExit> {
        // SAFETY: scalar format parameters; C owns the audio output and copies writes.
        #[allow(unsafe_code)]
        let rc = unsafe { as_audio_open(rate, channels, format) };
        if rc > 0 {
            Ok(rc as u64)
        } else {
            Err(audio_error("open", rc))
        }
    }
    fn start(&self, id: u64) -> Result<(), rd_vm::VmExit> {
        #[allow(unsafe_code)]
        let rc = unsafe { as_audio_start(id) };
        if rc == 0 {
            Ok(())
        } else {
            Err(audio_error("start", rc as i64))
        }
    }
    fn write(&self, id: u64, pcm: &[u8]) -> Result<usize, rd_vm::VmExit> {
        // SAFETY: C copies the bounded PCM slice before retaining any submitted memory.
        #[allow(unsafe_code)]
        let rc = unsafe { as_audio_write(id, pcm.as_ptr(), pcm.len() / 4, 1_000_000_000) };
        if rc >= 0 {
            Ok(rc as usize)
        } else {
            Err(audio_error("write", rc))
        }
    }
    fn stop(&self, id: u64) -> Result<(), rd_vm::VmExit> {
        #[allow(unsafe_code)]
        let rc = unsafe { as_audio_stop(id) };
        if rc == 0 {
            Ok(())
        } else {
            Err(audio_error("stop", rc as i64))
        }
    }
    fn close(&self, id: u64) -> Result<(), rd_vm::VmExit> {
        #[allow(unsafe_code)]
        let rc = unsafe { as_audio_close(id) };
        if rc == 0 {
            Ok(())
        } else {
            Err(audio_error("close", rc as i64))
        }
    }
}
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
#[derive(Deserialize, Serialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Event {
    Wait { ms: u64 },
    Touch { action: i32, x: f32, y: f32 },
    Key { code: i32 },
    Gc,
    Frame,
    ReturnToLauncher,
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
    native_activity: u64,
}
fn save_error(message: &str) {
    if let Ok(fs) = LibnxFileSystem::new("sdmc:/switch/androswitch") {
        let _ = fs::mkdirs(&fs, "logs");
        let pending = fs::read_all(&fs, "logs/pending-session.json", 64 << 10)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .unwrap_or(serde_json::Value::Null);
        let data = serde_json::to_vec(
            &serde_json::json!({"schema_version":1,"status":"failed","error":message,"session_id":pending["session_id"],"package":pending["package"],"revision":pending["revision"]}),
        )
        .unwrap_or_default();
        let _ = fs::write_all(&fs, "logs/session-report.json", &data);
    }
}
fn phase(name: &core::ffi::CStr) {
    // SAFETY: static NUL-terminated phase strings are copied synchronously by C.
    #[allow(unsafe_code)]
    unsafe {
        as_teardown_phase(name.as_ptr());
    }
}
fn open() -> Result<Running, String> {
    let fs = Rc::new(LibnxFileSystem::new("sdmc:/switch/androswitch").map_err(|e| e.to_string())?);
    let bytes = fs::read_all(fs.as_ref(), "runtime/launch.json", 64 << 10);
    // Requests are one-shot, including malformed requests and native crashes.
    // A subsequent boot always reaches the console and its interruption report.
    fs.remove("runtime/launch.json", false)
        .map_err(|e| e.to_string())?;
    let bytes = bytes.map_err(|e| e.to_string())?;
    let request: Launch = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    // GPU dumps are opt-in for scripted verification, never ordinary launches.
    #[allow(unsafe_code)]
    unsafe {
        as_media_capture(i32::from(request.events.is_some()));
    }
    if request.schema_version != 1
        || request.session_id.is_empty()
        || request.session_id.len() > 128
        || request.events.as_ref().is_some_and(|e| e.len() > 128)
    {
        return Err("INVALID_LAUNCH_REQUEST".into());
    }
    let pm = as_package::PackageManager::new(fs.clone());
    let current = pm
        .latest(&request.package)
        .map_err(|e| e.to_string())?
        .ok_or("PACKAGE_NOT_INSTALLED")?;
    if current.apk_bytes > as_launcher::APK_MEMORY_LIMIT as u64 {
        return Err("APK_MEMORY_BUDGET: this alpha supports memory-resident APKs <=64 MiB; OBB files stream separately".into());
    }
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
    app.android.bind_audio(Rc::new(HorizonAudio));
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
    let mut native_activity = 0;
    if app.vm.cp.is_subtype(
        &alloc::format!("L{};", app.entry.replace('.', "/")),
        "Landroid/app/NativeActivity;",
    ) {
        let session = native.as_ref().ok_or("NATIVE_ACTIVITY_LIBRARY_MISSING")?;
        let component = app
            .android
            .apk
            .manifest
            .application
            .components
            .iter()
            .find(|c| c.class_name == app.entry)
            .ok_or("ACTIVITY_NOT_FOUND")?;
        let metadata = |key: &str, default: &str| {
            component
                .meta_data
                .iter()
                .find(|m| m.name.as_deref() == Some(key))
                .and_then(|m| m.value.clone())
                .unwrap_or_else(|| default.into())
        };
        let library = metadata("android.app.lib_name", "main");
        let symbol = metadata("android.app.func_name", "ANativeActivity_onCreate");
        device_jni::lock();
        let created = (|| -> Result<u64, String> {
            let function = session
                .native_entry(&mut app.vm, &library, &symbol)
                .map_err(|e| e.to_string())?;
            let object = session
                .activity_reference(app.android.activity_object().ok_or("ACTIVITY_STATE")?)
                .map_err(|e| e.to_string())?;
            let package = alloc::ffi::CString::new(app.android.apk.manifest.package.as_str())
                .map_err(|e| e.to_string())?;
            // SAFETY: C copies package path and retains a real global JNI activity reference.
            #[allow(unsafe_code)]
            let activity = unsafe { as_activity_create(package.as_ptr(), object) };
            if activity == 0 {
                return Err("NATIVE_ACTIVITY_ALLOCATION".into());
            }
            session
                .native_call(&mut app.vm, function, &[activity, 0, 0])
                .map_err(|e| e.to_string())?;
            Ok(activity)
        })();
        device_jni::unlock();
        native_activity = created?;
    }
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
        native_activity,
    })
}
impl Running {
    fn event(&mut self, event: Event) -> Result<(), String> {
        if self.native_activity != 0 {
            // SAFETY: scalars are copied into the synchronized NDK input queue.
            #[allow(unsafe_code)]
            unsafe {
                match event {
                    Event::Touch { action, x, y } => as_media_input(action, 0, x * 2., y * 2.),
                    Event::Key { code } => as_media_input(4, code, 0., 0.),
                    _ => {}
                }
            }
            return Ok(());
        }
        let result = match event {
            Event::Touch { action, x, y } => self.app.android.touch(&mut self.app.vm, action, x, y),
            Event::Key { code } => self.app.android.key(&mut self.app.vm, code),
            Event::Gc => {
                self.app.vm.gc();
                Ok(())
            }
            Event::Frame | Event::Wait { .. } | Event::ReturnToLauncher => Ok(()),
        };
        result.map_err(|e| e.to_string())?;
        self.app.draw().map_err(|e| e.to_string())
    }
    fn poll(&mut self) -> Result<i32, String> {
        // SAFETY: reads atomic media counters; guest activity lifetime is owned here.
        #[allow(unsafe_code)]
        unsafe {
            if self.native_activity != 0 && as_media_error() != 0 {
                return Err(alloc::format!("NATIVE_MEDIA_ERROR: {}", as_media_error()));
            }
        }
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
                return Ok(if matches!(events.last(), Some(Event::ReturnToLauncher)) {
                    3
                } else {
                    2
                });
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
        #[allow(unsafe_code)]
        let native_finished = unsafe { self.native_activity != 0 && as_media_finished() != 0 };
        Ok((self.app.android.finished() || native_finished) as i32)
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
/// Token is live, accessed only by its owning platform loop.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_runtime_native(token: u64) -> u64 {
    unsafe { &*(token as *const Running) }.native_activity
}
/// # Safety
/// `token` is live; called once after an actual framebufferEnd submission.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_runtime_presented(token: u64, checksum: u64) {
    let running = unsafe { &mut *(token as *mut Running) };
    running.presents += 1;
    if let Some(step) = running.pending.take() {
        running.snapshots.push(serde_json::json!({"step":step,"frame_hash":alloc::format!("{:016x}",running.app.frame.checksum()),"presented_hash":alloc::format!("{checksum:016x}"),"views":running.app.android.views(),"lifecycle":running.app.android.lifecycle(),"finished":running.app.android.finished(),"native_frames":unsafe {as_media_frames()},"audio_frames":unsafe{as_media_audio_frames()}}));
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
/// Consume a live token once. Failure requires the caller to exit the Horizon
/// process; another application must never start after unproved quiescence.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_runtime_close(token: u64) -> i32 {
    let mut running = unsafe { Box::from_raw(token as *mut Running) };
    phase(c"native_activity_stop");
    if running.native_activity != 0 {
        let rc = unsafe { as_activity_stop(running.native_activity) };
        if rc != 0 {
            save_error(&alloc::format!("NATIVE_STOP_FAILED: {rc}"));
            core::mem::forget(running);
            return -1;
        }
    }
    // Java lifecycle may invoke native code or create threads too. Deliver it
    // before the final quiescence check, then release the VM lease while joining.
    phase(c"java_activity_stop");
    device_jni::lock();
    let stopped = running.app.android.stop(&mut running.app.vm);
    device_jni::unlock();
    if let Err(error) = stopped {
        save_error(&alloc::format!("ACTIVITY_STOP_ERROR: {error}"));
    }
    phase(c"media_thread_quiesce");
    let media_threads = unsafe { as_media_quiesce() };
    if media_threads != 0 {
        save_error("MEDIA_CALLBACK_NOT_QUIESCENT: reopen homebrew");
        core::mem::forget(running);
        return -1;
    }
    phase(c"guest_thread_quiesce");
    let rc = unsafe { as_native_quiesce(2_000_000_000) };
    if rc != 0 {
        save_error(&alloc::format!(
            "NATIVE_NOT_QUIESCENT: {rc}; reopen homebrew"
        ));
        core::mem::forget(running);
        return -1;
    }
    device_jni::lock();
    let media = unsafe { as_media_reset() };
    if media != 0 {
        device_jni::unlock();
        save_error("MEDIA_NOT_QUIESCENT: reopen homebrew");
        core::mem::forget(running);
        return -1;
    }
    phase(c"activity_dispose");
    unsafe { as_activity_dispose(running.native_activity) };
    if let Some(native) = &running.native {
        native.close_vm();
        native.release_root();
    }
    // Ordinary interactive sessions also need a report; otherwise the shell
    // can display a stale result from a previous APK. Preserve any named error.
    let fs = running.fs.clone();
    let final_report = if running.request.events.is_none() {
        fs::read_all(fs.as_ref(), "logs/session-report.json", 1 << 20)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .filter(|report| report["status"] == "running")
            .and_then(|mut report| {
                report["status"] = "stopping".into();
                let bytes = serde_json::to_vec(&report).ok()?;
                let _ = fs::write_all(fs.as_ref(), "logs/session-report.json", &bytes);
                report["status"] = "stopped".into();
                serde_json::to_vec(&report).ok()
            })
    } else {
        None
    };
    phase(c"runtime_drop");
    drop(running);
    device_jni::unlock();
    let rc = unsafe { as_native_reset() };
    if rc != 0 {
        save_error(&alloc::format!(
            "NATIVE_RESOURCE_RESET_FAILED: {rc}; reopen homebrew"
        ));
    } else if let Some(bytes) = final_report {
        let _ = fs::write_all(fs.as_ref(), "logs/session-report.json", &bytes);
    }
    rc
}
