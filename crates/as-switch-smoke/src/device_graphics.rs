//! Horizon's real EGL worker executes the framework's DEX Renderer callbacks.
//! C retains only stable Box/Rc addresses. Application teardown joins the worker
//! before releasing the Engine, Android, session, or any JNI/GC roots.
use crate::device_jni::{self, Session};
use alloc::{rc::Rc, string::String, vec};
use as_framework::{Android, App, EglConfigBackend, EglConfigs, GlEvent, GraphicsCaps};
use core::ffi::c_void;
use rd_vm::{Engine, ObjRef, VmExit};

#[repr(C)]
struct Request {
    vm: u64,
    android: u64,
    session: u64,
    view: u32,
    client_version: i32,
}
#[allow(unsafe_code)]
extern "C" {
    fn as_glsurface_start(request: *const Request) -> i32;
    fn as_media_egl_choose(
        display: u64,
        attributes: *const i32,
        length: usize,
        configs: *mut u64,
        capacity: usize,
        count: *mut i32,
    ) -> i32;
    fn as_media_egl_attrib(display: u64, config: u64, attribute: i32, value: *mut i32) -> i32;
    fn as_media_keep_screen_on(enabled: i32) -> i32;
    fn as_native_session() -> *const c_void;
    fn as_display_set_rotation(rotation: i32) -> i32;
    fn as_display_rotation() -> i32;
    fn as_display_width() -> i32;
    fn as_display_height() -> i32;
}
fn error(operation: &str, code: i32) -> VmExit {
    rd_vm::err::vm_error("HORIZON_EGL_ERROR", alloc::format!("{operation}: {code}")).into()
}
struct HorizonEgl;
impl EglConfigBackend for HorizonEgl {
    fn set_display_rotation(&self, rotation: i32) -> Result<GraphicsCaps, VmExit> {
        // SAFETY: scalar-only platform negotiation rejects changing live
        // presentation before mutating window geometry or committed metadata.
        #[allow(unsafe_code)]
        let rc = unsafe { as_display_set_rotation(rotation) };
        if rc != 0 {
            return Err(error("set_display_rotation", rc));
        }
        Ok(graphics_caps())
    }
    fn choose_config(
        &self,
        display: u64,
        attributes: &[i32],
        capacity: usize,
    ) -> Result<EglConfigs, VmExit> {
        if capacity > 64 || attributes.len() > 129 {
            return Err(error("choose_config argument budget", -22));
        }
        let mut configs = vec![0; capacity];
        let mut count = 0;
        // SAFETY: C consumes bounded input and writes only into the supplied
        // capacity. No pointers are retained after this synchronous call.
        #[allow(unsafe_code)]
        let rc = unsafe {
            as_media_egl_choose(
                display,
                attributes.as_ptr(),
                attributes.len(),
                configs.as_mut_ptr(),
                capacity,
                &mut count,
            )
        };
        if rc != 0 || count < 0 || (capacity > 0 && count as usize > capacity) {
            return Err(error("eglChooseConfig", if rc == 0 { -5 } else { rc }));
        }
        configs.truncate(count as usize);
        Ok(EglConfigs {
            configs,
            count: count as usize,
        })
    }
    fn get_config_attrib(&self, display: u64, config: u64, attribute: i32) -> Result<i32, VmExit> {
        let mut value = 0;
        // SAFETY: EGL handles are validated by the platform; output is live.
        #[allow(unsafe_code)]
        let rc = unsafe { as_media_egl_attrib(display, config, attribute, &mut value) };
        if rc == 0 {
            Ok(value)
        } else {
            Err(error("eglGetConfigAttrib", rc))
        }
    }
    fn set_keep_screen_on(&self, enabled: bool) -> Result<(), VmExit> {
        // SAFETY: scalar power policy forwarded to libnx.
        #[allow(unsafe_code)]
        let rc = unsafe { as_media_keep_screen_on(i32::from(enabled)) };
        if rc == 0 {
            Ok(())
        } else {
            Err(error("keep_screen_on", rc))
        }
    }
}
fn graphics_caps() -> GraphicsCaps {
    // SAFETY: getters read the platform's committed scalar display metadata.
    #[allow(unsafe_code)]
    unsafe {
        GraphicsCaps {
            width: as_display_width(),
            height: as_display_height(),
            rotation: as_display_rotation(),
            gles_version: 0x20000,
            density: 2.,
            density_dpi: 320,
        }
    }
}
pub fn bind(android: &Android) -> Result<(), VmExit> {
    android.bind_graphics_caps(graphics_caps())?;
    android.bind_egl_backend(Rc::new(HorizonEgl));
    Ok(())
}
/// Start after onCreate while holding the VM lease. C starts asynchronously;
/// its worker can enter DEX after the owning caller releases that lease.
pub fn start(app: &mut App, session: &Rc<Session>) -> Result<bool, String> {
    let requests = app
        .android
        .gl_requests()
        .map_err(|e| alloc::format!("{e}"))?;
    if requests.is_empty() {
        return Ok(false);
    }
    if requests.len() != 1 {
        return Err("GL_COMPOSITION_UNAVAILABLE: one full-screen surface required".into());
    }
    let r = &requests[0];
    let request = Request {
        vm: app.vm.as_mut() as *mut Engine as u64,
        android: Rc::as_ptr(&app.android) as u64,
        session: Rc::as_ptr(session) as u64,
        view: r.view,
        client_version: r.client_version,
    };
    // SAFETY: C copies this request. The process owner retains all three stable
    // allocations until as_media_quiesce has joined its worker.
    #[allow(unsafe_code)]
    let rc = unsafe { as_glsurface_start(&request) };
    if rc != 0 {
        Err(alloc::format!("HORIZON_GL_THREAD_ERROR: {rc}"))
    } else {
        Ok(true)
    }
}

