//! GLSurfaceView scheduling and EGL metadata are backed by one real renderer
//! thread/context, supplied by the platform after Activity.onCreate completes.
use super::*;

#[derive(Debug, Clone, Copy)]
pub struct GraphicsCaps {
    pub width: i32,
    pub height: i32,
    pub gles_version: u32,
    /// Runtime logical density policy, supplied by the display adapter.
    pub density: f32,
    pub density_dpi: i32,
    /// Android Surface rotation (0..3) relative to this display's natural axes.
    pub rotation: i32,
}
pub struct EglConfigs {
    pub configs: Vec<u64>,
    pub count: usize,
}
pub trait EglConfigBackend {
    fn choose_config(
        &self,
        display: u64,
        attributes: &[i32],
        capacity: usize,
    ) -> Result<EglConfigs>;
    fn get_config_attrib(&self, display: u64, config: u64, attribute: i32) -> Result<i32>;
    /// Change actual buffer/presentation/input coordinates before any surface
    /// starts. Return the resulting logical display, or leave it unchanged on error.
    fn set_display_rotation(&self, _rotation: i32) -> Result<GraphicsCaps> {
        Err(error("API_UNAVAILABLE", "display rotation backend"))
    }
    fn set_keep_screen_on(&self, _enabled: bool) -> Result<()> {
        Err(error("API_UNAVAILABLE", "display power inhibition"))
    }
}
#[derive(Debug, Clone)]
pub struct GlSurfaceRequest {
    pub view: ObjRef,
    pub renderer: ObjRef,
    pub client_version: i32,
    pub config: [i32; 6],
    pub config_chooser: Option<ObjRef>,
    pub render_mode: i32,
    pub paused: bool,
    pub dirty: bool,
    pub queued_count: usize,
    pub preserve_context_on_pause: bool,
}
#[derive(Debug, Clone, Copy)]
pub enum GlEvent {
    SurfaceCreated {
        context_token: u64,
        config_token: u64,
    },
    SurfaceChanged {
        width: i32,
        height: i32,
    },
    DrawFrame,
    DrainQueue,
    SurfaceDestroyed,
}
#[derive(Default)]
pub(crate) struct GlSurfaces {
    caps: RefCell<Option<GraphicsCaps>>,
    backend: RefCell<Option<Rc<dyn EglConfigBackend>>>,
    views: RefCell<BTreeMap<ObjRef, Surface>>,
    proxies: RefCell<BTreeMap<ObjRef, Proxy>>,
    singletons: RefCell<BTreeMap<String, ObjRef>>,
    system_ui: RefCell<BTreeMap<ObjRef, i32>>,
    window_flags: RefCell<BTreeMap<ObjRef, i32>>,
    orientations: RefCell<BTreeMap<ObjRef, i32>>,
    callback_depth: core::cell::Cell<u32>,
    display_started: core::cell::Cell<bool>,
}
struct CallbackScope<'a>(&'a core::cell::Cell<u32>);
impl Drop for CallbackScope<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}
const GL_VIEW: &str = "Landroid/opengl/GLSurfaceView;";
const EGL_API: &str = "Ljavax/microedition/khronos/egl/EGL10;";
const EGL_DISPLAY: &str = "Ljavax/microedition/khronos/egl/EGLDisplay;";
const EGL_CONFIG: &str = "Ljavax/microedition/khronos/egl/EGLConfig;";
const GL: &str = "Ljavax/microedition/khronos/opengles/GL10;";
const NONE: i32 = 0x3038;
#[derive(Clone)]
struct Proxy {
    class: &'static str,
    display: u64,
    token: u64,
}
struct Surface {
    renderer: Option<ObjRef>,
    chooser: Option<ObjRef>,
    config: [i32; 6],
    client_version: i32,
    render_mode: i32,
    paused: bool,
    preserve: bool,
    requested: u64,
    drawn: u64,
    queue: VecDeque<ObjRef>,
    gl: Option<ObjRef>,
    egl: Option<ObjRef>,
    display: Option<ObjRef>,
    chosen_config: Option<ObjRef>,
    created: bool,
    sized: bool,
}
impl Default for Surface {
    fn default() -> Self {
        Self {
            renderer: None,
            chooser: None,
            config: [8, 8, 8, 0, 16, 0],
            client_version: 1,
            render_mode: 1,
            paused: false,
            preserve: false,
            requested: 1,
            drawn: 0,
            queue: VecDeque::new(),
            gl: None,
            egl: None,
            display: None,
            chosen_config: None,
            created: false,
            sized: false,
        }
    }
}
fn illegal(message: &str) -> VmExit {
    Throwable::new("Ljava/lang/IllegalStateException;", message).into()
}
fn invalid(message: &str) -> VmExit {
    Throwable::new("Ljava/lang/IllegalArgumentException;", message).into()
}
impl GlSurfaces {
    fn enter_callback(&self) -> CallbackScope<'_> {
        self.callback_depth.set(self.callback_depth.get() + 1);
        CallbackScope(&self.callback_depth)
    }
    pub(crate) fn in_callback(&self) -> bool {
        self.callback_depth.get() != 0
    }
    pub(crate) fn attach(&self, vm: &mut Engine, class: &str, object: ObjRef) -> Result<bool> {
        if vm.cp.is_subtype(class, GL_VIEW) {
            if self.views.borrow().len() >= 16 {
                return Err(error("GL_VIEW_LIMIT", "16 view instances"));
            }
            self.views.borrow_mut().insert(object, Surface::default());
            return Ok(false); // Android also attaches the actual View/layout state.
        }
        if class == "Landroid/util/DisplayMetrics;" {
            for name in ["widthPixels", "heightPixels", "densityDpi"] {
                write(vm, object, name, Value::Int(0))?;
            }
            for name in ["density", "scaledDensity", "xdpi", "ydpi"] {
                write(vm, object, name, Value::Float(0.))?;
            }
            return Ok(true);
        }
        if class == "Landroid/content/res/Configuration;" {
            for field in [
                "orientation",
                "screenWidthDp",
                "screenHeightDp",
                "smallestScreenWidthDp",
                "densityDpi",
            ] {
                write(vm, object, field, Value::Int(0))?;
            }
            // API26 Configuration() calls unset(), including fontScale=0.
            write(vm, object, "fontScale", Value::Float(0.))?;
            return Ok(true);
        }
        Ok(matches!(
            class,
            EGL_API
                | EGL_DISPLAY
                | EGL_CONFIG
                | GL
                | "Landroid/view/Window;"
                | "Landroid/view/WindowManager;"
                | "Landroid/view/Display;"
                | "Landroid/app/ActivityManager;"
                | "Landroid/content/pm/ConfigurationInfo;"
                | "Landroid/content/res/Configuration;"
                | "Landroid/media/AudioManager;"
        ))
    }
    fn backend(&self) -> Result<Rc<dyn EglConfigBackend>> {
        self.backend
            .borrow()
            .clone()
            .ok_or_else(|| error("API_UNAVAILABLE", "platform EGL backend"))
    }
    fn proxy(
        &self,
        vm: &mut Engine,
        class: &'static str,
        display: u64,
        token: u64,
    ) -> Result<ObjRef> {
        if let Some((&object, _)) = self
            .proxies
            .borrow()
            .iter()
            .find(|(_, p)| p.class == class && p.display == display && p.token == token)
        {
            return Ok(object);
        }
        if self.proxies.borrow().len() >= 256 {
            return Err(error("EGL_PROXY_LIMIT", "256 EGL/GL objects"));
        }
        let object = shell(vm, class)?;
        self.proxies.borrow_mut().insert(
            object,
            Proxy {
                class,
                display,
                token,
            },
        );
        Ok(object)
    }
    fn singleton(&self, vm: &mut Engine, class: &str) -> Result<ObjRef> {
        if let Some(r) = self.singletons.borrow().get(class) {
            return Ok(*r);
        }
        let r = shell(vm, class)?;
        self.singletons.borrow_mut().insert(class.into(), r);
        Ok(r)
    }
    fn metrics(&self, android: &Android) -> GraphicsCaps {
        self.caps.borrow().unwrap_or(GraphicsCaps {
            width: android.width,
            height: android.height,
            gles_version: 0,
            density: 1.,
            density_dpi: 160,
            rotation: 0,
        })
    }
    pub(crate) fn caps(&self) -> Option<GraphicsCaps> {
        *self.caps.borrow()
    }
    pub(crate) fn software_size(&self, android: &Android) -> (i32, i32) {
        self.caps().map_or((android.width, android.height), |c| {
            (
                libm::roundf(c.width as f32 / c.density) as i32,
                libm::roundf(c.height as f32 / c.density) as i32,
            )
        })
    }
    pub(crate) fn mark_display_started(&self) {
        self.display_started.set(true);
    }
    pub(crate) fn dimensions(&self, view: ObjRef) -> Option<(i32, i32)> {
        if !self.views.borrow().contains_key(&view) {
            return None;
        }
        self.caps.borrow().map(|c| (c.width, c.height))
    }
    fn validate_caps(caps: GraphicsCaps) -> Result<()> {
        if caps.width <= 0
            || caps.height <= 0
            || !caps.density.is_finite()
            || caps.density <= 0.
            || caps.density_dpi <= 0
            || caps.gles_version < 0x20000
            || !(0..=3).contains(&caps.rotation)
            || libm::roundf(caps.width as f32 / caps.density) < 1.
            || libm::roundf(caps.height as f32 / caps.density) < 1.
            || libm::roundf(caps.width as f32 / caps.density) > 1920.
            || libm::roundf(caps.height as f32 / caps.density) > 1920.
        {
            return Err(error(
                "INVALID_GRAPHICS_CAPS",
                "display dimensions/density/GLES capability",
            ));
        }
        Ok(())
    }
    pub(crate) fn bind_caps(&self, caps: GraphicsCaps) -> Result<()> {
        Self::validate_caps(caps)?;
        if self.display_started.get() {
            return Err(error(
                "DISPLAY_ACTIVE",
                "display capabilities are already in use",
            ));
        }
        *self.caps.borrow_mut() = Some(caps);
        Ok(())
    }
    fn request_orientation(
        &self,
        android: &Android,
        activity: ObjRef,
        requested: i32,
    ) -> Result<()> {
        let current = self.metrics(android);
        let natural_landscape = if current.rotation % 2 == 0 {
            current.width >= current.height
        } else {
            current.height >= current.width
        };
        let landscape = if natural_landscape { 0 } else { 1 };
        let portrait = if natural_landscape { 1 } else { 0 };
        // No orientation sensor is exposed by this backend. Sensor/user modes
        // retain the actual orientation unless they constrain its shape.
        let rotation = match requested {
            -1 | 2 | 4 | 5 | 10 | 13 | 14 => current.rotation,
            0 => landscape,
            1 => portrait,
            6 | 11 => {
                if current.width >= current.height {
                    current.rotation
                } else {
                    landscape
                }
            }
            7 | 12 => {
                if current.height >= current.width {
                    current.rotation
                } else {
                    portrait
                }
            }
            8 => (landscape + 2) % 4,
            9 => (portrait + 2) % 4,
            3 => {
                return Err(error(
                    "API_UNAVAILABLE",
                    "Activity.setRequestedOrientation(BEHIND=3): activity display inheritance",
                ))
            }
            _ => {
                return Err(error(
                    "API_UNAVAILABLE",
                    format!(
                        "Activity.setRequestedOrientation({requested}): unknown API26 orientation"
                    ),
                ))
            }
        };
        if rotation != current.rotation {
            if self.display_started.get() {
                return Err(error("DISPLAY_ORIENTATION_UNAVAILABLE", format!("Activity.setRequestedOrientation({requested}): active display rotation {} -> {rotation} requires surface/configuration lifecycle recreation",current.rotation)));
            }
            let next = self.backend()?.set_display_rotation(rotation)?;
            Self::validate_caps(next)?;
            let expected = if (rotation - current.rotation).abs() % 2 == 0 {
                (current.width, current.height)
            } else {
                (current.height, current.width)
            };
            if next.rotation != rotation || (next.width, next.height) != expected {
                return Err(error(
                    "INVALID_DISPLAY_RESULT",
                    "backend did not return the requested rotation and logical dimensions",
                ));
            }
            *self.caps.borrow_mut() = Some(next);
        }
        self.orientations.borrow_mut().insert(activity, requested);
        Ok(())
    }
    pub(crate) fn bind_backend(&self, backend: Rc<dyn EglConfigBackend>) {
        *self.backend.borrow_mut() = Some(backend);
    }
    pub(crate) fn requests(&self, android: &Android) -> Result<Vec<GlSurfaceRequest>> {
        let root = android.active_root();
        let visible = android.views();
        let states = self.views.borrow();
        let mut requests = Vec::new();
        for (&view, state) in states.iter() {
            let Some(renderer) = state.renderer else {
                continue;
            };
            if !visible.iter().any(|v| v.object == view && v.visible) {
                continue;
            }
            if root != Some(view) || visible.len() != 1 {
                return Err(error("GL_COMPOSITION_UNAVAILABLE","only one full-screen GLSurfaceView is supported; software overlays need a compositor"));
            }
            requests.push(GlSurfaceRequest {
                view,
                renderer,
                client_version: state.client_version,
                config: state.config,
                config_chooser: state.chooser,
                render_mode: state.render_mode,
                paused: state.paused,
                dirty: state.requested != state.drawn,
                queued_count: state.queue.len(),
                preserve_context_on_pause: state.preserve,
            });
        }
        Ok(requests)
    }
    pub(crate) fn choose_config(&self, vm: &mut Engine, view: ObjRef, display: u64) -> Result<u64> {
        self.mark_display_started();
        let _scope = self.enter_callback();
        if display == 0 {
            return Err(invalid("null EGL display"));
        }
        let (chooser, config, client) = {
            let s = self.views.borrow();
            let s = s
                .get(&view)
                .ok_or_else(|| illegal("unknown GLSurfaceView"))?;
            (s.chooser, s.config, s.client_version)
        };
        if client != 2 {
            return Err(error(
                "API_UNAVAILABLE",
                "initial EGL backend supports GLES 2 contexts",
            ));
        }
        let egl = self.proxy(vm, EGL_API, display, display)?;
        let display_object = self.proxy(vm, EGL_DISPLAY, display, display)?;
        {
            let mut s = self.views.borrow_mut();
            let s = s.get_mut(&view).unwrap();
            s.egl = Some(egl);
            s.display = Some(display_object);
        }
        let token = if let Some(chooser) = chooser {
            vm.reset_fuel_window();
            let result=vm.invoke_instance(chooser,"chooseConfig","(Ljavax/microedition/khronos/egl/EGL10;Ljavax/microedition/khronos/egl/EGLDisplay;)Ljavax/microedition/khronos/egl/EGLConfig;",&[Value::Obj(egl),Value::Obj(display_object)])?;
            let object = obj(&result)?;
            let proxy = self
                .proxies
                .borrow()
                .get(&object)
                .cloned()
                .ok_or_else(|| invalid("EGL chooser returned a foreign config"))?;
            if proxy.class != EGL_CONFIG || proxy.display != display {
                return Err(invalid(
                    "EGL chooser returned a config from another display",
                ));
            }
            self.views
                .borrow_mut()
                .get_mut(&view)
                .unwrap()
                .chosen_config = Some(object);
            proxy.token
        } else {
            let backend = self.backend()?;
            let attributes = [
                0x3024, config[0], 0x3023, config[1], 0x3022, config[2], 0x3021, config[3], 0x3025,
                config[4], 0x3026, config[5], 0x3040, 4, NONE,
            ];
            let candidates = backend.choose_config(display, &attributes, 64)?;
            let mut chosen = None;
            for token in candidates.configs {
                let mut matches = true;
                for (i, attribute) in [0x3024, 0x3023, 0x3022, 0x3021, 0x3025, 0x3026]
                    .iter()
                    .enumerate()
                {
                    let actual = backend.get_config_attrib(display, token, *attribute)?;
                    matches &= if i < 4 {
                        actual == config[i]
                    } else {
                        actual >= config[i]
                    };
                }
                if matches {
                    chosen = Some(token);
                    break;
                }
            }
            let token = chosen.ok_or_else(|| {
                invalid("no EGL config satisfies the requested color/depth/stencil")
            })?;
            let object = self.proxy(vm, EGL_CONFIG, display, token)?;
            self.views
                .borrow_mut()
                .get_mut(&view)
                .unwrap()
                .chosen_config = Some(object);
            token
        };
        if token == 0 {
            return Err(invalid("EGL chooser returned null config"));
        }
        Ok(token)
    }
    pub(crate) fn callback(&self, vm: &mut Engine, view: ObjRef, event: GlEvent) -> Result<()> {
        let _scope = self.enter_callback();
        if matches!(event, GlEvent::DrainQueue) {
            for _ in 0..1024 {
                let event = self
                    .views
                    .borrow_mut()
                    .get_mut(&view)
                    .ok_or_else(|| illegal("unknown GL view"))?
                    .queue
                    .pop_front();
                let Some(runnable) = event else { return Ok(()) };
                vm.reset_fuel_window();
                vm.invoke_instance(runnable, "run", "()V", &[])?;
            }
            return if self.views.borrow()[&view].queue.is_empty() {
                Ok(())
            } else {
                Err(error(
                    "GL_EVENT_FUEL",
                    "more than 1024 queued render events",
                ))
            };
        }
        let renderer = self
            .views
            .borrow()
            .get(&view)
            .and_then(|s| s.renderer)
            .ok_or_else(|| illegal("renderer has not been set"))?;
        vm.reset_fuel_window();
        match event {
            GlEvent::SurfaceCreated {
                context_token,
                config_token,
            } => {
                if context_token == 0 || config_token == 0 {
                    return Err(invalid("actual EGL context/config tokens are required"));
                }
                let (display, config) = {
                    let s = self.views.borrow();
                    let s = s.get(&view).unwrap();
                    (s.display, s.chosen_config)
                };
                let display = display
                    .and_then(|r| self.proxies.borrow().get(&r).map(|p| p.token))
                    .ok_or_else(|| illegal("EGL config must be chosen before context creation"))?;
                let config = config.ok_or_else(|| illegal("EGL config is not selected"))?;
                if self.proxies.borrow().get(&config).map(|p| p.token) != Some(config_token) {
                    return Err(invalid(
                        "created context config differs from chooser result",
                    ));
                }
                let gl = self.proxy(vm, GL, display, context_token)?;
                {
                    let mut states = self.views.borrow_mut();
                    let s = states.get_mut(&view).unwrap();
                    s.gl = Some(gl);
                    s.created = true;
                    s.sized = false;
                }
                vm.invoke_instance(renderer,"onSurfaceCreated","(Ljavax/microedition/khronos/opengles/GL10;Ljavax/microedition/khronos/egl/EGLConfig;)V",&[Value::Obj(gl),Value::Obj(config)])?;
            }
            GlEvent::SurfaceChanged { width, height } => {
                if width <= 0 || height <= 0 {
                    return Err(invalid("surface dimensions must be positive"));
                }
                let gl = self
                    .views
                    .borrow()
                    .get(&view)
                    .filter(|s| s.created)
                    .and_then(|s| s.gl)
                    .ok_or_else(|| illegal("surface has not been created"))?;
                vm.invoke_instance(
                    renderer,
                    "onSurfaceChanged",
                    "(Ljavax/microedition/khronos/opengles/GL10;II)V",
                    &[Value::Obj(gl), Value::Int(width), Value::Int(height)],
                )?;
                let mut states = self.views.borrow_mut();
                let s = states.get_mut(&view).unwrap();
                s.sized = true;
                s.requested = s.requested.wrapping_add(1);
            }
            GlEvent::DrawFrame => {
                let (gl, serial) = {
                    let s = self.views.borrow();
                    let s = s.get(&view).unwrap();
                    if !s.created || !s.sized || s.paused {
                        return Err(illegal("GL surface is not ready to draw"));
                    }
                    (s.gl.unwrap(), s.requested)
                };
                vm.invoke_instance(
                    renderer,
                    "onDrawFrame",
                    "(Ljavax/microedition/khronos/opengles/GL10;)V",
                    &[Value::Obj(gl)],
                )?;
                self.views.borrow_mut().get_mut(&view).unwrap().drawn = serial;
            }
            GlEvent::SurfaceDestroyed => {
                let mut states = self.views.borrow_mut();
                let s = states.get_mut(&view).unwrap();
                s.created = false;
                s.sized = false;
                s.gl = None;
            }
            GlEvent::DrainQueue => unreachable!(),
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn call(
        &self,
        android: &Android,
        vm: &mut Engine,
        name: &str,
        sig: &str,
        r: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if self.views.borrow().contains_key(&r) {
            if let Some(value) = self.view_call(vm, name, sig, r, args)? {
                return Ok(Some(value));
            }
        }
        self.platform_call(android, vm, name, sig, r, args)
    }
    fn view_call(
        &self,
        vm: &mut Engine,
        name: &str,
        sig: &str,
        r: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        let setting = matches!(name, "setEGLContextClientVersion" | "setEGLConfigChooser");
        if setting && self.views.borrow()[&r].renderer.is_some() {
            return Err(illegal("EGL configuration must be set before setRenderer"));
        }
        let value = match (name, sig) {
            (
                "<init>",
                "(Landroid/content/Context;)V"
                | "(Landroid/content/Context;Landroid/util/AttributeSet;)V",
            ) => {
                obj(&args[0])?;
                Value::Null
            }
            ("setEGLContextClientVersion", "(I)V") => {
                if int(args, 0)? != 2 {
                    return Err(error(
                        "API_UNAVAILABLE",
                        "current backend exposes GLES 2 contexts only",
                    ));
                }
                self.views.borrow_mut().get_mut(&r).unwrap().client_version = 2;
                Value::Null
            }
            ("setEGLConfigChooser", "(IIIIII)V") => {
                let mut config = [0; 6];
                for (i, v) in config.iter_mut().enumerate() {
                    *v = int(args, i)?;
                    if !(0..=64).contains(v) {
                        return Err(invalid("invalid EGL channel/depth/stencil bit count"));
                    }
                }
                let mut s = self.views.borrow_mut();
                let s = s.get_mut(&r).unwrap();
                s.config = config;
                s.chooser = None;
                Value::Null
            }
            ("setEGLConfigChooser", "(Z)V") => {
                let mut s = self.views.borrow_mut();
                let s = s.get_mut(&r).unwrap();
                s.config = [8, 8, 8, 0, if int(args, 0)? != 0 { 16 } else { 0 }, 0];
                s.chooser = None;
                Value::Null
            }
            ("setEGLConfigChooser", "(Landroid/opengl/GLSurfaceView$EGLConfigChooser;)V") => {
                let chooser = obj(&args[0])?;
                self.views.borrow_mut().get_mut(&r).unwrap().chooser = Some(chooser);
                Value::Null
            }
            ("setRenderer", "(Landroid/opengl/GLSurfaceView$Renderer;)V") => {
                let renderer = obj(&args[0])?;
                if self.views.borrow()[&r].renderer.is_some() {
                    return Err(illegal("setRenderer may be called exactly once"));
                }
                if self.caps.borrow().is_none() || self.backend.borrow().is_none() {
                    return Err(error(
                        "GL_BACKEND_UNAVAILABLE",
                        "no display/EGL backend is bound",
                    ));
                }
                for (name,sig) in [("onSurfaceCreated","(Ljavax/microedition/khronos/opengles/GL10;Ljavax/microedition/khronos/egl/EGLConfig;)V"),("onSurfaceChanged","(Ljavax/microedition/khronos/opengles/GL10;II)V"),("onDrawFrame","(Ljavax/microedition/khronos/opengles/GL10;)V")] {
                    if vm.cp.resolve_method(vm.heap.class_of(renderer)?,name,sig).is_none(){return Err(invalid("renderer does not implement its DEX callback contract"));}
                }
                self.views.borrow_mut().get_mut(&r).unwrap().renderer = Some(renderer);
                Value::Null
            }
            ("setRenderMode", "(I)V") => {
                let mode = int(args, 0)?;
                if mode != 0 && mode != 1 {
                    return Err(invalid("render mode must be WHEN_DIRTY or CONTINUOUSLY"));
                }
                let mut s = self.views.borrow_mut();
                let s = s.get_mut(&r).unwrap();
                if s.renderer.is_none() {
                    return Err(illegal("renderer is not set"));
                }
                s.render_mode = mode;
                Value::Null
            }
            ("getRenderMode", "()I") => {
                let s = self.views.borrow();
                let s = &s[&r];
                if s.renderer.is_none() {
                    return Err(illegal("renderer is not set"));
                }
                Value::Int(s.render_mode)
            }
            ("requestRender", "()V") => {
                let mut s = self.views.borrow_mut();
                let s = s.get_mut(&r).unwrap();
                if s.renderer.is_none() {
                    return Err(illegal("renderer is not set"));
                }
                s.requested = s.requested.wrapping_add(1);
                Value::Null
            }
            ("onPause" | "onResume", "()V") => {
                let mut s = self.views.borrow_mut();
                let s = s.get_mut(&r).unwrap();
                if s.renderer.is_none() {
                    return Err(illegal("renderer is not set"));
                }
                s.paused = name == "onPause";
                if !s.paused {
                    s.requested = s.requested.wrapping_add(1);
                }
                Value::Null
            }
            ("setPreserveEGLContextOnPause", "(Z)V") => {
                self.views.borrow_mut().get_mut(&r).unwrap().preserve = int(args, 0)? != 0;
                Value::Null
            }
            ("getPreserveEGLContextOnPause", "()Z") => {
                Value::Int(i32::from(self.views.borrow()[&r].preserve))
            }
            ("queueEvent", "(Ljava/lang/Runnable;)V") => {
                let event = obj(&args[0])?;
                let mut s = self.views.borrow_mut();
                let s = s.get_mut(&r).unwrap();
                if s.renderer.is_none() {
                    return Err(illegal("renderer is not set"));
                }
                if s.queue.len() >= 1024 {
                    return Err(error("GL_EVENT_LIMIT", "1024 queued events"));
                }
                s.queue.push_back(event);
                Value::Null
            }
            ("setDebugFlags", "(I)V") if int(args, 0)? == 0 => Value::Null,
            ("getDebugFlags", "()I") => Value::Int(0),
            ("setDebugFlags", _)
            | ("setGLWrapper", _)
            | ("setEGLContextFactory", _)
            | ("setEGLWindowSurfaceFactory", _) => {
                return Err(error(
                    "API_UNAVAILABLE",
                    format!("GLSurfaceView.{name}{sig}"),
                ))
            }
            _ => return Ok(None),
        };
        Ok(Some(value))
    }
    fn integer_array(vm: &Engine, value: &Value) -> Result<Vec<i32>> {
        let r = obj(value)?;
        let values = vm
            .heap
            .as_array_elems(r)
            .map_err(|e| error("INVALID_ARRAY", e))?
            .0;
        if values.len() > 128 {
            return Err(error("EGL_ATTRIBUTE_LIMIT", "128 EGL attribute integers"));
        }
        values
            .iter()
            .map(|v| v.as_int().map_err(Into::into))
            .collect()
    }
    fn array_int(vm: &mut Engine, array: &Value, index: usize, value: i32) -> Result<()> {
        let r = obj(array)?;
        let values = vm
            .heap
            .array_elems_mut(r)
            .map_err(|e| error("INVALID_ARRAY", e))?;
        let slot = values
            .get_mut(index)
            .ok_or_else(|| invalid("EGL output array is too small"))?;
        *slot = Value::Int(value);
        Ok(())
    }
    fn egl_call(
        &self,
        vm: &mut Engine,
        name: &str,
        sig: &str,
        r: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        let proxy = self.proxies.borrow().get(&r).cloned();
        let Some(proxy) = proxy else { return Ok(None) };
        if matches!(
            (name, sig),
            ("getClass", "()Ljava/lang/Class;")
                | ("toString", "()Ljava/lang/String;")
                | ("hashCode", "()I")
                | ("equals", "(Ljava/lang/Object;)Z")
        ) {
            return Ok(None);
        }
        if proxy.class == GL {
            return Err(error(
                "API_UNAVAILABLE",
                format!("Java GL10.{name}{sig}; native GLES requires the active platform context"),
            ));
        }
        if proxy.class != EGL_API {
            return Ok(None);
        }
        if !matches!(
            (name, sig),
            (
                "eglChooseConfig",
                "(Ljavax/microedition/khronos/egl/EGLDisplay;[I[Ljavax/microedition/khronos/egl/EGLConfig;I[I)Z"
            ) | (
                "eglGetConfigAttrib",
                "(Ljavax/microedition/khronos/egl/EGLDisplay;Ljavax/microedition/khronos/egl/EGLConfig;I[I)Z"
            )
        ) {
            return Err(error("API_UNAVAILABLE", format!("EGL10.{name}{sig}")));
        }
        let display = obj(&args[0])?;
        let display = self
            .proxies
            .borrow()
            .get(&display)
            .cloned()
            .ok_or_else(|| invalid("foreign EGL display"))?;
        if display.class != EGL_DISPLAY || display.token != proxy.display {
            return Err(invalid("EGL display belongs to another context"));
        }
        match (name,sig) {
            ("eglChooseConfig","(Ljavax/microedition/khronos/egl/EGLDisplay;[I[Ljavax/microedition/khronos/egl/EGLConfig;I[I)Z")=>{
                let attributes=if matches!(args[1],Value::Null){vec![NONE]}else{Self::integer_array(vm,&args[1])?};
                if attributes.last()!=Some(&NONE) || attributes.len()%2!=1 {return Err(invalid("EGL attributes require pairs followed by EGL_NONE"));}
                let capacity=int(args,3)?;
                if !(0..=64).contains(&capacity){return Err(error("EGL_CONFIG_LIMIT","64 EGL configs per query"));}
                let capacity=if matches!(args[2],Value::Null){0}else{capacity as usize};
                if let Value::Obj(out)=args[2]{if vm.heap.as_array_elems(out).map_err(|e|error("INVALID_ARRAY",e))?.0.len()<capacity{return Err(invalid("config output array is too small"));}}
                let result=self.backend()?.choose_config(display.token,&attributes,capacity)?;
                if result.configs.len()>capacity || result.count>i32::MAX as usize{return Err(error("INVALID_EGL_RESULT","backend returned invalid config count"));}
                Self::array_int(vm,&args[4],0,result.count as i32)?;
                if let Value::Obj(out)=args[2] {
                    for (i,token) in result.configs.into_iter().enumerate(){let config=self.proxy(vm,EGL_CONFIG,display.token,token)?;vm.heap.array_elems_mut(out).map_err(|e|error("INVALID_ARRAY",e))?[i]=Value::Obj(config);}
                }
                Ok(Some(Value::Int(1)))
            }
            ("eglGetConfigAttrib","(Ljavax/microedition/khronos/egl/EGLDisplay;Ljavax/microedition/khronos/egl/EGLConfig;I[I)Z")=>{
                let object=obj(&args[1])?;let config=self.proxies.borrow().get(&object).cloned().ok_or_else(||invalid("foreign EGL config"))?;
                if config.class!=EGL_CONFIG || config.display!=display.token{return Err(invalid("EGL config belongs to another display"));}
                let value=self.backend()?.get_config_attrib(display.token,config.token,int(args,2)?)?;
                Self::array_int(vm,&args[3],0,value)?;Ok(Some(Value::Int(1)))
            }
            _=>Err(error("API_UNAVAILABLE",format!("EGL10.{name}{sig}"))),
        }
    }
    fn fill_metrics(&self, android: &Android, vm: &mut Engine, metrics: ObjRef) -> Result<()> {
        let caps = self.metrics(android);
        for (name, value) in [
            ("widthPixels", caps.width),
            ("heightPixels", caps.height),
            ("densityDpi", caps.density_dpi),
        ] {
            write(vm, metrics, name, Value::Int(value))?;
        }
        for name in ["density", "scaledDensity"] {
            write(vm, metrics, name, Value::Float(caps.density))?;
        }
        // Physical xdpi/ydpi are not guessed from logical density.
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn platform_call(
        &self,
        android: &Android,
        vm: &mut Engine,
        name: &str,
        sig: &str,
        r: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if self.proxies.borrow().contains_key(&r) {
            return self.egl_call(vm, name, sig, r, args);
        }
        let class = vm.heap.class_of(r)?.to_string();
        let context = vm.cp.is_subtype(&class, "Landroid/content/Context;");
        let activity = vm.cp.is_subtype(&class, "Landroid/app/Activity;");
        let view = vm.cp.is_subtype(&class, "Landroid/view/View;");
        let value = match (name, sig) {
            ("<init>", "()V")
                if matches!(
                    class.as_str(),
                    "Landroid/util/DisplayMetrics;"
                        | "Landroid/view/Window;"
                        | "Landroid/view/WindowManager;"
                        | "Landroid/view/Display;"
                        | "Landroid/app/ActivityManager;"
                        | "Landroid/content/pm/ConfigurationInfo;"
                        | "Landroid/content/res/Configuration;"
                        | "Landroid/media/AudioManager;"
                ) =>
            {
                Value::Null
            }
            ("getSystemService", "(Ljava/lang/String;)Ljava/lang/Object;") if context => {
                match text(vm, &args[0])?.as_str() {
                    "window" => Value::Obj(self.singleton(vm, "Landroid/view/WindowManager;")?),
                    "activity" => Value::Obj(self.singleton(vm, "Landroid/app/ActivityManager;")?),
                    "audio" if android.audio.borrow().is_some() => {
                        Value::Obj(self.singleton(vm, "Landroid/media/AudioManager;")?)
                    }
                    _ => Value::Null,
                }
            }
            ("getWindowManager", "()Landroid/view/WindowManager;") if activity => {
                Value::Obj(self.singleton(vm, "Landroid/view/WindowManager;")?)
            }
            ("getWindow", "()Landroid/view/Window;") if activity => {
                Value::Obj(self.singleton(vm, "Landroid/view/Window;")?)
            }
            ("getDefaultDisplay", "()Landroid/view/Display;")
                if class == "Landroid/view/WindowManager;" =>
            {
                Value::Obj(self.singleton(vm, "Landroid/view/Display;")?)
            }
            ("getMetrics" | "getRealMetrics", "(Landroid/util/DisplayMetrics;)V")
                if class == "Landroid/view/Display;" =>
            {
                self.fill_metrics(android, vm, obj(&args[0])?)?;
                Value::Null
            }
            ("getWidth", "()I") if class == "Landroid/view/Display;" => {
                Value::Int(self.metrics(android).width)
            }
            ("getHeight", "()I") if class == "Landroid/view/Display;" => {
                Value::Int(self.metrics(android).height)
            }
            ("getRotation", "()I") if class == "Landroid/view/Display;" => {
                Value::Int(self.metrics(android).rotation)
            }
            ("getDeviceConfigurationInfo", "()Landroid/content/pm/ConfigurationInfo;")
                if class == "Landroid/app/ActivityManager;" =>
            {
                let info = shell(vm, "Landroid/content/pm/ConfigurationInfo;")?;
                write(
                    vm,
                    info,
                    "reqGlEsVersion",
                    Value::Int(self.metrics(android).gles_version as i32),
                )?;
                Value::Obj(info)
            }
            ("getDisplayMetrics", "()Landroid/util/DisplayMetrics;")
                if class == "Landroid/content/res/Resources;" =>
            {
                let metrics = shell(vm, "Landroid/util/DisplayMetrics;")?;
                self.fill_metrics(android, vm, metrics)?;
                Value::Obj(metrics)
            }
            ("getConfiguration", "()Landroid/content/res/Configuration;")
                if class == "Landroid/content/res/Resources;" =>
            {
                let caps = self.metrics(android);
                let configuration = shell(vm, "Landroid/content/res/Configuration;")?;
                let width = libm::roundf(caps.width as f32 / caps.density) as i32;
                let height = libm::roundf(caps.height as f32 / caps.density) as i32;
                for (field, value) in [
                    (
                        "orientation",
                        if caps.width > caps.height {
                            2
                        } else if caps.height > caps.width {
                            1
                        } else {
                            0
                        },
                    ),
                    ("screenWidthDp", width),
                    ("screenHeightDp", height),
                    ("smallestScreenWidthDp", width.min(height)),
                    ("densityDpi", caps.density_dpi),
                ] {
                    write(vm, configuration, field, Value::Int(value))?;
                }
                write(vm, configuration, "fontScale", Value::Float(1.))?;
                Value::Obj(configuration)
            }
            ("getProperty", "(Ljava/lang/String;)Ljava/lang/String;")
                if class == "Landroid/media/AudioManager;" =>
            {
                text(vm, &args[0])?;
                Value::Null
            }
            ("setVolumeControlStream", "(I)V") if activity => {
                if int(args, 0)? != 3 {
                    return Err(error(
                        "API_UNAVAILABLE",
                        "volume keys currently control STREAM_MUSIC",
                    ));
                }
                write(vm, r, "volumeControlStream", Value::Int(3))?;
                Value::Null
            }
            ("getVolumeControlStream", "()I") if activity => vm
                .heap
                .get_field(r, "volumeControlStream")
                .unwrap_or(Value::Int(-1)),
            ("setRequestedOrientation", "(I)V") if activity => {
                self.request_orientation(android, r, int(args, 0)?)?;
                Value::Null
            }
            ("getRequestedOrientation", "()I") if activity => {
                Value::Int(*self.orientations.borrow().get(&r).unwrap_or(&-1))
            }
            ("getDecorView", "()Landroid/view/View;") if class == "Landroid/view/Window;" => {
                Value::Obj(self.singleton(vm, "Landroid/widget/FrameLayout;")?)
            }
            ("setSystemUiVisibility", "(I)V") if view => {
                let flags = int(args, 0)?;
                if flags & !0x3fff != 0 {
                    return Err(error("API_UNAVAILABLE", "system UI flag"));
                }
                self.system_ui.borrow_mut().insert(r, flags);
                Value::Null
            }
            ("getSystemUiVisibility", "()I") if view => {
                Value::Int(*self.system_ui.borrow().get(&r).unwrap_or(&0))
            }
            ("setKeepScreenOn", "(Z)V") if view => {
                self.backend()?.set_keep_screen_on(int(args, 0)? != 0)?;
                write(vm, r, "keepScreenOn", Value::Int(int(args, 0)?))?;
                Value::Null
            }
            ("getKeepScreenOn", "()Z") if view => vm
                .heap
                .get_field(r, "keepScreenOn")
                .unwrap_or(Value::Int(0)),
            ("addFlags" | "clearFlags", "(I)V") if class == "Landroid/view/Window;" => {
                let mask = int(args, 0)?;
                self.flags(r, if name == "addFlags" { mask } else { 0 }, mask)?;
                Value::Null
            }
            ("setFlags", "(II)V") if class == "Landroid/view/Window;" => {
                self.flags(r, int(args, 0)?, int(args, 1)?)?;
                Value::Null
            }
            ("requestWindowFeature" | "requestFeature", "(I)Z")
                if activity || class == "Landroid/view/Window;" =>
            {
                Value::Int(i32::from(int(args, 0)? == 1))
            }
            _ => return Ok(None),
        };
        Ok(Some(value))
    }
    fn flags(&self, window: ObjRef, flags: i32, mask: i32) -> Result<()> {
        if mask & !0x480 != 0 || (mask & 0x400 != 0 && flags & 0x400 == 0) {
            return Err(error(
                "API_UNAVAILABLE",
                "only fullscreen/keep-screen-on window policy is supported",
            ));
        }
        let old = *self.window_flags.borrow().get(&window).unwrap_or(&0x400);
        let new = (old & !mask) | (flags & mask);
        if (old ^ new) & 0x80 != 0 {
            self.backend()?.set_keep_screen_on(new & 0x80 != 0)?;
        }
        self.window_flags.borrow_mut().insert(window, new);
        Ok(())
    }
    pub(crate) fn collect(&self, marked: &[bool]) {
        let live = |object: &ObjRef| marked.get(*object as usize).copied().unwrap_or(false);
        self.proxies.borrow_mut().retain(|r, _| live(r));
        self.system_ui.borrow_mut().retain(|r, _| live(r));
        self.window_flags.borrow_mut().retain(|r, _| live(r));
        self.orientations.borrow_mut().retain(|r, _| live(r));
    }
    pub(crate) fn roots(&self) -> Vec<ObjRef> {
        let mut roots = self
            .singletons
            .borrow()
            .values()
            .copied()
            .collect::<Vec<_>>();
        for (&view, s) in self.views.borrow().iter() {
            roots.push(view);
            roots.extend(s.renderer);
            roots.extend(s.chooser);
            roots.extend(s.queue.iter().copied());
            roots.extend(s.gl);
            roots.extend(s.egl);
            roots.extend(s.display);
            roots.extend(s.chosen_config);
        }
        roots
    }
}
