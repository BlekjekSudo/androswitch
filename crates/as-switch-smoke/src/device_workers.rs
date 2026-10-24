//! Horizon-owned serial VM workers, independent of the UI Looper/render loop.
//! Stable owner addresses stay live until application teardown proves joins.
use crate::device_jni::{self, Session};
use alloc::rc::Rc;
use as_framework::{Android, App};
use as_platform::{BackgroundWorkers, PlatformError};
use core::ffi::c_void;
use rd_vm::{Engine, VmExit};

#[repr(C)]
struct Request {
    vm: u64,
    android: u64,
    session: u64,
    token: u64,
}
#[allow(unsafe_code)]
extern "C" {
    fn as_workers_start(request: *const Request) -> i32;
    fn as_workers_wake(token: u64) -> i32;
    fn as_workers_cancel(token: u64) -> i32;
    fn as_workers_quiesce(timeout_ns: u64) -> i32;
    fn as_native_session() -> *const c_void;
}
struct HorizonWorkers {
    vm: u64,
    android: u64,
    session: u64,
}
fn platform_error(operation: &'static str, token: u64, result: i32) -> PlatformError {
    PlatformError {
        code: "HORIZON_WORKER_ERROR",
        operation,
        cause: alloc::format!("worker {token}: platform result {result}"),
    }
}
impl BackgroundWorkers for HorizonWorkers {
    fn start(&self, token: u64) -> Result<(), PlatformError> {
        let request = Request {
            vm: self.vm,
            android: self.android,
            session: self.session,
            token,
        };
        // SAFETY: C copies the scalar request and starts a real OS worker.
        // Running retains these stable owners through successful quiescence.
        #[allow(unsafe_code)]
        let result = unsafe { as_workers_start(&request) };
        if result == 0 {
            Ok(())
        } else {
            Err(platform_error("start", token, result))
        }
    }
    fn wake(&self, token: u64) -> Result<(), PlatformError> {
        // SAFETY: scalar token lookup; no VM callbacks run synchronously.
        #[allow(unsafe_code)]
        let result = unsafe { as_workers_wake(token) };
        if result == 0 {
            Ok(())
        } else {
            Err(platform_error("wake", token, result))
        }
    }
    fn cancel(&self, token: u64) -> Result<(), PlatformError> {
        // SAFETY: signals the owned worker, never joins the invoking thread.
        #[allow(unsafe_code)]
        let result = unsafe { as_workers_cancel(token) };
        if result == 0 {
            Ok(())
        } else {
            Err(platform_error("cancel", token, result))
        }
    }
}
pub fn bind(app: &mut App, session: &Rc<Session>) -> Result<(), VmExit> {
    app.android.bind_background_workers(Rc::new(HorizonWorkers {
        vm: app.vm.as_mut() as *mut Engine as u64,
        android: Rc::as_ptr(&app.android) as u64,
        session: Rc::as_ptr(session) as u64,
    }));
    Ok(())
}
pub fn quiesce(timeout_ns: u64) -> i32 {
    // SAFETY: caller released the VM lease and retains all callback owners.
    #[allow(unsafe_code)]
    unsafe {
        as_workers_quiesce(timeout_ns)
    }
}
fn copy_error(error: &VmExit) -> VmExit {
    match error {
        VmExit::Exception(value) => VmExit::Exception(value.clone()),
        VmExit::Error(value) => VmExit::Error(value.clone()),
    }
}
/// # Safety
/// Only registered C workers may call these stable VM/Android pointers; the
/// caller's actual native TLS Session stays alive until this worker is joined.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_application_worker_call(
    vm: u64,
    android: u64,
    token: u64,
    event: i32,
    detail: i64,
) -> i64 {
    device_jni::lock();
    let session = unsafe { as_native_session() }.cast::<Session>();
    let mut result = -2300;
    if vm != 0 && android != 0 && token != 0 && !session.is_null() {
        // SAFETY: stable owner contract above, protected by the VM lease.
        let (session, vm, android) = unsafe {
            (
                &*session,
                &mut *(vm as *mut Engine),
                &*(android as *const Android),
            )
        };
        let called = match event {
            0 => {
                session.attach_runtime_thread();
                Ok(0)
            }
            1 => android.background_worker_step(vm, token).map(|wait| {
                wait.map(|milliseconds| milliseconds.min(i64::MAX as u64) as i64)
                    .unwrap_or(-1)
            }),
            2 => {
                android.background_worker_stopped(token);
                session.detach_runtime_thread();
                Ok(0)
            }
            _ => Err(rd_vm::err::vm_error(
                "HORIZON_WORKER_ERROR",
                alloc::format!("worker {token}: transport event {event}, result {detail}"),
            )
            .into()),
        };
        match called {
            Ok(value) => result = value,
            Err(error) => {
                android.background_worker_error(token, copy_error(&error));
                session.record_runtime_error(error);
            }
        }
    }
    device_jni::unlock();
    result
}
