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
    fn as_worker_admission_begin_shutdown();
    fn as_platform_wall_millis(output: *mut i64) -> i32;
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
    fn wall_time_millis(&self) -> Result<i64, as_platform::PlatformError> {
        let mut value = 0;
        // SAFETY: the platform writes this live scalar only on successful
        // checked CLOCK_REALTIME conversion. Monotonic time is a separate API.
        #[allow(unsafe_code)]
        let result = unsafe { as_platform_wall_millis(&mut value) };
        if result == 0 {
            Ok(value)
        } else {
            Err(as_platform::PlatformError {
                code: "HORIZON_CLOCK_ERROR",
                operation: "wall_time_millis",
                cause: alloc::format!("CLOCK_REALTIME: {result}"),
            })
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
    gl_surface: bool,
    startup_error: Option<String>,
    runtime_error: Option<String>,
}
fn save_error(message: &str) {
    if let Ok(fs) = LibnxFileSystem::new("sdmc:/switch/androswitch") {
        let _ = fs::mkdirs(&fs, "logs");
        let pending = fs::read_all(&fs, "logs/pending-session.json", 64 << 10)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .unwrap_or(serde_json::Value::Null);
        let data = serde_json::to_vec(&crate::session_report::failure(&pending, message))
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
fn available_memory() -> usize {
    #[allow(unsafe_code)]
    extern "C" {
        fn as_host_memory_available() -> u64;
    }
    // SAFETY: scalar aggregate allocator snapshot, no pointers retained.
    #[allow(unsafe_code)]
    unsafe {
        as_host_memory_available() as usize
    }
}
fn memory_stage(stage: &'static str) {
    #[allow(unsafe_code)]
    extern "C" {
        fn as_memory_stage(stage: *const core::ffi::c_char);
    }
    if let Ok(name) = alloc::ffi::CString::new(stage) {
        // SAFETY: the live NUL-terminated label is consumed synchronously.
        #[allow(unsafe_code)]
        unsafe {
            as_memory_stage(name.as_ptr());
        }
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
        return Err("APK_CONTAINER_BUDGET: file-backed APK exceeds 512 MiB".into());
    }
    let (record, apk) = pm
        .load_apk(&request.package, Some(request.revision))
        .map_err(|e| e.to_string())?;
    let load_budget = available_memory();
    let write_memory = |details: serde_json::Value| {
        if let Ok(bytes) = serde_json::to_vec(&details) {
            let _ = fs::write_all(fs.as_ref(), "logs/memory-admission.json", &bytes);
        }
    };
    write_memory(
        serde_json::json!({"schema_version":1,"status":"loading","session_id":request.session_id,"package":record.package,"revision":record.revision,"allocator_available_before_load":load_budget}),
    );
    let mut app = App::new_with_options(
        apk,
        fs.clone(),
        Rc::new(HorizonClock),
        640,
        360,
        record.entry_activity.as_deref(),
        as_framework::AppOptions {
            source_apk: Some(alloc::format!(
                "installed/{}/{:020}/package.apk",
                record.package,
                record.revision
            )),
            memory: as_framework::memory::LoadMemory {
                budget: load_budget,
                available: Some(available_memory),
                checkpoint: Some(memory_stage),
            },
        },
    )
    .map_err(|e| {
        let cause = e.to_string();
        write_memory(serde_json::json!({"schema_version":1,"status":"failed","session_id":request.session_id,"package":record.package,"revision":record.revision,"allocator_available_before_load":load_budget,"error":cause}));
        cause
    })?;
    write_memory(
        serde_json::json!({"schema_version":1,"status":"vm_ready","session_id":request.session_id,"package":record.package,"revision":record.revision,"allocator_available_before_load":load_budget,"raw_dex_bytes":app.memory.raw_dex_bytes,"estimated_parser_bytes":app.memory.estimated_parser_bytes,"parser_budget_bytes":app.memory.parser_budget_bytes,"allocator_available_after_load":available_memory(),"scope":"parser estimate is admission accounting, not an allocator measurement; memory.log contains allocator snapshots"}),
    );
    app.android.bind_audio(Rc::new(HorizonAudio));
    crate::device_resources::bind(&app.android);
    crate::device_graphics::bind(&app.android).map_err(|e| e.to_string())?;
    // Java-only applications can own background VM callbacks too. Every device
    // application therefore has a real JNI/thread session, even before loading
    // an ELF. Retain it in Running before a worker can be started or fail.
    let native = device_jni::Session::new(&app.android.apk).map_err(|e| e.to_string())?;
    native.bind_vm(&mut app.vm);
    app.vm.bind_native(native.clone());
    let native = Some(native);
    let pending = request.events.as_ref().map(|_| 0);
    let mut running = Running {
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
        native_activity: 0,
        gl_surface: false,
        startup_error: None,
        runtime_error: None,
    };
    let started = running.start();
    running.record_native_compatibility();
    if let Err(error) = started {
        save_error(&error);
        running.startup_error = Some(error);
    }
    // Even failed initialization returns an owner. C runs bounded teardown
    // before returning to the console; VM/JNI roots cannot dangle on an error.
    Ok(running)
}
impl Running {
    fn start(&mut self) -> Result<(), String> {
        device_jni::lock();
        let result = self.start_locked();
        device_jni::unlock();
        result
    }
    // onCreate can start a Timer immediately. Keep every subsequent VM and
    // framework access under the same lease used by that actual worker.
    fn start_locked(&mut self) -> Result<(), String> {
        crate::device_workers::bind(
            &mut self.app,
            self.native.as_ref().ok_or("JNI_SESSION_MISSING")?,
        )
        .map_err(|e| e.to_string())?;
        self.app.launch().map_err(|e| e.to_string())?;
        let app = &mut self.app;
        if app.vm.cp.is_subtype(
            &alloc::format!("L{};", app.entry.replace('.', "/")),
            "Landroid/app/NativeActivity;",
        ) {
            let session = self
                .native
                .as_ref()
                .ok_or("NATIVE_ACTIVITY_LIBRARY_MISSING")?;
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
                // Retain ownership before entering guest code, including failed onCreate.
                self.native_activity = activity;
                session
                    .native_call(&mut app.vm, function, &[activity, 0, 0])
                    .map_err(|e| e.to_string())?;
                Ok(activity)
            })();
            created?;
        }
        let requests = self.app.android.gl_requests().map_err(|e| e.to_string())?;
        if !requests.is_empty() {
            if self.native_activity != 0 {
                return Err("GL_COMPOSITION_UNAVAILABLE: NativeActivity and GLSurfaceView share one display".into());
            }
            self.gl_surface = crate::device_graphics::start(
                &mut self.app,
                self.native.as_ref().ok_or("JNI_SESSION_MISSING")?,
            )?;
        }
        Ok(())
    }
    fn record_native_compatibility(&self) {
        device_jni::lock();
        let deferred = self
            .native
            .as_ref()
            .map(|native| native.deferred_imports())
            .unwrap_or_default();
        device_jni::unlock();
        let report = serde_json::json!({"schema_version":1,"session_id":self.request.session_id,
            "package":self.request.package,"revision":self.request.revision,
            "binding_policy":"unimplemented strong function imports trap with library/symbol on first invocation",
            "deferred_imports":deferred,
            "scope":"listed functions remain unimplemented; mapping an APK does not certify their API behavior"});
        if let Ok(bytes) = serde_json::to_vec(&report) {
            let _ = fs::write_all(self.fs.as_ref(), "logs/native-compatibility.json", &bytes);
        }
    }
    /// Caller holds the VM lease after every callback/guest worker has joined.
    /// Retain handler failures separately; they must not replace the original
    /// application failure or inspect a still-running worker's VM state.
    fn record_uncaught_exceptions(&self) {
        let reports = self.app.android.uncaught_exception_reports();
        if reports.is_empty() {
            return;
        }
        let rows: Vec<_> = reports
            .into_iter()
            .map(|report| {
                serde_json::json!({"serial":report.serial,"origin":report.origin,
                    "thread":report.thread,"handler":report.handler,
                    "original":{"class":report.original.class,"message":report.original.message,
                        "object":report.original.obj},
                    "handler_failure":report.handler_failure.map(|error|error.to_string()),
                    "via_thread_group":report.via_thread_group,
                    "handler_invoked":report.handler_invoked})
            })
            .collect();
        let value = serde_json::json!({"schema_version":1,
            "session_id":self.request.session_id,"package":self.request.package,
            "revision":self.request.revision,"reports":rows,
            "scope":"bounded uncaught Java diagnostics after quiescence; object IDs belong only to this session; original worker-stop policy retained"});
        if let Ok(bytes) = serde_json::to_vec(&value) {
            let _ = fs::write_all(self.fs.as_ref(), "logs/uncaught-exceptions.json", &bytes);
        }
    }
    fn cleanup_error(&mut self, message: &str) {
        let combined = crate::session_report::cleanup_failure(
            self.startup_error
                .as_deref()
                .or(self.runtime_error.as_deref()),
            message,
        );
        if self.startup_error.is_none() && self.runtime_error.is_none() {
            self.runtime_error = Some(message.to_string());
        }
        save_error(&combined);
    }
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
            Event::Touch { action, x, y } => self.app.android.touch(
                &mut self.app.vm,
                action,
                if self.gl_surface { x * 2. } else { x },
                if self.gl_surface { y * 2. } else { y },
            ),
            Event::Key { code } => self.app.android.key(&mut self.app.vm, code),
            Event::Gc => {
                self.app.vm.gc();
                Ok(())
            }
            Event::Frame | Event::Wait { .. } | Event::ReturnToLauncher => Ok(()),
        };
        result.map_err(|e| e.to_string())?;
        self.draw()
    }
    fn draw(&mut self) -> Result<(), String> {
        if self.gl_surface {
            self.app.android.tick(&mut self.app.vm)
        } else {
            self.app.draw()
        }
        .map_err(|e| e.to_string())
    }
    fn poll(&mut self) -> Result<i32, String> {
        if let Some(error) = &self.runtime_error {
            return Err(error.clone());
        }
        if let Some(error) = self
            .native
            .as_ref()
            .and_then(|native| native.runtime_error())
        {
            return Err(error);
        }
        // SAFETY: reads atomic media counters; guest activity lifetime is owned here.
        #[allow(unsafe_code)]
        unsafe {
            if (self.native_activity != 0 || self.gl_surface) && as_media_error() != 0 {
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
                    self.draw()?;
                    return Ok(0);
                }
                self.deadline = None;
            }
            self.event(event)?;
            self.cursor += 1;
            self.pending = Some(self.cursor);
        } else {
            self.draw()?;
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
/// Token is live and uniquely owned by the C loop. A failed startup must be
/// closed through the same watchdog/quiescence boundary as a running app.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_runtime_failed(token: u64) -> bool {
    unsafe { &*(token as *const Running) }
        .startup_error
        .is_some()
}
/// # Safety
/// `token` is a live, uniquely accessed token from as_runtime_open.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_runtime_step(token: u64) -> i32 {
    let running = unsafe { &mut *(token as *mut Running) };
    device_jni::lock();
    let result = running.poll();
    if let Err(error) = &result {
        if running.runtime_error.is_none() {
            running.runtime_error = Some(error.clone());
        }
    }
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
#[repr(C)]
pub struct CpuFrame {
    pixels: *const u32,
    length: usize,
    width: u32,
    height: u32,
}
/// # Safety
/// Token and output are live, accessed by the owning presentation loop. The
/// returned slice metadata remains valid until the next runtime step/event.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_runtime_frame(token: u64, output: *mut CpuFrame) -> bool {
    if output.is_null() {
        return false;
    }
    let frame = &unsafe { &*(token as *const Running) }.app.frame;
    let (Ok(width), Ok(height)) = (u32::try_from(frame.width), u32::try_from(frame.height)) else {
        return false;
    };
    // SAFETY: caller supplies writable scalar metadata, with no ownership
    // transfer of the retained Rust raster allocation.
    unsafe {
        *output = CpuFrame {
            pixels: frame.pixels.as_ptr(),
            length: frame.pixels.len(),
            width,
            height,
        };
    }
    true
}
/// # Safety
/// Token is live, accessed only by its owning platform loop.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_runtime_native(token: u64) -> u64 {
    unsafe { &*(token as *const Running) }.native_activity
}
/// # Safety
/// `token` is live; C reports a CPU submission or its observed GPU frame count.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_runtime_presented(token: u64, checksum: u64) {
    device_jni::lock();
    let running = unsafe { &mut *(token as *mut Running) };
    if running.native_activity != 0 || running.gl_surface {
        running.presents = checksum;
    } else {
        running.presents += 1;
    }
    if let Some(step) = running.pending.take() {
        running.snapshots.push(serde_json::json!({"step":step,"frame_hash":alloc::format!("{:016x}",running.app.frame.checksum()),"presented_hash":alloc::format!("{checksum:016x}"),"views":running.app.android.views(),"lifecycle":running.app.android.lifecycle(),"finished":running.app.android.finished(),"native_frames":unsafe {as_media_frames()},"audio_frames":unsafe{as_media_audio_frames()}}));
    }
    device_jni::unlock();
}
/// # Safety
/// `token` is live. The test driver's explicit Frame events and final frame
/// request a capture. Interactive sessions never request one here.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_runtime_capture(token: u64) -> bool {
    let r = unsafe { &*(token as *const Running) };
    r.request.events.as_ref().is_some_and(|events| {
        r.pending.is_some_and(|step| {
            step == events.len()
                || step
                    .checked_sub(1)
                    .and_then(|index| events.get(index))
                    .is_some_and(|event| matches!(event, Event::Frame))
        })
    })
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
    if let Err(error) = &result {
        if running.runtime_error.is_none() {
            running.runtime_error = Some(error.clone());
        }
    }
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
            running.cleanup_error(&alloc::format!("NATIVE_STOP_FAILED: {rc}"));
            core::mem::forget(running);
            return -1;
        }
    }
    // Java lifecycle may invoke native code or create threads too. Deliver it
    // before the final quiescence check, then release the VM lease while joining.
    phase(if running.startup_error.is_some() {
        c"java_startup_abort"
    } else {
        c"java_activity_stop"
    });
    device_jni::lock();
    let stopped = if running.startup_error.is_some() {
        running.app.android.abort_startup()
    } else {
        running.app.android.stop(&mut running.app.vm)
    };
    device_jni::unlock();
    if let Err(error) = stopped {
        running.cleanup_error(&alloc::format!("ACTIVITY_STOP_ERROR: {error}"));
    }
    // Close every actual platform/guest worker creation path atomically with
    // its registration. Destructors may still call Java/native services, but
    // cannot create a new callback owner behind an already completed join.
    phase(c"worker_admission_closed");
    unsafe { as_worker_admission_begin_shutdown() };
    phase(c"background_worker_quiesce");
    let background_workers = crate::device_workers::quiesce(6_000_000_000);
    if background_workers != 0 {
        running.cleanup_error(&alloc::format!(
            "BACKGROUND_WORKER_NOT_QUIESCENT: {background_workers}; reopen homebrew"
        ));
        core::mem::forget(running);
        return -1;
    }
    phase(c"media_thread_quiesce");
    let media_threads = unsafe { as_media_quiesce() };
    if media_threads != 0 {
        running.cleanup_error("MEDIA_CALLBACK_NOT_QUIESCENT: reopen homebrew");
        core::mem::forget(running);
        return -1;
    }
    phase(c"guest_thread_quiesce");
    let rc = unsafe { as_native_quiesce(2_000_000_000) };
    if rc != 0 {
        running.cleanup_error(&alloc::format!(
            "NATIVE_NOT_QUIESCENT: {rc}; reopen homebrew"
        ));
        core::mem::forget(running);
        return -1;
    }
    // Execution errors reported by now-joined workers are application errors,
    // not failures of a destructor that has not started. Acknowledge them before
    // finalization so only a newly raised callback failure stops that phase.
    device_jni::lock();
    let worker_error = running
        .native
        .as_ref()
        .and_then(|native| native.take_runtime_error());
    device_jni::unlock();
    if let Some(error) = worker_error {
        if running.startup_error.is_none() && running.runtime_error.is_none() {
            save_error(&error);
            running.runtime_error = Some(error);
        }
    }
    // Static/thread-local C++ destructors may call Java, files, or media.
    // Execute them while both the VM session and all mapped code are alive.
    let mut finalization_settled = false;
    for _ in 0..64 {
        phase(c"native_destructors");
        device_jni::lock();
        let finalized = if let Some(native) = &running.native {
            native.finalize_libraries(&mut running.app.vm)
        } else {
            Ok(())
        };
        device_jni::unlock();
        if let Err(finalized) = finalized {
            running.cleanup_error(&alloc::format!("NATIVE_FINALIZE_FAILED: {finalized}"));
            core::mem::forget(running);
            return -1;
        }
        // A callback may create another guest/platform worker. Prove
        // quiescence again before inspecting its newly registered callbacks.
        let background_workers = crate::device_workers::quiesce(6_000_000_000);
        let media_threads = unsafe { as_media_quiesce() };
        let native_threads = unsafe { as_native_quiesce(2_000_000_000) };
        if background_workers != 0 || media_threads != 0 || native_threads != 0 {
            running.cleanup_error("DESTRUCTOR_NOT_QUIESCENT: reopen homebrew");
            core::mem::forget(running);
            return -1;
        }
        device_jni::lock();
        let finalizer_worker_error = running
            .native
            .as_ref()
            .and_then(|native| native.runtime_error());
        if let Some(error) = finalizer_worker_error {
            device_jni::unlock();
            running.cleanup_error(&alloc::format!("NATIVE_FINALIZE_FAILED: {error}"));
            core::mem::forget(running);
            return -1;
        }
        let pending = running
            .native
            .as_ref()
            .is_some_and(|native| native.finalization_pending());
        device_jni::unlock();
        if !pending {
            finalization_settled = true;
            break;
        }
        phase(c"native_destructors_pending");
    }
    if !finalization_settled {
        running.cleanup_error("NATIVE_FINALIZE_LIMIT: callbacks did not settle; reopen homebrew");
        core::mem::forget(running);
        return -1;
    }
    device_jni::lock();
    let media = unsafe { as_media_reset() };
    if media != 0 {
        device_jni::unlock();
        running.cleanup_error("MEDIA_NOT_QUIESCENT: reopen homebrew");
        core::mem::forget(running);
        return -1;
    }
    phase(c"activity_dispose");
    unsafe { as_activity_dispose(running.native_activity) };
    running.record_uncaught_exceptions();
    if let Some(native) = &running.native {
        native.close_vm();
        native.release_root();
    }
    // Ordinary interactive sessions also need a report; otherwise the shell
    // can display a stale result from a previous APK. Preserve any named error.
    let fs = running.fs.clone();
    let primary_error = running
        .startup_error
        .clone()
        .or(running.runtime_error.clone());
    let final_report = if running.request.events.is_none() {
        fs::read_all(fs.as_ref(), "logs/session-report.json", 1 << 20)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .filter(|report| {
                let expected = serde_json::json!({"schema_version":1,"session_id":running.request.session_id,"package":running.request.package,"revision":running.request.revision});
                report["status"] == "running" && crate::session_report::identity_matches(report, &expected)
            })
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
    running.record_native_compatibility();
    phase(c"runtime_drop");
    drop(running);
    device_jni::unlock();
    let rc = unsafe { as_native_reset() };
    if rc != 0 {
        let message = alloc::format!("NATIVE_RESOURCE_RESET_FAILED: {rc}; reopen homebrew");
        save_error(&crate::session_report::cleanup_failure(
            primary_error.as_deref(),
            &message,
        ));
    } else if let Some(bytes) = final_report {
        let _ = fs::write_all(fs.as_ref(), "logs/session-report.json", &bytes);
    }
    rc
}