/// # Safety
/// C's registered renderer owns these stable allocations; its caller holds the
/// VM lease. Session is the actual native TLS owner, never an invented handle.
#[allow(unsafe_code)]
unsafe fn invoke<T>(
    vm: u64,
    android: u64,
    f: impl FnOnce(&Session, &mut Engine, &Android) -> Result<T, VmExit>,
) -> Option<T> {
    let session = unsafe { as_native_session() }.cast::<Session>();
    if vm == 0 || android == 0 || session.is_null() {
        return None;
    }
    let session = unsafe { &*session };
    match f(session, unsafe { &mut *(vm as *mut Engine) }, unsafe {
        &*(android as *const Android)
    }) {
        Ok(value) => Some(value),
        Err(error) => {
            session.record_runtime_error(error);
            None
        }
    }
}
/// # Safety
/// Only the owned C render thread calls this with registered stable pointers.
#[allow(unsafe_code)]
#[allow(clippy::too_many_arguments)] // Stable scalar-only render-thread C ABI.
#[no_mangle]
pub unsafe extern "C" fn as_application_gl_call(
    vm: u64,
    android: u64,
    view: ObjRef,
    event: i32,
    width: i32,
    height: i32,
    context: u64,
    config: u64,
) -> i32 {
    device_jni::lock();
    let result = unsafe {
        invoke(vm, android, |session, vm, android| {
            let event = match event {
                0 => {
                    session.attach_runtime_thread();
                    return Ok(());
                }
                5 => {
                    session.detach_runtime_thread();
                    return Ok(());
                }
                1 => GlEvent::SurfaceCreated {
                    context_token: context,
                    config_token: config,
                },
                2 => GlEvent::SurfaceChanged { width, height },
                3 => GlEvent::DrawFrame,
                4 => GlEvent::DrainQueue,
                6 => GlEvent::SurfaceDestroyed,
                _ => return Err(error("unknown render event", event)),
            };
            android.gl_callback(vm, view, event)
        })
    };
    device_jni::unlock();
    if result.is_some() {
        0
    } else {
        -2300
    }
}
/// # Safety
/// Called under the same stable owner contract as as_application_gl_call.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_application_gl_choose(
    vm: u64,
    android: u64,
    view: ObjRef,
    display: u64,
) -> u64 {
    device_jni::lock();
    let result = unsafe {
        invoke(vm, android, |_, vm, android| {
            android.gl_choose_config(vm, view, display)
        })
    };
    device_jni::unlock();
    result.unwrap_or(0)
}
/// # Safety
/// Called under the same stable owner contract as as_application_gl_call.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_application_gl_poll(vm: u64, android: u64, view: ObjRef) -> i32 {
    device_jni::lock();
    let result = unsafe {
        invoke(vm, android, |session, _, android| {
            if session.runtime_error().is_some() {
                return Ok(-2300);
            }
            let requests = android.gl_requests()?;
            let Some(r) = requests.iter().find(|r| r.view == view) else {
                return Ok(0);
            };
            Ok(1 | if r.paused { 2 } else { 0 }
                | if r.render_mode == 1 || r.dirty { 4 } else { 0 }
                | if r.queued_count != 0 { 8 } else { 0 }
                | if r.preserve_context_on_pause { 16 } else { 0 })
        })
    };
    device_jni::unlock();
    result.unwrap_or(-2300)
}
