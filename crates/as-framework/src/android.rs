//! A portable adaptation of RustyDroid's Activity/View/Looper model.
//! The shared APK/AXML/ARSC parsers and real DEX methods remain authoritative.
use crate::Frame;
use alloc::{
    collections::{BTreeMap, VecDeque},
    format,
    rc::Rc,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use as_platform::Clock;
use core::cell::RefCell;
use rd_apk::{
    arsc::ResolvedValue,
    axml::{AttrValue, ComplexUnit, XmlElement},
    Apk,
};
use rd_vm::{framework_host::FrameworkHost, Engine, ObjRef, Throwable, Value, VmExit};
#[path = "services/mod.rs"]
mod services;
use serde::Serialize;
pub use services::executors::{MainExceptionScope, UncaughtExceptionReport};
pub use services::glsurface::{
    EglConfigBackend, EglConfigs, GlEvent, GlSurfaceRequest, GraphicsCaps,
};
type Result<T> = core::result::Result<T, VmExit>;
fn error(code: &str, cause: impl Into<String>) -> VmExit {
    rd_vm::err::vm_error(code, cause).into()
}
fn write(vm: &mut Engine, r: ObjRef, name: &str, value: Value) -> Result<()> {
    vm.heap.put_field(r, name, value).map_err(|e| match e {
        rd_vm::heap::FieldWriteErr::Oom(e) => e.into(),
        rd_vm::heap::FieldWriteErr::NotInstance => error("INVALID_OBJECT", "field receiver"),
    })
}
fn obj(v: &Value) -> Result<ObjRef> {
    v.as_ref()?.ok_or_else(|| {
        Throwable::new("Ljava/lang/NullPointerException;", "framework argument").into()
    })
}
fn int(a: &[Value], i: usize) -> Result<i32> {
    Ok(a.get(i)
        .ok_or_else(|| error("INVALID_ARGUMENT", "missing integer"))?
        .as_int()?)
}
fn text(vm: &Engine, v: &Value) -> Result<String> {
    Ok(vm.heap.as_str(obj(v)?)?.to_string())
}
fn string(vm: &mut Engine, s: impl Into<String>) -> Result<Value> {
    Ok(Value::Obj(vm.heap.alloc_string(s.into())?))
}
// Dalvik registers contain untyped bits. Host services receive the types in
// the actual method descriptor, including zero-as-null and float constants.
fn typed_args(sig: &str, args: &[Value]) -> Result<Vec<Value>> {
    let parameters =
        rd_vm::engine::parse_param_types(sig).ok_or_else(|| error("INVALID_DESCRIPTOR", sig))?;
    if parameters.len() != args.len() {
        return Err(error("INVALID_ARGUMENT", sig));
    }
    parameters
        .iter()
        .zip(args)
        .map(|(d, v)| {
            Ok(match d.as_str() {
                d if d.starts_with(['L', '[']) && matches!(v, Value::Int(0) | Value::Null) => {
                    Value::Null
                }
                "F" => Value::Float(v.as_float()?),
                "D" => Value::Double(v.as_double()?),
                "Z" | "B" | "C" | "S" | "I" => Value::Int(rd_vm::value::narrow_i32(v.as_int()?, d)),
                "J" => Value::Long(v.as_long()?),
                _ => v.clone(),
            })
        })
        .collect()
}
fn shell(vm: &mut Engine, class: &str) -> Result<ObjRef> {
    vm.ensure_initialized(class)?;
    let r = vm.heap.alloc_instance(class.into(), vec![])?;
    vm.attach_host_state(class, r)?;
    Ok(r)
}
fn descriptor(name: &str) -> String {
    if name.starts_with('L') && name.ends_with(';') {
        name.into()
    } else {
        format!("L{};", name.replace('.', "/"))
    }
}
#[derive(Clone, Serialize)]
pub struct View {
    pub object: ObjRef,
    pub class: String,
    pub id: i32,
    pub text: String,
    pub bounds: [i32; 4],
    pub children: Vec<ObjRef>,
    pub visible: bool,
    pub enabled: bool,
    pub clickable: bool,
    pub color: u32,
    pub text_color: u32,
    pub text_size: f32,
    pub width: i32,
    pub height: i32,
    pub orientation: i32,
    pub listener: Option<ObjRef>,
    pub touch_listener: Option<ObjRef>,
    pub click_method: Option<String>,
    pub input_type: Option<i32>,
    /// The actual Context passed to the supported View constructor.
    #[serde(skip)]
    pub context: Option<ObjRef>,
}
impl View {
    fn new(object: ObjRef, class: &str) -> Self {
        Self {
            object,
            class: class.into(),
            id: -1,
            text: String::new(),
            bounds: [0; 4],
            children: vec![],
            visible: true,
            enabled: true,
            clickable: class == "Landroid/widget/Button;",
            color: if class == "Landroid/widget/Button;" {
                0xff305c89
            } else {
                0
            },
            text_color: 0xffeff5fa,
            text_size: 18.,
            width: -2,
            height: -2,
            orientation: 0,
            listener: None,
            touch_listener: None,
            click_method: None,
            input_type: (class == "Landroid/widget/EditText;").then_some(1),
            context: None,
        }
    }
}
#[derive(Clone, Serialize)]
pub struct Lifecycle {
    pub activity: String,
    pub event: String,
    pub time_ms: u64,
}
#[derive(Clone)]
struct Activity {
    object: ObjRef,
    class: String,
    root: Option<ObjRef>,
    intent: Option<ObjRef>,
    saved_state: Option<ObjRef>,
}
#[derive(Clone)]
enum Action {
    Start(String, ObjRef),
    Finish(ObjRef),
}
#[derive(Clone)]
struct Task {
    due: u64,
    order: u64,
    handler: ObjRef,
    runnable: ObjRef,
}
#[derive(Clone)]
enum Draw {
    Bitmap(services::bitmap::BitmapDraw),
    Color(u32),
    Rect([f32; 4], u32),
    Circle(f32, f32, f32, u32),
    Text(String, f32, f32, f32, u32),
}
#[derive(Clone)]
struct BundleEntry {
    kind: &'static str,
    value: Value,
}
#[derive(Default)]
struct State {
    views: BTreeMap<ObjRef, View>,
    activities: Vec<Activity>,
    application: Option<ObjRef>,
    actions: VecDeque<Action>,
    tasks: Vec<Task>,
    serial: u64,
    roots: Vec<ObjRef>,
    intents: BTreeMap<ObjRef, String>,
    bundles: BTreeMap<ObjRef, BTreeMap<Option<String>, BundleEntry>>,
    builders: BTreeMap<ObjRef, String>,
    paints: BTreeMap<ObjRef, (u32, f32)>,
    canvases: BTreeMap<ObjRef, Vec<Draw>>,
    motion: BTreeMap<ObjRef, (i32, f32, f32, f32, f32, u64)>,
    lifecycle: Vec<Lifecycle>,
    focus: usize,
    pressed: Option<ObjRef>,
    finished: bool,
    resources: Option<ObjRef>,
    looper: Option<ObjRef>,
    title: String,
    assets: BTreeMap<u64, rd_apk::zip::ZipStream>,
    asset_streams: BTreeMap<ObjRef, u64>,
    asset_serial: u64,
    audio_tracks: BTreeMap<ObjRef, (u64, bool)>,
}
pub struct Android {
    services: services::Services,
    pub apk: Apk,
    pub width: i32,
    pub height: i32,
    clock: Rc<dyn Clock>,
    state: RefCell<State>,
    audio: RefCell<Option<Rc<dyn crate::PcmOutput>>>,
}
impl Android {
    pub fn new(
        apk: Apk,
        clock: Rc<dyn Clock>,
        width: i32,
        height: i32,
        fs: Rc<dyn as_platform::fs::FileSystem>,
    ) -> Result<Rc<Self>> {
        let services = services::Services::new(fs, &apk)?;
        Ok(Rc::new(Self {
            services,
            apk,
            clock,
            width,
            height,
            state: RefCell::new(State::default()),
            audio: RefCell::new(None),
        }))
    }
    pub fn now(&self) -> u64 {
        self.clock.monotonic_micros() / 1000
    }
    /// Bind the actual process resource provider used by java.lang.Runtime.
    pub fn bind_runtime_resources(&self, provider: Rc<dyn as_platform::RuntimeResources>) {
        self.services.runtime.bind(provider);
    }
    pub fn begin_main_exception_scope(&self, vm: &Engine) -> Result<MainExceptionScope> {
        self.services.executors.begin_main_exception_scope(vm)
    }
    pub fn finish_main_exception_scope<T>(
        &self,
        vm: &mut Engine,
        scope: MainExceptionScope,
        result: Result<T>,
    ) -> Result<T> {
        self.services
            .executors
            .finish_main_exception_scope(self, vm, scope, result)
    }
    pub fn dispatch_uncaught(
        &self,
        vm: &mut Engine,
        origin: &'static str,
        failure: VmExit,
    ) -> VmExit {
        self.services
            .executors
            .dispatch_uncaught(self, vm, origin, failure)
    }
    pub fn uncaught_exception_reports(&self) -> Vec<UncaughtExceptionReport> {
        self.services.executors.uncaught_reports()
    }
    fn main_boundary<T>(
        &self,
        vm: &mut Engine,
        callback: impl FnOnce(&mut Engine) -> Result<T>,
    ) -> Result<T> {
        let scope = self.begin_main_exception_scope(vm)?;
        let result = callback(vm);
        self.finish_main_exception_scope(vm, scope, result)
    }
    /// Pixels retained by live Java bitmaps and pending Canvas commands.
    pub fn bitmap_memory_bytes(&self) -> usize {
        self.services.bitmaps.bytes()
    }
    pub fn bind_background_workers(&self, backend: Rc<dyn as_platform::BackgroundWorkers>) {
        self.services.timers.bind(backend.clone());
        self.services.executors.bind(backend);
    }
    pub fn bind_main_thread(&self, vm: &Engine) -> Result<()> {
        self.services.executors.bind_main(vm)
    }
    /// Requested Java metadata, not a claim of OS-thread naming or JVM daemon
    /// process-exit policy. The token identifies the real dedicated worker.
    pub fn timer_worker_metadata(&self, token: u64) -> Option<(Option<String>, bool)> {
        self.services.timers.metadata(token)
    }
    /// Called on the dedicated worker under the runtime VM lease. Delays are
    /// relative milliseconds; None requests terminal worker cleanup.
    pub fn timer_worker_step(&self, vm: &mut Engine, token: u64) -> Result<Option<u64>> {
        self.services.timers.step(self, vm, token)
    }
    /// Platform calls only after the worker callback and TLS cleanup finish.
    pub fn timer_worker_stopped(&self, token: u64) {
        self.services.timers.stopped(token);
    }
    pub fn background_worker_error(&self, token: u64, failure: VmExit) {
        if token & services::executors::TOKEN_BIT != 0 {
            self.services.executors.worker_error(token, failure);
        } else {
            self.services.timers.worker_error(token, failure);
        }
    }
    /// Shared worker transport; tokens identify the owning service namespace.
    pub fn background_worker_step(&self, vm: &mut Engine, token: u64) -> Result<Option<u64>> {
        if token & services::executors::TOKEN_BIT != 0 {
            self.services.executors.step(self, vm, token)
        } else {
            self.timer_worker_step(vm, token)
        }
    }
    pub fn background_worker_stopped(&self, token: u64) {
        if token & services::executors::TOKEN_BIT != 0 {
            self.services.executors.stopped(token);
        } else {
            self.timer_worker_stopped(token);
        }
    }
    /// Bind the readable guest APK alias installed by the runtime's VFS.
    /// Call before launching: package metadata must not advertise an APK path
    /// that the application cannot actually open.
    pub fn bind_package_source(&self, guest_path: String, storage_path: String) -> Result<()> {
        self.services
            .bind_package_source(&self.apk, guest_path, storage_path)
    }
    pub fn bind_graphics_caps(&self, caps: GraphicsCaps) -> Result<()> {
        self.services.glsurface.bind_caps(caps)
    }
    pub fn bind_egl_backend(&self, backend: Rc<dyn EglConfigBackend>) {
        self.services.glsurface.bind_backend(backend);
    }
    pub fn graphics_caps(&self) -> Option<GraphicsCaps> {
        self.services.glsurface.caps()
    }
    pub fn software_size(&self) -> (i32, i32) {
        self.services.glsurface.software_size(self)
    }
    pub fn gl_requests(&self) -> Result<Vec<GlSurfaceRequest>> {
        self.services.glsurface.requests(self)
    }
    pub fn gl_choose_config(&self, vm: &mut Engine, view: ObjRef, display: u64) -> Result<u64> {
        self.services
            .glsurface
            .choose_config(self, vm, view, display)
    }
    pub fn gl_callback(&self, vm: &mut Engine, view: ObjRef, event: GlEvent) -> Result<()> {
        self.services.glsurface.callback(self, vm, view, event)
    }
    pub fn bind_audio(&self, audio: Rc<dyn crate::PcmOutput>) {
        *self.audio.borrow_mut() = Some(audio);
    }
    fn audio(&self) -> Result<Rc<dyn crate::PcmOutput>> {
        self.audio
            .borrow()
            .clone()
            .ok_or_else(|| error("API_UNAVAILABLE", "platform PCM output"))
    }
    pub fn finished(&self) -> bool {
        self.state.borrow().finished
    }
    pub fn title(&self) -> String {
        self.state.borrow().title.clone()
    }
    pub fn activity_object(&self) -> Option<ObjRef> {
        self.state.borrow().activities.last().map(|a| a.object)
    }
    /// Capture application-managed state in one real Bundle. Automatic window
    /// hierarchy serialization/recreation is not part of this callback contract.
    pub fn save_state(&self, vm: &mut Engine) -> Result<ObjRef> {
        self.main_boundary(vm, |vm| {
            let (activity, class) = self
                .state
                .borrow()
                .activities
                .last()
                .map(|a| (a.object, a.class.clone()))
                .ok_or_else(|| error("ACTIVITY_STATE", "no foreground activity to save"))?;
            let bundle = shell(vm, "Landroid/os/Bundle;")?;
            self.callback(vm, bundle, "<init>", "()V", &[])?;
            self.state
                .borrow_mut()
                .activities
                .last_mut()
                .ok_or_else(|| error("ACTIVITY_STATE", "foreground activity disappeared"))?
                .saved_state = Some(bundle);
            self.callback(
                vm,
                activity,
                "onSaveInstanceState",
                "(Landroid/os/Bundle;)V",
                &[Value::Obj(bundle)],
            )?;
            self.record(&class, "onSaveInstanceState");
            Ok(bundle)
        })
    }
    pub fn stop(&self, vm: &mut Engine) -> Result<()> {
        self.main_boundary(vm, |vm| self.stop_inner(vm))
    }
    fn stop_inner(&self, vm: &mut Engine) -> Result<()> {
        while let Some(r) = self.activity_object() {
            self.finish(vm, r)?;
        }
        self.abort_startup()
    }
    /// Release platform-owned resources after a failed launch without invoking
    /// lifecycle methods on an activity whose initialization did not complete.
    /// The caller must still join native callbacks/threads before dropping VM state.
    pub fn abort_startup(&self) -> Result<()> {
        self.services.flush()?;
        self.state.borrow_mut().tasks.clear();
        self.state.borrow_mut().actions.clear();
        let tracks = core::mem::take(&mut self.state.borrow_mut().audio_tracks);
        for (_, (id, _)) in tracks {
            self.audio()?.close(id)?;
        }
        Ok(())
    }
    pub fn views(&self) -> Vec<View> {
        let mut out = Vec::new();
        let mut pending = self.active_root().into_iter().collect::<Vec<_>>();
        let state = self.state.borrow();
        while let Some(r) = pending.pop() {
            if let Some(v) = state.views.get(&r) {
                out.push(v.clone());
                pending.extend(v.children.iter().rev().copied());
            }
        }
        out
    }
    pub fn lifecycle(&self) -> Vec<Lifecycle> {
        self.state.borrow().lifecycle.clone()
    }
    fn record(&self, class: &str, event: &str) {
        self.state.borrow_mut().lifecycle.push(Lifecycle {
            activity: class.into(),
            event: event.into(),
            time_ms: self.now(),
        });
    }
    fn callback(
        &self,
        vm: &mut Engine,
        r: ObjRef,
        name: &str,
        sig: &str,
        args: &[Value],
    ) -> Result<()> {
        vm.reset_fuel_window();
        if vm
            .cp
            .is_subtype(vm.heap.class_of(r)?, "Landroid/app/Activity;")
        {
            if let Some(event) = services::application_lifecycle::Event::from_method(name, sig) {
                return self.services.lifecycle_observers.perform(
                    vm,
                    r,
                    event,
                    args,
                    self.apk.manifest.target_sdk,
                );
            }
        }
        vm.invoke_instance(r, name, sig, args)?;
        Ok(())
    }
    pub fn launch(&self, vm: &mut Engine, entry: &str) -> Result<()> {
        self.main_boundary(vm, |vm| self.launch_inner(vm, entry))
    }
    fn launch_inner(&self, vm: &mut Engine, entry: &str) -> Result<()> {
        let app_class = self
            .apk
            .manifest
            .application
            .name
            .as_deref()
            .map(descriptor)
            .unwrap_or_else(|| "Landroid/app/Application;".into());
        let app = shell(vm, &app_class)?;
        self.state.borrow_mut().application = Some(app);
        self.callback(vm, app, "<init>", "()V", &[])?;
        let base = shell(vm, "Landroid/content/Context;")?;
        self.state.borrow_mut().roots.push(base);
        self.callback(
            vm,
            app,
            "attachBaseContext",
            "(Landroid/content/Context;)V",
            &[Value::Obj(base)],
        )?;
        self.state.borrow_mut().roots.retain(|r| *r != base);
        self.callback(vm, app, "onCreate", "()V", &[])?;
        self.start(vm, &descriptor(entry), None)?;
        self.tick(vm)
    }
    fn start(&self, vm: &mut Engine, class: &str, intent: Option<ObjRef>) -> Result<()> {
        if !self
            .apk
            .manifest
            .application
            .components
            .iter()
            .any(|c| descriptor(&c.class_name) == class && c.enabled != Some(false))
        {
            return Err(error("ACTIVITY_NOT_FOUND", class));
        }
        let previous = self.state.borrow().activities.last().cloned();
        if let Some(old) = &previous {
            self.callback(vm, old.object, "onPause", "()V", &[])?;
            self.record(&old.class, "onPause");
        }
        let r = shell(vm, class)?;
        self.state.borrow_mut().activities.push(Activity {
            object: r,
            class: class.into(),
            root: None,
            intent,
            saved_state: None,
        });
        self.callback(vm, r, "<init>", "()V", &[])?;
        let base = self.state.borrow().application.unwrap();
        self.callback(
            vm,
            r,
            "attachBaseContext",
            "(Landroid/content/Context;)V",
            &[Value::Obj(base)],
        )?;
        for (method, sig, args) in [
            ("onCreate", "(Landroid/os/Bundle;)V", vec![Value::Null]),
            ("onStart", "()V", vec![]),
            ("onResume", "()V", vec![]),
        ] {
            self.callback(vm, r, method, sig, &args)?;
            self.record(class, method);
        }
        if let Some(old) = previous {
            self.callback(vm, old.object, "onStop", "()V", &[])?;
            self.record(&old.class, "onStop");
        }
        self.state.borrow_mut().focus = 0;
        self.layout();
        Ok(())
    }
    pub fn tick(&self, vm: &mut Engine) -> Result<()> {
        self.main_boundary(vm, |vm| self.tick_inner(vm))
    }
    fn tick_inner(&self, vm: &mut Engine) -> Result<()> {
        self.services.flush()?;
        self.services.pump(self, vm)?;
        for _ in 0..256 {
            let action = self.state.borrow_mut().actions.pop_front();
            if let Some(action) = action {
                match action {
                    Action::Start(class, intent) => self.start(vm, &class, Some(intent))?,
                    Action::Finish(r) => self.finish(vm, r)?,
                }
                continue;
            }
            let next = {
                let mut s = self.state.borrow_mut();
                s.tasks.sort_by_key(|t| (t.due, t.order));
                if s.tasks.first().is_some_and(|t| t.due <= self.now()) {
                    Some(s.tasks.remove(0))
                } else {
                    None
                }
            };
            if let Some(task) = next {
                self.callback(vm, task.runnable, "run", "()V", &[])?;
                continue;
            }
            self.layout();
            return Ok(());
        }
        Err(error(
            "LOOPER_FUEL",
            "more than 256 immediate events in one pump",
        ))
    }
    fn finish(&self, vm: &mut Engine, r: ObjRef) -> Result<()> {
        let active = self
            .state
            .borrow()
            .activities
            .last()
            .cloned()
            .ok_or_else(|| error("ACTIVITY_STATE", "no foreground activity"))?;
        if active.object != r {
            return Err(error("API_UNAVAILABLE", "finish background activity"));
        }
        self.callback(vm, r, "onPause", "()V", &[])?;
        self.record(&active.class, "onPause");
        let previous = {
            let s = self.state.borrow();
            s.activities
                .len()
                .checked_sub(2)
                .and_then(|i| s.activities.get(i))
                .cloned()
        };
        if let Some(previous) = &previous {
            for method in ["onRestart", "onStart", "onResume"] {
                self.callback(vm, previous.object, method, "()V", &[])?;
                self.record(&previous.class, method);
            }
        }
        for method in ["onStop", "onDestroy"] {
            self.callback(vm, r, method, "()V", &[])?;
            self.record(&active.class, method);
        }
        self.state.borrow_mut().activities.pop();
        if previous.is_none() {
            self.state.borrow_mut().finished = true;
        }
        Ok(())
    }
    pub fn back(&self, vm: &mut Engine) -> Result<()> {
        self.main_boundary(vm, |vm| self.back_inner(vm))
    }
    fn back_inner(&self, vm: &mut Engine) -> Result<()> {
        let r = self.state.borrow().activities.last().map(|a| a.object);
        if let Some(r) = r {
            self.callback(vm, r, "onBackPressed", "()V", &[])?;
        }
        self.tick(vm)
    }
    fn find(&self, root: ObjRef, id: i32) -> Option<ObjRef> {
        let s = self.state.borrow();
        let mut pending = vec![root];
        while let Some(r) = pending.pop() {
            if let Some(v) = s.views.get(&r) {
                if v.id == id {
                    return Some(r);
                }
                pending.extend(v.children.iter().copied());
            }
        }
        None
    }
    fn active_root(&self) -> Option<ObjRef> {
        self.state.borrow().activities.last().and_then(|a| a.root)
    }
    pub fn click(&self, vm: &mut Engine, r: ObjRef) -> Result<bool> {
        self.main_boundary(vm, |vm| self.click_inner(vm, r))
    }
    fn click_inner(&self, vm: &mut Engine, r: ObjRef) -> Result<bool> {
        let view = self
            .state
            .borrow()
            .views
            .get(&r)
            .cloned()
            .ok_or_else(|| error("INVALID_VIEW", "click"))?;
        if !view.enabled || !view.visible {
            return Ok(false);
        }
        if let Some(listener) = view.listener {
            self.callback(
                vm,
                listener,
                "onClick",
                "(Landroid/view/View;)V",
                &[Value::Obj(r)],
            )?;
        } else if let Some(method) = view.click_method {
            let activity = self.state.borrow().activities.last().unwrap().object;
            self.callback(
                vm,
                activity,
                &method,
                "(Landroid/view/View;)V",
                &[Value::Obj(r)],
            )?;
        } else {
            return Ok(false);
        }
        self.tick_inner(vm)?;
        Ok(true)
    }
    pub fn touch(&self, vm: &mut Engine, action: i32, x: f32, y: f32) -> Result<()> {
        self.main_boundary(vm, |vm| self.touch_inner(vm, action, x, y))
    }
    fn touch_inner(&self, vm: &mut Engine, action: i32, x: f32, y: f32) -> Result<()> {
        self.layout();
        let root = self.active_root();
        let hit = root.and_then(|r| self.hit(r, x as i32, y as i32));
        if action == 0 {
            self.state.borrow_mut().pressed = hit;
        }
        let target = if action == 0 {
            hit
        } else {
            self.state.borrow().pressed.or(hit)
        };
        let mut consumed = false;
        if let Some(r) = target {
            let custom = vm
                .cp
                .resolve_method(
                    vm.heap.class_of(r)?,
                    "onTouchEvent",
                    "(Landroid/view/MotionEvent;)Z",
                )
                .is_some();
            let listener = self.state.borrow().views[&r].touch_listener;
            if custom || listener.is_some() {
                let bounds = self.state.borrow().views[&r].bounds;
                let event = shell(vm, "Landroid/view/MotionEvent;")?;
                self.state.borrow_mut().motion.insert(
                    event,
                    (
                        action,
                        x - bounds[0] as f32,
                        y - bounds[1] as f32,
                        x,
                        y,
                        self.now(),
                    ),
                );
                self.state.borrow_mut().roots.push(event);
                let result = (|| -> Result<bool> {
                    vm.reset_fuel_window();
                    if let Some(listener) = listener {
                        if vm
                            .invoke_instance(
                                listener,
                                "onTouch",
                                "(Landroid/view/View;Landroid/view/MotionEvent;)Z",
                                &[Value::Obj(r), Value::Obj(event)],
                            )?
                            .as_int()?
                            != 0
                        {
                            return Ok(true);
                        }
                    }
                    if custom {
                        return Ok(vm
                            .invoke_instance(
                                r,
                                "onTouchEvent",
                                "(Landroid/view/MotionEvent;)Z",
                                &[Value::Obj(event)],
                            )?
                            .as_int()?
                            != 0);
                    }
                    Ok(false)
                })();
                self.state.borrow_mut().roots.retain(|v| *v != event);
                self.state.borrow_mut().motion.remove(&event);
                consumed = result?;
            }
            if !consumed && self.state.borrow().views[&r].clickable {
                if action == 1 && self.state.borrow().pressed == hit {
                    self.click(vm, r)?;
                }
                consumed = true;
            }
        }
        if !consumed {
            if let Some(activity) = self.activity_object() {
                if vm
                    .cp
                    .resolve_method(
                        vm.heap.class_of(activity)?,
                        "onTouchEvent",
                        "(Landroid/view/MotionEvent;)Z",
                    )
                    .is_some()
                {
                    let event = shell(vm, "Landroid/view/MotionEvent;")?;
                    self.state
                        .borrow_mut()
                        .motion
                        .insert(event, (action, x, y, x, y, self.now()));
                    self.state.borrow_mut().roots.push(event);
                    let result = self.callback(
                        vm,
                        activity,
                        "onTouchEvent",
                        "(Landroid/view/MotionEvent;)Z",
                        &[Value::Obj(event)],
                    );
                    self.state.borrow_mut().roots.retain(|r| *r != event);
                    self.state.borrow_mut().motion.remove(&event);
                    result?;
                }
            }
        }
        if action == 1 || action == 3 {
            self.state.borrow_mut().pressed = None;
        }
        self.tick(vm)
    }
    fn hit(&self, r: ObjRef, x: i32, y: i32) -> Option<ObjRef> {
        let view = self.state.borrow().views.get(&r)?.clone();
        let [vx, vy, w, h] = view.bounds;
        if !view.visible || !view.enabled || x < vx || y < vy || x >= vx + w || y >= vy + h {
            return None;
        }
        for child in view.children.iter().rev() {
            if let Some(hit) = self.hit(*child, x, y) {
                return Some(hit);
            }
        }
        Some(r)
    }
    pub fn key(&self, vm: &mut Engine, key: i32) -> Result<()> {
        self.main_boundary(vm, |vm| self.key_inner(vm, key))
    }
    fn key_inner(&self, vm: &mut Engine, key: i32) -> Result<()> {
        if key == 4 {
            return self.back(vm);
        }
        let Some(root) = self.active_root() else {
            return Ok(());
        };
        let mut all = vec![];
        self.clickables(root, &mut all);
        if all.is_empty() {
            return Ok(());
        }
        let mut s = self.state.borrow_mut();
        if key == 20 || key == 22 {
            s.focus = (s.focus + 1) % all.len();
        } else if key == 19 || key == 21 {
            s.focus = (s.focus + all.len() - 1) % all.len();
        }
        let focus = all[s.focus % all.len()];
        drop(s);
        if key == 23 || key == 66 {
            self.click_inner(vm, focus)?;
        }
        Ok(())
    }
    fn clickables(&self, r: ObjRef, out: &mut Vec<ObjRef>) {
        if let Some(v) = self.state.borrow().views.get(&r).cloned() {
            if v.visible && v.enabled && v.clickable {
                out.push(r)
            }
            for c in v.children {
                self.clickables(c, out)
            }
        }
    }
    fn layout(&self) {
        if let Some(root) = self.active_root() {
            let (width, height) = self
                .services
                .glsurface
                .dimensions(root)
                .unwrap_or_else(|| self.software_size());
            self.layout_node(root, 0, 0, width, height, 0);
        }
    }
    fn preferred(&self, r: ObjRef, vertical: bool, depth: usize) -> i32 {
        if depth > 64 {
            return 0;
        }
        let Some(v) = self.state.borrow().views.get(&r).cloned() else {
            return 0;
        };
        let explicit = if vertical { v.height } else { v.width };
        if explicit >= 0 {
            return explicit;
        }
        if v.children.is_empty() {
            if vertical {
                if v.class == "Landroid/widget/TextView;" {
                    36
                } else {
                    48
                }
            } else {
                (v.text.chars().count() as i32 * 10 + 32).max(64)
            }
        } else {
            let sizes = v
                .children
                .iter()
                .map(|r| self.preferred(*r, vertical, depth + 1));
            if (v.orientation == 1) == vertical {
                sizes.sum()
            } else {
                sizes.max().unwrap_or(0)
            }
        }
    }
    fn layout_node(&self, r: ObjRef, x: i32, y: i32, w: i32, h: i32, depth: usize) {
        if depth > 64 {
            return;
        }
        let Some(v) = self.state.borrow().views.get(&r).cloned() else {
            return;
        };
        self.state.borrow_mut().views.get_mut(&r).unwrap().bounds = [x, y, w.max(0), h.max(0)];
        let mut at = 0;
        for child in &v.children {
            let Some(c) = self.state.borrow().views.get(child).cloned() else {
                continue;
            };
            if !c.visible {
                continue;
            }
            let vertical = v.orientation == 1;
            let main = if vertical { c.height } else { c.width };
            let amount = if main == -1 {
                (if vertical { h } else { w }) - at
            } else {
                self.preferred(*child, vertical, depth + 1)
            }
            .max(0);
            let cross = if vertical { c.width } else { c.height };
            let cross_size = if cross == -1 {
                if vertical {
                    w
                } else {
                    h
                }
            } else if cross >= 0 {
                cross
            } else {
                self.preferred(*child, !vertical, depth + 1)
            };
            if v.class == "Landroid/widget/FrameLayout;" {
                self.layout_node(
                    *child,
                    x,
                    y,
                    if c.width < 0 { w } else { c.width },
                    if c.height < 0 { h } else { c.height },
                    depth + 1,
                )
            } else if vertical {
                self.layout_node(
                    *child,
                    x,
                    y + at,
                    cross_size.min(w),
                    amount.min((h - at).max(0)),
                    depth + 1,
                )
            } else {
                self.layout_node(
                    *child,
                    x + at,
                    y,
                    amount.min((w - at).max(0)),
                    cross_size.min(h),
                    depth + 1,
                )
            }
            at += amount;
        }
    }
    pub fn render(&self, vm: &mut Engine, frame: &mut Frame) -> Result<()> {
        self.layout();
        // The platform EGL thread owns this surface. A successful event pump
        // is not a claim that the software rasterizer produced a GL frame.
        if !self.gl_requests()?.is_empty() {
            return Ok(());
        }
        self.services.glsurface.mark_display_started();
        frame.clear(0xff101419);
        if let Some(root) = self.active_root() {
            self.draw_view(vm, frame, root, 0)?;
        }
        Ok(())
    }
    fn draw_view(&self, vm: &mut Engine, frame: &mut Frame, r: ObjRef, depth: usize) -> Result<()> {
        if depth > 64 {
            return Err(error("VIEW_DEPTH", "draw tree >64"));
        }
        let v = self.state.borrow().views[&r].clone();
        if !v.visible {
            return Ok(());
        }
        let [x, y, w, h] = v.bounds;
        frame.rect(x, y, w, h, v.color);
        if !v.text.is_empty() {
            frame.text(&v.text, x + 12, y + 26, v.text_size, v.text_color);
        }
        let class = vm.heap.class_of(r)?.to_string();
        if vm
            .cp
            .resolve_method(&class, "onDraw", "(Landroid/graphics/Canvas;)V")
            .is_some()
        {
            let canvas = shell(vm, "Landroid/graphics/Canvas;")?;
            self.state.borrow_mut().canvases.insert(canvas, vec![]);
            self.state.borrow_mut().roots.push(canvas);
            let draw = self.callback(
                vm,
                r,
                "onDraw",
                "(Landroid/graphics/Canvas;)V",
                &[Value::Obj(canvas)],
            );
            let commands = self
                .state
                .borrow_mut()
                .canvases
                .remove(&canvas)
                .unwrap_or_default();
            self.state.borrow_mut().roots.retain(|o| *o != canvas);
            draw?;
            for cmd in commands {
                match cmd {
                    Draw::Bitmap(bitmap) => bitmap.render(frame, x, y, w, h),
                    Draw::Color(c) => frame.rect(x, y, w, h, c),
                    Draw::Rect(b, c) => frame.rect(
                        x + b[0] as i32,
                        y + b[1] as i32,
                        (b[2] - b[0]) as i32,
                        (b[3] - b[1]) as i32,
                        c,
                    ),
                    Draw::Circle(cx, cy, r, c) => {
                        frame.circle(x + cx as i32, y + cy as i32, r as i32, c)
                    }
                    Draw::Text(t, tx, ty, size, c) => {
                        frame.text(&t, x + tx as i32, y + ty as i32, size, c)
                    }
                }
            }
        }
        for child in v.children {
            self.draw_view(vm, frame, child, depth + 1)?;
        }
        Ok(())
    }
    fn resource(&self, id: u32, depth: usize) -> Result<ResolvedValue> {
        if depth > 8 {
            return Err(error("RESOURCE_CYCLE", format!("{id:#x}")));
        }
        match self
            .apk
            .arsc
            .as_ref()
            .and_then(|a| a.resolve_value_for_api(id, 26))
            .ok_or_else(|| error("RESOURCE_NOT_FOUND", format!("{id:#x}")))?
        {
            ResolvedValue::Reference(id) => self.resource(id, depth + 1),
            v => Ok(v),
        }
    }
    fn resource_string(&self, id: u32) -> Result<String> {
        match self.resource(id, 0)? {
            ResolvedValue::Str(s) => Ok(s),
            _ => Err(error("RESOURCE_TYPE", format!("{id:#x} is not a string"))),
        }
    }
    fn inflate(&self, vm: &mut Engine, id: u32, context: ObjRef) -> Result<ObjRef> {
        let path = self.resource_string(id)?;
        let bytes = self
            .apk
            .zip
            .read(&path)
            .map_err(|e| error(&e.code, e.cause))?;
        let xml = rd_apk::axml::parse(&bytes).map_err(|e| error(&e.code, e.cause))?;
        self.inflate_node(vm, &xml.root, context, 0)
    }
    fn inflate_node(
        &self,
        vm: &mut Engine,
        node: &XmlElement,
        context: ObjRef,
        depth: usize,
    ) -> Result<ObjRef> {
        if depth > 64 {
            return Err(error("VIEW_DEPTH", "XML nesting >64"));
        }
        let class = if node.name.contains('.') {
            descriptor(&node.name)
        } else {
            match node.name.as_str() {
                "View" => "Landroid/view/View;".into(),
                "LinearLayout" | "FrameLayout" | "TextView" | "Button" | "EditText" => {
                    format!("Landroid/widget/{};", node.name)
                }
                _ => {
                    return Err(error(
                        "API_UNAVAILABLE",
                        format!("LayoutInflater tag {}", node.name),
                    ))
                }
            }
        };
        let r = shell(vm, &class)?;
        self.state.borrow_mut().roots.push(r);
        self.callback(
            vm,
            r,
            "<init>",
            "(Landroid/content/Context;)V",
            &[Value::Obj(context)],
        )?;
        for attribute in &node.attrs {
            let value = attribute.value.clone();
            let name = attribute.name.as_str();
            match name {
                "autoText" | "capitalize"
                    if vm.cp.is_subtype(&class, "Landroid/widget/EditText;") =>
                {
                    let bits = if name == "autoText" {
                        if attribute.as_bool() == Some(true) {
                            0x8000
                        } else {
                            0
                        }
                    } else {
                        match attribute.as_i32().unwrap_or(0) {
                            1 => 0x1000,
                            2 => 0x2000,
                            3 => 0x4000,
                            _ => 0,
                        }
                    };
                    let mut state = self.state.borrow_mut();
                    let view = state.views.get_mut(&r).unwrap();
                    view.input_type = Some(view.input_type.unwrap_or(1) | bits);
                }
                "id" => {
                    self.state.borrow_mut().views.get_mut(&r).unwrap().id =
                        attribute.as_u32().unwrap_or(0) as i32
                }
                "text" => {
                    let s = match value {
                        AttrValue::Reference(id) => self.resource_string(id)?,
                        AttrValue::String(s) => s,
                        _ => return Err(error("RESOURCE_TYPE", "XML text")),
                    };
                    self.state.borrow_mut().views.get_mut(&r).unwrap().text = s;
                }
                "layout_width" | "layout_height" => {
                    let dimension = match value {
                        AttrValue::Dimension(
                            n,
                            ComplexUnit::Dip | ComplexUnit::Px | ComplexUnit::Sp,
                        ) => n as i32,
                        AttrValue::Int(n) => n,
                        _ => {
                            return Err(error(
                                "API_UNAVAILABLE",
                                format!("layout dimension {value:?}"),
                            ))
                        }
                    };
                    let mut s = self.state.borrow_mut();
                    let view = s.views.get_mut(&r).unwrap();
                    if name == "layout_width" {
                        view.width = dimension
                    } else {
                        view.height = dimension
                    }
                }
                "orientation" => {
                    self.state
                        .borrow_mut()
                        .views
                        .get_mut(&r)
                        .unwrap()
                        .orientation = attribute.as_i32().unwrap_or(0)
                }
                "background" | "textColor" => {
                    let color = match value {
                        AttrValue::Reference(id) => match self.resource(id, 0)? {
                            ResolvedValue::Integer(c) => c as u32,
                            _ => return Err(error("API_UNAVAILABLE", "drawable backgrounds")),
                        },
                        AttrValue::Int(c) => c as u32,
                        AttrValue::Raw { data, .. } => data,
                        _ => return Err(error("RESOURCE_TYPE", "color")),
                    };
                    let mut s = self.state.borrow_mut();
                    let view = s.views.get_mut(&r).unwrap();
                    if name == "background" {
                        view.color = color
                    } else {
                        view.text_color = color
                    }
                }
                "textSize" => {
                    let n = match value {
                        AttrValue::Dimension(
                            n,
                            ComplexUnit::Sp | ComplexUnit::Dip | ComplexUnit::Px,
                        ) => n,
                        _ => return Err(error("RESOURCE_TYPE", "text size")),
                    };
                    self.state.borrow_mut().views.get_mut(&r).unwrap().text_size = n
                }
                "onClick" => {
                    let mut s = self.state.borrow_mut();
                    let v = s.views.get_mut(&r).unwrap();
                    v.click_method = Some(attribute.text());
                    v.clickable = true;
                }
                "enabled" | "clickable" => {
                    let b = attribute.as_bool().unwrap_or(false);
                    let mut s = self.state.borrow_mut();
                    let v = s.views.get_mut(&r).unwrap();
                    if name == "enabled" {
                        v.enabled = b
                    } else {
                        v.clickable = b
                    }
                }
                _ => {
                    return Err(error(
                        "API_UNAVAILABLE",
                        format!("LayoutInflater attribute {}", attribute.name),
                    ))
                }
            }
        }
        for child in &node.children {
            let c = self.inflate_node(vm, child, context, depth + 1)?;
            self.state
                .borrow_mut()
                .views
                .get_mut(&r)
                .unwrap()
                .children
                .push(c);
        }
        self.state.borrow_mut().roots.retain(|o| *o != r);
        Ok(r)
    }
    fn platform_class(&self, vm: &Engine, class: &str) -> Option<String> {
        let mut c = class.to_string();
        for _ in 0..64 {
            if matches!(
                c.as_str(),
                "Landroid/app/Activity;"
                    | "Landroid/app/NativeActivity;"
                    | "Landroid/content/res/AssetManager;"
                    | "Landroid/content/res/AssetManager$AssetInputStream;"
                    | "Landroid/media/AudioTrack;"
                    | "Landroid/app/Application;"
                    | "Landroid/content/Context;"
                    | "Landroid/content/ContextWrapper;"
                    | "Landroid/content/pm/ApplicationInfo;"
                    | "Ljava/lang/ClassLoader;"
                    | "Ljava/lang/BootClassLoader;"
                    | "Landroid/view/View;"
                    | "Landroid/opengl/GLSurfaceView;"
                    | "Landroid/view/SurfaceView;"
                    | "Landroid/view/ViewGroup;"
                    | "Landroid/widget/TextView;"
                    | "Landroid/widget/EditText;"
                    | "Landroid/widget/Button;"
                    | "Landroid/widget/LinearLayout;"
                    | "Landroid/widget/FrameLayout;"
                    | "Landroid/os/Bundle;"
                    | "Landroid/content/Intent;"
                    | "Landroid/os/Handler;"
                    | "Landroid/os/Looper;"
                    | "Landroid/graphics/Paint;"
                    | "Landroid/graphics/Canvas;"
                    | "Landroid/view/MotionEvent;"
                    | "Landroid/content/res/Resources;"
                    | "Landroid/view/LayoutInflater;"
                    | "Ljava/lang/StringBuilder;"
                    | "Ljava/lang/StringBuffer;"
                    | "Landroid/view/ViewGroup$LayoutParams;"
                    | "Landroid/widget/LinearLayout$LayoutParams;"
            ) {
                return Some(c);
            }
            c = vm.cp.superclass_of(&c)?;
        }
        None
    }
    fn is_view(vm: &Engine, class: &str) -> bool {
        vm.cp.is_subtype(class, "Landroid/view/View;")
    }
    fn validate_input_target(&self, vm: &Engine, view: ObjRef) -> Result<()> {
        if Self::is_view(vm, vm.heap.class_of(view)?)
            && self.state.borrow().views.contains_key(&view)
        {
            Ok(())
        } else {
            Err(error(
                "INVALID_VIEW",
                "input target has no registered View state",
            ))
        }
    }
    fn bind_view_context(&self, vm: &Engine, view: ObjRef, context: ObjRef) -> Result<()> {
        self.validate_input_target(vm, view)?;
        if !vm
            .cp
            .is_subtype(vm.heap.class_of(context)?, "Landroid/content/Context;")
        {
            return Err(error(
                "INVALID_CONTEXT",
                "View constructor requires a Context",
            ));
        }
        self.state
            .borrow_mut()
            .views
            .get_mut(&view)
            .unwrap()
            .context = Some(context);
        Ok(())
    }
    fn content_activity(&self, context: ObjRef) -> Result<usize> {
        self.state
            .borrow()
            .activities
            .iter()
            .position(|a| a.object == context)
            .ok_or_else(|| error("ACTIVITY_STATE", "receiver is not a live Activity"))
    }
}

impl FrameworkHost for Android {
    fn method_metadata(
        &self,
        class: &str,
        name: &str,
        sig: &str,
    ) -> Option<rd_vm::framework_host::HostMethodKind> {
        services::Services::method_metadata(class, name, sig)
    }
    fn thread_attached(&self, owner: u64, name: Option<&str>) -> Result<()> {
        self.services.executors.attached(owner, name)
    }
    fn thread_detached(&self, owner: u64) {
        self.services.executors.detached(owner)
    }
    fn static_field(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        descriptor: &str,
    ) -> Result<Option<Value>> {
        self.services.static_field(vm, class, name, descriptor)
    }
    fn asset_open(&self, name: &str) -> Result<u64> {
        if name.is_empty()
            || name.starts_with('/')
            || name.contains('\\')
            || name
                .split('/')
                .any(|s| s == ".." || s == "." || s.is_empty())
        {
            return Err(error("INVALID_ASSET_PATH", name));
        }
        let stream = self
            .apk
            .zip
            .stream(&format!("assets/{name}"))
            .map_err(|e| error(&e.code, e.cause))?;
        let mut s = self.state.borrow_mut();
        if s.assets.len() >= 256 {
            return Err(error("ASSET_HANDLE_LIMIT", "256 open assets"));
        }
        s.asset_serial += 1;
        let id = s.asset_serial;
        s.assets.insert(id, stream);
        Ok(id)
    }
    fn asset_read(&self, id: u64, output: &mut [u8]) -> Result<usize> {
        self.state
            .borrow_mut()
            .assets
            .get_mut(&id)
            .ok_or_else(|| error("ASSET_CLOSED", "read"))?
            .read(output)
            .map_err(|e| error(&e.code, e.cause))
    }
    fn asset_len(&self, id: u64) -> Result<u64> {
        Ok(self
            .state
            .borrow()
            .assets
            .get(&id)
            .ok_or_else(|| error("ASSET_CLOSED", "length"))?
            .len())
    }
    fn asset_seek(&self, id: u64, offset: i64, origin: i32) -> Result<u64> {
        let mut s = self.state.borrow_mut();
        let stream = s
            .assets
            .get_mut(&id)
            .ok_or_else(|| error("ASSET_CLOSED", "seek"))?;
        let base = match origin {
            0 => 0,
            1 => stream.position(),
            2 => stream.len(),
            _ => return Err(error("INVALID_SEEK", "origin")),
        };
        let target = (base as i128 + offset as i128)
            .try_into()
            .map_err(|_| error("INVALID_SEEK", "overflow/negative"))?;
        stream.seek(target).map_err(|e| error(&e.code, e.cause))
    }
    fn asset_close(&self, id: u64) {
        self.state.borrow_mut().assets.remove(&id);
    }
    fn references(&self, r: ObjRef) -> Vec<ObjRef> {
        let s = self.state.borrow();
        let mut refs = vec![];
        if let Some(v) = s.views.get(&r) {
            refs.extend(v.context);
            refs.extend(v.children.iter().copied());
            refs.extend(v.listener);
            refs.extend(v.touch_listener);
        }
        if let Some(bundle) = s.bundles.get(&r) {
            refs.extend(
                bundle
                    .values()
                    .filter_map(|v| v.value.as_ref().ok().flatten()),
            );
        }
        refs.extend(self.services.references(r));
        refs
    }
    fn collect(&self, marked: &[bool]) {
        self.services.collect(marked);
        let live = |r: &ObjRef| marked.get(*r as usize).copied().unwrap_or(false);
        let mut s = self.state.borrow_mut();
        s.views.retain(|r, _| live(r));
        s.bundles.retain(|r, _| live(r));
        s.builders.retain(|r, _| live(r));
        s.paints.retain(|r, _| live(r));
        s.intents.retain(|r, _| live(r));
        s.motion.retain(|r, _| live(r));
        s.canvases.retain(|r, _| live(r));
        let dead: Vec<_> = s
            .asset_streams
            .iter()
            .filter(|(r, _)| !live(r))
            .map(|(r, id)| (*r, *id))
            .collect();
        for (r, id) in dead {
            s.asset_streams.remove(&r);
            s.assets.remove(&id);
        }
        let dead: Vec<_> = s
            .audio_tracks
            .iter()
            .filter(|(r, _)| !live(r))
            .map(|(r, (id, _))| (*r, *id))
            .collect();
        for (r, id) in dead {
            s.audio_tracks.remove(&r);
            if let Ok(audio) = self.audio() {
                let _ = audio.close(id);
            }
        }
    }
    fn attach(&self, vm: &mut Engine, class: &str, r: ObjRef) -> Result<bool> {
        if self.services.attach(vm, class, r)? {
            return Ok(true);
        }
        let Some(platform) = self.platform_class(vm, class) else {
            return Ok(false);
        };
        let mut s = self.state.borrow_mut();
        if Self::is_view(vm, &platform) {
            s.views.insert(r, View::new(r, &platform));
        }
        match platform.as_str() {
            "Landroid/content/Intent;" => {
                s.intents.insert(r, String::new());
            }
            "Landroid/os/Bundle;" => {
                s.bundles.insert(r, BTreeMap::new());
            }
            "Ljava/lang/StringBuilder;" | "Ljava/lang/StringBuffer;" => {
                s.builders.insert(r, String::new());
            }
            "Landroid/graphics/Paint;" => {
                s.paints.insert(r, (0xff000000, 16.));
            }
            _ => {}
        }
        Ok(true)
    }
    fn call_static(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        args: &[Value],
    ) -> Result<Option<Value>> {
        let arguments = typed_args(sig, args)?;
        let args = arguments.as_slice();
        if let Some(value) = self
            .services
            .bitmaps
            .call_static(self, vm, class, name, sig, args)?
        {
            return Ok(Some(value));
        }
        if let Some(value) = self
            .services
            .call_static(self, vm, class, name, sig, args)?
        {
            return Ok(Some(value));
        }
        let value = match (class, name, sig) {
            ("Landroid/media/AudioTrack;", "getMinBufferSize", "(III)I") => Value::Int(
                if int(args, 0)? == 48000 && int(args, 1)? == 12 && int(args, 2)? == 2 {
                    1920
                } else {
                    -2
                },
            ),
            ("Landroid/os/SystemClock;", "uptimeMillis" | "elapsedRealtime", "()J") => {
                Value::Long(self.now() as i64)
            }
            ("Ljava/lang/System;", "nanoTime", "()J") => {
                Value::Long(self.clock.monotonic_micros().saturating_mul(1000) as i64)
            }
            ("Ljava/lang/System;", "currentTimeMillis", "()J") => Value::Long(
                self.clock
                    .wall_time_millis()
                    .map_err(|e| error(e.code, e.to_string()))?,
            ),
            ("Ljava/lang/Integer;", "toString", "(I)Ljava/lang/String;") => {
                string(vm, int(args, 0)?.to_string())?
            }
            ("Ljava/lang/Integer;", "parseInt", "(Ljava/lang/String;)I") => Value::Int(
                text(vm, &args[0])?
                    .parse()
                    .map_err(|_| Throwable::new("Ljava/lang/NumberFormatException;", "integer"))?,
            ),
            ("Ljava/lang/String;", "valueOf", _) if sig.ends_with("Ljava/lang/String;") => {
                string(vm, display(vm, &args[0])?)?
            }
            (
                "Lkotlin/jvm/internal/Intrinsics;",
                "checkNotNullParameter" | "checkNotNullExpressionValue",
                "(Ljava/lang/Object;Ljava/lang/String;)V",
            ) => {
                obj(&args[0])?;
                Value::Null
            }
            ("Landroid/os/Looper;", "getMainLooper" | "myLooper", "()Landroid/os/Looper;") => {
                if name == "myLooper"
                    && (self.services.glsurface.in_callback()
                        || self.services.timers.in_callback(vm)?
                        || self.services.executors.in_callback(vm)?)
                {
                    return Ok(Some(Value::Null));
                }
                let existing = self.state.borrow().looper;
                Value::Obj(if let Some(r) = existing {
                    r
                } else {
                    let r = shell(vm, class)?;
                    self.state.borrow_mut().looper = Some(r);
                    r
                })
            }
            (
                "Landroid/view/LayoutInflater;",
                "from",
                "(Landroid/content/Context;)Landroid/view/LayoutInflater;",
            ) => {
                let r = shell(vm, class)?;
                write(vm, r, "context", args[0].clone())?;
                Value::Obj(r)
            }
            ("Landroid/graphics/Color;", "rgb", "(III)I") => Value::Int(
                (0xff000000
                    | ((int(args, 0)? as u32 & 255) << 16)
                    | ((int(args, 1)? as u32 & 255) << 8)
                    | (int(args, 2)? as u32 & 255)) as i32,
            ),
            _ => return Ok(None),
        };
        Ok(Some(value))
    }
    fn call(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        r: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        let arguments = typed_args(sig, args)?;
        let args = arguments.as_slice();
        if class == "Landroid/os/Handler;"
            && name == "<init>"
            && sig == "()V"
            && (self.services.glsurface.in_callback()
                || self.services.timers.in_callback(vm)?
                || self.services.executors.in_callback(vm)?)
        {
            return Err(Throwable::new(
                "Ljava/lang/RuntimeException;",
                "Cannot create Handler: platform callback worker has no prepared Looper",
            )
            .into());
        }
        if let Some(value) = self.services.call(self, vm, class, name, sig, r, args)? {
            return Ok(Some(value));
        }
        if vm.heap.class_of(r)? == "Ljava/lang/String;" {
            let s = vm.heap.as_str(r)?.to_string();
            let v = match (name, sig) {
                ("length", "()I") => Value::Int(s.encode_utf16().count() as i32),
                ("toString", "()Ljava/lang/String;") => Value::Obj(r),
                ("equals", "(Ljava/lang/Object;)Z") => Value::Int(
                    args[0]
                        .as_ref()?
                        .and_then(|r| vm.heap.as_str(r).ok())
                        .is_some_and(|other| s == other) as i32,
                ),
                ("concat", "(Ljava/lang/String;)Ljava/lang/String;") => {
                    string(vm, format!("{s}{}", text(vm, &args[0])?))?
                }
                ("endsWith", "(Ljava/lang/String;)Z") => {
                    Value::Int(s.ends_with(&text(vm, &args[0])?) as i32)
                }
                ("charAt", "(I)C") => Value::Int(
                    s.encode_utf16()
                        .nth(int(args, 0)? as usize)
                        .ok_or_else(|| {
                            Throwable::new("Ljava/lang/StringIndexOutOfBoundsException;", "charAt")
                        })? as i32,
                ),
                _ => return Ok(None),
            };
            return Ok(Some(v));
        }
        if class == "Ljava/lang/Object;" && name == "getClass" && sig == "()Ljava/lang/Class;" {
            let descriptor = vm.heap.class_of(r)?.to_string();
            let c = vm.device_class_object(&descriptor)?;
            return Ok(Some(Value::Obj(c)));
        }
        let actual = vm.heap.class_of(r)?.to_string();
        if name == "equals"
            && sig == "(Ljava/lang/Object;)Z"
            && !vm
                .cp
                .resolve_method(&actual, name, sig)
                .is_some_and(|(_, definition, _)| definition.access_flags & 0x0200 == 0)
        {
            // A virtual reference declared on Object can still select a
            // builtin override. Arrays and a DEX-only class chain reaching
            // Object prove inherited identity here. True invoke-super
            // Object.equals is handled by the opcode-aware interpreter path.
            let array = matches!(
                vm.heap.get(r).map_err(|cause| error("HEAP_READ", cause))?,
                rd_vm::heap::HeapObj::Array { .. }
            );
            let mut ancestor = actual.clone();
            let mut remaining = 64;
            while !array && ancestor != "Ljava/lang/Object;" {
                if remaining == 0 || vm.cp.find_class(&ancestor).is_none() {
                    return Err(error(
                        "API_UNAVAILABLE",
                        format!("Object.equals implementation for builtin boundary {ancestor}"),
                    ));
                }
                ancestor = vm.cp.superclass_of(&ancestor).ok_or_else(|| {
                    error(
                        "INVALID_CLASS_HIERARCHY",
                        "Object.equals superclass missing",
                    )
                })?;
                remaining -= 1;
            }
            return Ok(Some(Value::Int((args[0].as_ref()? == Some(r)) as i32)));
        }
        let Some(platform) = self
            .platform_class(vm, class)
            .or_else(|| self.platform_class(vm, &actual))
        else {
            return Ok(None);
        };
        let value = match (name, sig) {
            ("<init>", "(IIIIII)V") if platform == "Landroid/media/AudioTrack;" => {
                if int(args, 0)? != 3
                    || int(args, 1)? != 48000
                    || int(args, 2)? != 12
                    || int(args, 3)? != 2
                    || int(args, 4)? < 1920
                    || int(args, 5)? != 1
                {
                    return Err(error("AUDIO_FORMAT_UNAVAILABLE","AudioTrack requires STREAM_MUSIC, 48kHz stereo PCM16, MODE_STREAM, buffer>=1920"));
                }
                let id = self.audio()?.open(48000, 2, 1)?;
                self.state.borrow_mut().audio_tracks.insert(r, (id, false));
                Value::Null
            }
            ("getState", "()I") if platform == "Landroid/media/AudioTrack;" => {
                Value::Int(self.state.borrow().audio_tracks.contains_key(&r) as i32)
            }
            ("getPlayState", "()I") if platform == "Landroid/media/AudioTrack;" => Value::Int(
                if self
                    .state
                    .borrow()
                    .audio_tracks
                    .get(&r)
                    .is_some_and(|(_, playing)| *playing)
                {
                    3
                } else {
                    1
                },
            ),
            ("play", "()V") if platform == "Landroid/media/AudioTrack;" => {
                let id = self
                    .state
                    .borrow()
                    .audio_tracks
                    .get(&r)
                    .ok_or_else(|| error("AUDIO_RELEASED", "play"))?
                    .0;
                self.audio()?.start(id)?;
                self.state.borrow_mut().audio_tracks.get_mut(&r).unwrap().1 = true;
                Value::Null
            }
            ("stop", "()V") if platform == "Landroid/media/AudioTrack;" => {
                let id = self
                    .state
                    .borrow()
                    .audio_tracks
                    .get(&r)
                    .ok_or_else(|| error("AUDIO_RELEASED", "stop"))?
                    .0;
                self.audio()?.stop(id)?;
                self.state.borrow_mut().audio_tracks.get_mut(&r).unwrap().1 = false;
                Value::Null
            }
            ("release", "()V") if platform == "Landroid/media/AudioTrack;" => {
                let old = self.state.borrow_mut().audio_tracks.remove(&r);
                if let Some((id, _)) = old {
                    self.audio()?.close(id)?;
                }
                Value::Null
            }
            ("write", "([SII)I" | "([BII)I") if platform == "Landroid/media/AudioTrack;" => {
                let id = self
                    .state
                    .borrow()
                    .audio_tracks
                    .get(&r)
                    .ok_or_else(|| error("AUDIO_RELEASED", "write"))?
                    .0;
                let shorts = sig.starts_with("([S");
                let pcm = crate::audio::samples(
                    vm,
                    obj(&args[0])?,
                    int(args, 1)?,
                    int(args, 2)?,
                    shorts,
                )?;
                let frames = self.audio()?.write(id, &pcm)?;
                Value::Int((frames * if shorts { 2 } else { 4 }) as i32)
            }
            ("getAssets", "()Landroid/content/res/AssetManager;") => {
                Value::Obj(shell(vm, "Landroid/content/res/AssetManager;")?)
            }
            ("open", "(Ljava/lang/String;)Ljava/io/InputStream;")
            | ("open", "(Ljava/lang/String;I)Ljava/io/InputStream;")
                if platform == "Landroid/content/res/AssetManager;" =>
            {
                let id = self.asset_open(&text(vm, &args[0])?)?;
                let stream = shell(vm, "Landroid/content/res/AssetManager$AssetInputStream;")?;
                self.state.borrow_mut().asset_streams.insert(stream, id);
                Value::Obj(stream)
            }
            ("read", "()I") if self.state.borrow().asset_streams.contains_key(&r) => {
                let id = self.state.borrow().asset_streams[&r];
                let mut byte = [0];
                Value::Int(if self.asset_read(id, &mut byte)? == 0 {
                    -1
                } else {
                    byte[0] as i32
                })
            }
            ("read", "([B)I" | "([BII)I") if self.state.borrow().asset_streams.contains_key(&r) => {
                let array = obj(&args[0])?;
                let length = vm
                    .heap
                    .as_array_elems(array)
                    .map_err(|e| error("INVALID_ARRAY", e))?
                    .0
                    .len();
                let (off, len) = if args.len() == 1 {
                    (0, length)
                } else {
                    (int(args, 1)? as usize, int(args, 2)? as usize)
                };
                if off > length || len > length - off {
                    return Err(Throwable::new(
                        "Ljava/lang/IndexOutOfBoundsException;",
                        "asset read",
                    )
                    .into());
                }
                let mut bytes = vec![0; len.min(64 << 10)];
                let id = self.state.borrow().asset_streams[&r];
                let n = self.asset_read(id, &mut bytes)?;
                let values = vm
                    .heap
                    .array_elems_mut(array)
                    .map_err(|e| error("INVALID_ARRAY", e))?;
                for (i, b) in bytes[..n].iter().enumerate() {
                    values[off + i] = Value::Int(*b as i8 as i32);
                }
                Value::Int(if n == 0 && len != 0 { -1 } else { n as i32 })
            }
            ("close", "()V") if self.state.borrow().asset_streams.contains_key(&r) => {
                let id = self.state.borrow_mut().asset_streams.remove(&r).unwrap();
                self.asset_close(id);
                Value::Null
            }
            (
                "<init>",
                "(Landroid/content/Context;)V"
                | "(Landroid/content/Context;Landroid/util/AttributeSet;)V",
            ) if Self::is_view(vm, &platform) => {
                let context = obj(&args[0])?;
                self.bind_view_context(vm, r, context)?;
                Value::Null
            }
            (
                "<init>",
                "()V"
                | "(Landroid/content/Context;)V"
                | "(Landroid/content/Context;Landroid/util/AttributeSet;)V"
                | "(I)V",
            ) if !platform.contains("Intent") => Value::Null,
            ("onCreate", "(Landroid/os/Bundle;)V")
                if platform == "Landroid/app/Activity;"
                    || platform == "Landroid/app/NativeActivity;" =>
            {
                let application = self.state.borrow().application.ok_or_else(|| {
                    error("APPLICATION_STATE", "Activity has no attached Application")
                })?;
                self.services.lifecycle_observers.base(
                    vm,
                    application,
                    r,
                    services::application_lifecycle::Event::Created,
                    args,
                )?;
                Value::Null
            }
            ("onSaveInstanceState", "(Landroid/os/Bundle;)V")
                if platform == "Landroid/app/Activity;"
                    || platform == "Landroid/app/NativeActivity;" =>
            {
                let application = self.state.borrow().application.ok_or_else(|| {
                    error("APPLICATION_STATE", "Activity has no attached Application")
                })?;
                self.services.lifecycle_observers.base(
                    vm,
                    application,
                    r,
                    services::application_lifecycle::Event::Save,
                    args,
                )?;
                Value::Null
            }
            (
                "onCreate" | "onStart" | "onRestart" | "onResume" | "onPause" | "onStop"
                | "onDestroy",
                "()V",
            ) if platform == "Landroid/app/Activity;"
                || platform == "Landroid/app/NativeActivity;"
                || platform == "Landroid/app/Application;" =>
            {
                if platform != "Landroid/app/Application;" {
                    if let Some(event) =
                        services::application_lifecycle::Event::from_method(name, sig)
                    {
                        let application = self.state.borrow().application.ok_or_else(|| {
                            error("APPLICATION_STATE", "Activity has no attached Application")
                        })?;
                        self.services
                            .lifecycle_observers
                            .base(vm, application, r, event, args)?;
                    }
                }
                Value::Null
            }
            ("getPackageName", "()Ljava/lang/String;") => {
                string(vm, self.apk.manifest.package.clone())?
            }
            ("getApplicationContext", "()Landroid/content/Context;")
            | ("getApplication", "()Landroid/app/Application;") => Value::Obj(
                self.state
                    .borrow()
                    .application
                    .ok_or_else(|| error("APPLICATION_STATE", "not created"))?,
            ),
            ("getResources", "()Landroid/content/res/Resources;") => {
                let old = self.state.borrow().resources;
                Value::Obj(if let Some(r) = old {
                    r
                } else {
                    let r = shell(vm, "Landroid/content/res/Resources;")?;
                    self.state.borrow_mut().resources = Some(r);
                    r
                })
            }
            ("getString", "(I)Ljava/lang/String;") | ("getText", "(I)Ljava/lang/CharSequence;") => {
                string(vm, self.resource_string(int(args, 0)? as u32)?)?
            }
            ("getLayoutInflater", "()Landroid/view/LayoutInflater;")
                if vm
                    .cp
                    .is_subtype(vm.heap.class_of(r)?, "Landroid/app/Activity;") =>
            {
                let inflater = shell(vm, "Landroid/view/LayoutInflater;")?;
                write(vm, inflater, "context", Value::Obj(r))?;
                Value::Obj(inflater)
            }
            ("attachBaseContext", "(Landroid/content/Context;)V") => {
                obj(&args[0])?;
                write(vm, r, "base_context", args[0].clone())?;
                Value::Null
            }
            ("getBaseContext", "()Landroid/content/Context;") => {
                vm.heap.get_field(r, "base_context")?
            }
            ("setTitle", "(Ljava/lang/CharSequence;)V") => {
                self.state.borrow_mut().title = text(vm, &args[0])?;
                Value::Null
            }
            ("setContentView", "(I)V") => {
                let root = self.inflate(vm, int(args, 0)? as u32, r)?;
                let index = self.content_activity(r)?;
                self.state.borrow_mut().activities[index].root = Some(root);
                Value::Null
            }
            ("setContentView", "(Landroid/view/View;)V") => {
                let root = obj(&args[0])?;
                if !self.state.borrow().views.contains_key(&root) {
                    return Err(error("INVALID_VIEW", "setContentView"));
                }
                let index = self.content_activity(r)?;
                self.state.borrow_mut().activities[index].root = Some(root);
                Value::Null
            }
            ("findViewById", "(I)Landroid/view/View;") => {
                let root = if self.state.borrow().views.contains_key(&r) {
                    Some(r)
                } else {
                    let index = self.content_activity(r)?;
                    self.state.borrow().activities[index].root
                };
                root.and_then(|r| self.find(r, int(args, 0).unwrap_or(-1)))
                    .map(Value::Obj)
                    .unwrap_or(Value::Null)
            }
            ("getIntent", "()Landroid/content/Intent;") => self.state.borrow().activities
                [self.content_activity(r)?]
            .intent
            .map(Value::Obj)
            .unwrap_or(Value::Null),
            ("finish" | "onBackPressed", "()V") => {
                self.state.borrow_mut().actions.push_back(Action::Finish(r));
                Value::Null
            }
            ("startActivity", "(Landroid/content/Intent;)V") => {
                let intent = obj(&args[0])?;
                let class = self
                    .state
                    .borrow()
                    .intents
                    .get(&intent)
                    .cloned()
                    .ok_or_else(|| error("INVALID_INTENT", "startActivity"))?;
                if class.is_empty() {
                    return Err(error("API_UNAVAILABLE", "implicit intent resolution"));
                }
                self.state
                    .borrow_mut()
                    .actions
                    .push_back(Action::Start(class, intent));
                Value::Null
            }
            ("<init>", "(Landroid/content/Context;Ljava/lang/Class;)V")
                if platform == "Landroid/content/Intent;" =>
            {
                let class = obj(&args[1])?;
                let value = vm.heap.get_field(class, "class_name")?;
                self.state.borrow_mut().intents.insert(r, text(vm, &value)?);
                Value::Null
            }
            ("<init>", "()V") if platform == "Landroid/content/Intent;" => Value::Null,
            ("post", "(Ljava/lang/Runnable;)Z") | ("postDelayed", "(Ljava/lang/Runnable;J)Z") => {
                let runnable = obj(&args[0])?;
                let delay = if args.len() > 1 {
                    args[1].as_long()?.max(0) as u64
                } else {
                    0
                };
                let mut s = self.state.borrow_mut();
                if s.tasks.len() >= 4096 {
                    return Err(error("LOOPER_LIMIT", "4096 queued callbacks"));
                }
                s.serial += 1;
                let order = s.serial;
                s.tasks.push(Task {
                    due: self.now().saturating_add(delay),
                    order,
                    handler: r,
                    runnable,
                });
                Value::Int(1)
            }
            ("removeCallbacks", "(Ljava/lang/Runnable;)V") => {
                let runnable = obj(&args[0])?;
                self.state
                    .borrow_mut()
                    .tasks
                    .retain(|t| t.handler != r || t.runnable != runnable);
                Value::Null
            }
            ("<init>", "(Landroid/os/Looper;)V") if platform == "Landroid/os/Handler;" => {
                obj(&args[0])?;
                Value::Null
            }
            ("inflate", "(ILandroid/view/ViewGroup;)Landroid/view/View;") => {
                let context = obj(&vm.heap.get_field(r, "context")?)?;
                let root = self.inflate(vm, int(args, 0)? as u32, context)?;
                if let Some(parent) = args[1].as_ref()? {
                    self.add_child(parent, root)?;
                    Value::Obj(parent)
                } else {
                    Value::Obj(root)
                }
            }
            _ => return self.call_object(vm, &platform, name, sig, r, args),
        };
        Ok(Some(value))
    }
    fn roots(&self) -> Vec<ObjRef> {
        let s = self.state.borrow();
        let mut roots = s.roots.clone();
        roots.extend(self.services.roots());
        roots.extend(s.application);
        roots.extend(s.resources);
        roots.extend(s.looper);
        for a in &s.activities {
            roots.push(a.object);
            roots.extend(a.intent);
            roots.extend(a.saved_state);
            if let Some(r) = a.root {
                let mut nodes = vec![r];
                while let Some(r) = nodes.pop() {
                    roots.push(r);
                    if let Some(v) = s.views.get(&r) {
                        roots.extend(v.listener);
                        roots.extend(v.touch_listener);
                        nodes.extend(v.children.iter().copied())
                    }
                }
            }
        }
        roots.extend(s.tasks.iter().flat_map(|t| [t.handler, t.runnable]));
        for action in &s.actions {
            match action {
                Action::Start(_, r) | Action::Finish(r) => roots.push(*r),
            }
        }
        roots
    }
}
fn display(vm: &Engine, value: &Value) -> Result<String> {
    Ok(match value {
        Value::Obj(r) => vm.heap.as_str(*r)?.to_string(),
        Value::Null => "null".into(),
        Value::Int(n) => n.to_string(),
        Value::Long(n) => n.to_string(),
        Value::Float(n) => n.to_string(),
        Value::Double(n) => n.to_string(),
        _ => return Err(error("INVALID_ARGUMENT", "display value")),
    })
}
impl Android {
    fn add_child(&self, parent: ObjRef, child: ObjRef) -> Result<()> {
        let mut s = self.state.borrow_mut();
        if parent == child || !s.views.contains_key(&child) {
            return Err(error("INVALID_VIEW", "addView child"));
        }
        let mut pending = vec![child];
        while let Some(r) = pending.pop() {
            if r == parent {
                return Err(error("VIEW_CYCLE", "addView ancestor"));
            }
            if let Some(v) = s.views.get(&r) {
                pending.extend(v.children.iter().copied());
            }
        }
        if s.views.values().any(|v| v.children.contains(&child)) {
            return Err(Throwable::new(
                "Ljava/lang/IllegalStateException;",
                "child already has a parent",
            )
            .into());
        }
        s.views
            .get_mut(&parent)
            .ok_or_else(|| error("INVALID_VIEW", "addView parent"))?
            .children
            .push(child);
        Ok(())
    }
    fn call_object(
        &self,
        vm: &mut Engine,
        platform: &str,
        name: &str,
        sig: &str,
        r: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if self.state.borrow().builders.contains_key(&r) {
            let v = match name {
                "length" if sig == "()I" => {
                    Value::Int(self.state.borrow().builders[&r].encode_utf16().count() as i32)
                }
                "<init>" if sig == "()V" || sig == "(I)V" => Value::Null,
                "<init>" if sig == "(Ljava/lang/String;)V" => {
                    self.state
                        .borrow_mut()
                        .builders
                        .insert(r, text(vm, &args[0])?);
                    Value::Null
                }
                "append" if args.len() == 1 => {
                    // DEX stores char, boolean and int arguments in the same
                    // register kind; the method descriptor selects their Java
                    // textual representation.
                    let s = match sig {
                        "(C)Ljava/lang/StringBuilder;" | "(C)Ljava/lang/StringBuffer;" => {
                            char::from_u32(int(args, 0)? as u16 as u32)
                                .ok_or_else(|| {
                                    error(
                                        "API_UNAVAILABLE",
                                        "builder append of an isolated UTF-16 surrogate",
                                    )
                                })?
                                .to_string()
                        }
                        "(Z)Ljava/lang/StringBuilder;" | "(Z)Ljava/lang/StringBuffer;" => {
                            (int(args, 0)? != 0).to_string()
                        }
                        _ => display(vm, &args[0])?,
                    };
                    self.state
                        .borrow_mut()
                        .builders
                        .get_mut(&r)
                        .unwrap()
                        .push_str(&s);
                    Value::Obj(r)
                }
                "toString" if sig == "()Ljava/lang/String;" => {
                    string(vm, self.state.borrow().builders[&r].clone())?
                }
                _ => return Ok(None),
            };
            return Ok(Some(v));
        }
        if platform.ends_with("LayoutParams;") {
            return match (name, sig) {
                ("<init>", "(II)V") => {
                    write(vm, r, "width", Value::Int(int(args, 0)?))?;
                    write(vm, r, "height", Value::Int(int(args, 1)?))?;
                    Ok(Some(Value::Null))
                }
                _ => Ok(None),
            };
        }
        if platform == "Landroid/view/MotionEvent;" {
            let (action, x, y, raw_x, raw_y, time) = self
                .state
                .borrow()
                .motion
                .get(&r)
                .copied()
                .ok_or_else(|| error("INPUT_EVENT_EXPIRED", "MotionEvent"))?;
            return Ok(match (name, sig) {
                ("getAction" | "getActionMasked", "()I") => Some(Value::Int(action)),
                ("getX", "()F") => Some(Value::Float(x)),
                ("getY", "()F") => Some(Value::Float(y)),
                ("getRawX", "()F") => Some(Value::Float(raw_x)),
                ("getRawY", "()F") => Some(Value::Float(raw_y)),
                ("getPointerCount", "()I") => Some(Value::Int(1)),
                ("getActionIndex", "()I") => Some(Value::Int(0)),
                ("getEventTime", "()J") => Some(Value::Long(time as i64)),
                ("getPointerId", "(I)I") if int(args, 0)? == 0 => Some(Value::Int(0)),
                ("getX", "(I)F") if int(args, 0)? == 0 => Some(Value::Float(x)),
                ("getY", "(I)F") if int(args, 0)? == 0 => Some(Value::Float(y)),
                ("getPointerId", "(I)I") | ("getX" | "getY", "(I)F") => {
                    return Err(Throwable::new(
                        "Ljava/lang/IllegalArgumentException;",
                        "single-pointer MotionEvent index",
                    )
                    .into())
                }
                _ => None,
            });
        }
        if platform == "Landroid/graphics/Paint;" {
            let mut s = self.state.borrow_mut();
            let p = s
                .paints
                .get_mut(&r)
                .ok_or_else(|| error("INVALID_PAINT", "Paint"))?;
            let v = match (name, sig) {
                ("setColor", "(I)V") => {
                    p.0 = int(args, 0)? as u32;
                    Value::Null
                }
                ("getColor", "()I") => Value::Int(p.0 as i32),
                ("setTextSize", "(F)V") => {
                    p.1 = args[0].as_float()?.clamp(1., 256.);
                    Value::Null
                }
                ("getTextSize", "()F") => Value::Float(p.1),
                _ => return Ok(None),
            };
            return Ok(Some(v));
        }
        if platform == "Landroid/graphics/Canvas;" {
            if let Some(bitmap) = self.services.bitmaps.draw(self, vm, name, sig, args)? {
                let mut state = self.state.borrow_mut();
                let commands = state
                    .canvases
                    .get_mut(&r)
                    .ok_or_else(|| error("CANVAS_EXPIRED", "draw outside onDraw"))?;
                if commands.len() >= 4096 {
                    return Err(error("CANVAS_BUDGET", "4096 recorded commands per onDraw"));
                }
                commands.push(Draw::Bitmap(bitmap));
                return Ok(Some(Value::Null));
            }
            let paint_index = match (name, sig) {
                ("drawRect", "(FFFFLandroid/graphics/Paint;)V") => Some(4),
                ("drawCircle", "(FFFLandroid/graphics/Paint;)V") => Some(3),
                ("drawText", "(Ljava/lang/String;FFLandroid/graphics/Paint;)V") => Some(3),
                _ => None,
            };
            let paint = if let Some(index) = paint_index {
                self.state
                    .borrow()
                    .paints
                    .get(&obj(&args[index])?)
                    .copied()
                    .ok_or_else(|| error("INVALID_PAINT", "Canvas paint"))?
            } else {
                (0, 0.)
            };
            let command = match (name, sig) {
                ("drawColor", "(I)V") => Draw::Color(int(args, 0)? as u32),
                ("drawRect", "(FFFFLandroid/graphics/Paint;)V") => Draw::Rect(
                    [
                        args[0].as_float()?,
                        args[1].as_float()?,
                        args[2].as_float()?,
                        args[3].as_float()?,
                    ],
                    paint.0,
                ),
                ("drawCircle", "(FFFLandroid/graphics/Paint;)V") => Draw::Circle(
                    args[0].as_float()?,
                    args[1].as_float()?,
                    args[2].as_float()?,
                    paint.0,
                ),
                ("drawText", "(Ljava/lang/String;FFLandroid/graphics/Paint;)V") => Draw::Text(
                    text(vm, &args[0])?,
                    args[1].as_float()?,
                    args[2].as_float()?,
                    paint.1,
                    paint.0,
                ),
                _ => return Ok(None),
            };
            self.state
                .borrow_mut()
                .canvases
                .get_mut(&r)
                .ok_or_else(|| error("CANVAS_EXPIRED", "draw outside onDraw"))?
                .push(command);
            return Ok(Some(Value::Null));
        }
        if !self.state.borrow().views.contains_key(&r) {
            return Ok(None);
        }
        let value = match (name, sig) {
            ("getContext", "()Landroid/content/Context;") => Value::Obj(
                self.state.borrow().views[&r]
                    .context
                    .ok_or_else(|| error("API_UNAVAILABLE", "View Context is not initialized"))?,
            ),
            ("setId", "(I)V") => {
                self.state.borrow_mut().views.get_mut(&r).unwrap().id = int(args, 0)?;
                Value::Null
            }
            ("getId", "()I") => Value::Int(self.state.borrow().views[&r].id),
            ("setText", "(Ljava/lang/CharSequence;)V") => {
                let s = if args[0].as_ref()?.is_none() {
                    String::new()
                } else {
                    text(vm, &args[0])?
                };
                self.state.borrow_mut().views.get_mut(&r).unwrap().text = s;
                Value::Null
            }
            ("setText", "(I)V") => {
                let s = self.resource_string(int(args, 0)? as u32)?;
                self.state.borrow_mut().views.get_mut(&r).unwrap().text = s;
                Value::Null
            }
            ("getText", "()Ljava/lang/CharSequence;") => {
                string(vm, self.state.borrow().views[&r].text.clone())?
            }
            ("setTextSize", "(F)V") => {
                self.state.borrow_mut().views.get_mut(&r).unwrap().text_size =
                    args[0].as_float()?.clamp(1., 256.);
                Value::Null
            }
            ("setTextColor", "(I)V") => {
                self.state
                    .borrow_mut()
                    .views
                    .get_mut(&r)
                    .unwrap()
                    .text_color = int(args, 0)? as u32;
                Value::Null
            }
            ("setBackgroundColor", "(I)V") => {
                self.state.borrow_mut().views.get_mut(&r).unwrap().color = int(args, 0)? as u32;
                Value::Null
            }
            ("setOrientation", "(I)V") => {
                self.state
                    .borrow_mut()
                    .views
                    .get_mut(&r)
                    .unwrap()
                    .orientation = int(args, 0)?;
                Value::Null
            }
            ("setOnClickListener", "(Landroid/view/View$OnClickListener;)V") => {
                let listener = args[0].as_ref()?;
                let mut s = self.state.borrow_mut();
                let v = s.views.get_mut(&r).unwrap();
                v.listener = listener;
                v.clickable = listener.is_some();
                Value::Null
            }
            ("setOnTouchListener", "(Landroid/view/View$OnTouchListener;)V") => {
                self.state
                    .borrow_mut()
                    .views
                    .get_mut(&r)
                    .unwrap()
                    .touch_listener = args[0].as_ref()?;
                Value::Null
            }
            ("setClickable" | "setEnabled", "(Z)V") => {
                let b = int(args, 0)? != 0;
                let mut s = self.state.borrow_mut();
                let v = s.views.get_mut(&r).unwrap();
                if name == "setClickable" {
                    v.clickable = b
                } else {
                    v.enabled = b
                }
                Value::Null
            }
            ("isEnabled", "()Z") => Value::Int(self.state.borrow().views[&r].enabled as i32),
            ("performClick", "()Z") => Value::Int(self.click_inner(vm, r)? as i32),
            ("setVisibility", "(I)V") => {
                self.state.borrow_mut().views.get_mut(&r).unwrap().visible = int(args, 0)? == 0;
                Value::Null
            }
            ("getWidth" | "getHeight", "()I") => Value::Int(
                self.state.borrow().views[&r].bounds[if name == "getWidth" { 2 } else { 3 }],
            ),
            ("setLayoutParams", "(Landroid/view/ViewGroup$LayoutParams;)V") => {
                let p = obj(&args[0])?;
                let w = vm.heap.get_field(p, "width")?.as_int()?;
                let h = vm.heap.get_field(p, "height")?.as_int()?;
                let mut s = self.state.borrow_mut();
                let v = s.views.get_mut(&r).unwrap();
                v.width = w;
                v.height = h;
                Value::Null
            }
            ("addView", "(Landroid/view/View;)V") | ("addView", "(Landroid/view/View;II)V") => {
                let child = obj(&args[0])?;
                if args.len() == 3 {
                    let mut s = self.state.borrow_mut();
                    let v = s
                        .views
                        .get_mut(&child)
                        .ok_or_else(|| error("INVALID_VIEW", "addView"))?;
                    v.width = int(args, 1)?;
                    v.height = int(args, 2)?;
                }
                self.add_child(r, child)?;
                Value::Null
            }
            ("removeAllViews", "()V") => {
                self.state
                    .borrow_mut()
                    .views
                    .get_mut(&r)
                    .unwrap()
                    .children
                    .clear();
                Value::Null
            }
            ("invalidate" | "requestLayout", "()V") => Value::Null, // draw/layout is scheduled every platform frame
            ("onDraw", "(Landroid/graphics/Canvas;)V") if class_is_base_view(platform) => {
                Value::Null
            }
            ("onTouchEvent", "(Landroid/view/MotionEvent;)Z") => Value::Int(0),
            _ => return Ok(None),
        };
        Ok(Some(value))
    }
}
fn class_is_base_view(class: &str) -> bool {
    class == "Landroid/view/View;"
}
