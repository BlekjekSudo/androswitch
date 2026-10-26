//! A recording backend checks API scheduling; it does not claim GPU execution.
use as_framework::{App, AppOptions, EglConfigBackend, EglConfigs, GlEvent, GraphicsCaps};
use as_platform::{host::HostPlatform, host_fs::HostFileSystem};
use rd_vm::{native_host::NativeHost, Engine, ObjRef, Value, VmExit};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};
const ACTIVITY: &str = "Lorg/androswitch/graphics/SurfaceActivity;";
static NEXT: AtomicU64 = AtomicU64::new(0);
#[derive(Default)]
struct RecordingEgl {
    calls: RefCell<Vec<(u64, Vec<i32>, usize)>>,
    rotations: RefCell<Vec<i32>>,
    reject_rotation: Cell<bool>,
}
impl EglConfigBackend for RecordingEgl {
    fn set_display_rotation(&self, rotation: i32) -> Result<GraphicsCaps, VmExit> {
        self.rotations.borrow_mut().push(rotation);
        if self.reject_rotation.get() {
            return Err(rd_vm::err::vm_error(
                "DISPLAY_TEST_REJECTED",
                "provider kept display unchanged",
            )
            .into());
        }
        Ok(GraphicsCaps {
            width: if rotation % 2 == 0 { 1280 } else { 720 },
            height: if rotation % 2 == 0 { 720 } else { 1280 },
            gles_version: 0x20000,
            density: 2.,
            density_dpi: 320,
            rotation,
        })
    }
    fn choose_config(
        &self,
        display: u64,
        attributes: &[i32],
        capacity: usize,
    ) -> Result<EglConfigs, VmExit> {
        assert_eq!(display, 0x100);
        assert_eq!(attributes.last(), Some(&0x3038));
        self.calls
            .borrow_mut()
            .push((display, attributes.to_vec(), capacity));
        Ok(EglConfigs {
            configs: if capacity == 0 { vec![] } else { vec![0x11] },
            count: 1,
        })
    }
    fn get_config_attrib(&self, display: u64, config: u64, attribute: i32) -> Result<i32, VmExit> {
        assert_eq!((display, config), (0x100, 0x11));
        Ok(match attribute {
            0x3022..=0x3024 => 8,
            0x3021 | 0x3026 => 0,
            0x3025 => 16,
            _ => panic!("unexpected attribute {attribute:x}"),
        })
    }
}
fn make(fixture: &str, bind: bool) -> (App, Rc<RecordingEgl>) {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = repo.join(format!(
        "target/gl-tests/{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let fs = Rc::new(HostFileSystem::new(&root).unwrap());
    let pm = as_package::PackageManager::new(fs.clone());
    let record = pm
        .install(
            std::fs::read(repo.join(format!(
                "local-artifacts/m9/glsurface-fixture/{fixture}.apk"
            )))
            .expect("run scripts/build-glsurface-fixture.ps1"),
            true,
            None,
        )
        .unwrap();
    let app = App::new_with_options(
        pm.load_apk(&record.package, None).unwrap().1,
        fs,
        Rc::new(HostPlatform::default()),
        640,
        360,
        None,
        AppOptions {
            source_apk: Some(format!(
                "installed/{}/{:020}/package.apk",
                record.package, record.revision
            )),
            ..Default::default()
        },
    )
    .unwrap();
    let backend = Rc::new(RecordingEgl::default());
    if bind {
        app.android
            .bind_graphics_caps(GraphicsCaps {
                width: 1280,
                height: 720,
                gles_version: 0x20000,
                density: 2.,
                density_dpi: 320,
                rotation: 0,
            })
            .unwrap();
        app.android.bind_egl_backend(backend.clone());
    }
    (app, backend)
}
fn status(app: &mut App) -> String {
    let value = app
        .vm
        .invoke_static(ACTIVITY, "status", "()Ljava/lang/String;", &[])
        .unwrap();
    app.vm
        .heap
        .as_str(value.as_ref().unwrap().unwrap())
        .unwrap()
        .to_string()
}
fn initialize_surface(app: &mut App) -> ObjRef {
    app.launch().unwrap();
    let view = app.android.gl_requests().unwrap()[0].view;
    app.vm.gc();
    assert_eq!(
        app.android
            .gl_choose_config(&mut app.vm, view, 0x100)
            .unwrap(),
        0x11
    );
    app.android
        .gl_callback(
            &mut app.vm,
            view,
            GlEvent::SurfaceCreated {
                context_token: 0x200,
                config_token: 0x11,
            },
        )
        .unwrap();
    app.android
        .gl_callback(
            &mut app.vm,
            view,
            GlEvent::SurfaceChanged {
                width: 1280,
                height: 720,
            },
        )
        .unwrap();
    view
}
fn request_orientation(app: &mut App, requested: i32) -> Result<Value, VmExit> {
    app.vm.invoke_instance(
        app.android.activity_object().unwrap(),
        "setRequestedOrientation",
        "(I)V",
        &[Value::Int(requested)],
    )
}
fn requested_orientation(app: &mut App) -> i32 {
    app.vm
        .invoke_instance(
            app.android.activity_object().unwrap(),
            "getRequestedOrientation",
            "()I",
            &[],
        )
        .unwrap()
        .as_int()
        .unwrap()
}
fn display(app: &mut App) -> ObjRef {
    let wm = app
        .vm
        .invoke_instance(
            app.android.activity_object().unwrap(),
            "getWindowManager",
            "()Landroid/view/WindowManager;",
            &[],
        )
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    app.vm
        .invoke_instance(wm, "getDefaultDisplay", "()Landroid/view/Display;", &[])
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap()
}
#[test]
#[ignore = "requires scripts/build-glsurface-fixture.ps1"]
fn negotiated_portrait_updates_real_framework_state_and_active_rotation_fails_closed() {
    for fixture in ["surface", "renamed"] {
        let (mut app, backend) = make(fixture, true);
        app.vm
            .invoke_static(
                ACTIVITY,
                "configureInitialOrientation",
                "(IZ)V",
                &[Value::Int(1), Value::Int(0)],
            )
            .unwrap();
        app.launch().unwrap();
        assert_eq!((app.frame.width, app.frame.height), (360, 640));
        assert_eq!(app.android.views()[0].bounds, [0, 0, 720, 1280]);
        assert!(status(&mut app).starts_with("M720:1280:2"));
        for requested in [-1, 2, 4, 5, 10, 13, 14] {
            request_orientation(&mut app, requested).unwrap();
            assert_eq!(requested_orientation(&mut app), requested);
            assert_eq!(app.android.graphics_caps().unwrap().rotation, 1);
        }
        for (requested, rotation) in [
            (1, 1),
            (7, 1),
            (12, 1),
            (0, 0),
            (6, 0),
            (11, 0),
            (8, 2),
            (6, 2),
            (9, 3),
            (7, 3),
            (1, 1),
        ] {
            request_orientation(&mut app, requested).unwrap();
            assert_eq!(requested_orientation(&mut app), requested);
            let d = display(&mut app);
            assert_eq!(
                app.vm
                    .invoke_instance(d, "getRotation", "()I", &[])
                    .unwrap(),
                Value::Int(rotation)
            );
        }
        assert_eq!(*backend.rotations.borrow(), [1, 0, 2, 3, 1]);
        let activity = app.android.activity_object().unwrap();
        let resources = app
            .vm
            .invoke_instance(
                activity,
                "getResources",
                "()Landroid/content/res/Resources;",
                &[],
            )
            .unwrap()
            .as_ref()
            .unwrap()
            .unwrap();
        let metrics = app
            .vm
            .invoke_instance(
                resources,
                "getDisplayMetrics",
                "()Landroid/util/DisplayMetrics;",
                &[],
            )
            .unwrap()
            .as_ref()
            .unwrap()
            .unwrap();
        for (field, value) in [
            ("widthPixels", 720),
            ("heightPixels", 1280),
            ("densityDpi", 320),
        ] {
            assert_eq!(
                app.vm.heap.get_field(metrics, field).unwrap(),
                Value::Int(value)
            );
        }
        let config = app
            .vm
            .invoke_instance(
                resources,
                "getConfiguration",
                "()Landroid/content/res/Configuration;",
                &[],
            )
            .unwrap()
            .as_ref()
            .unwrap()
            .unwrap();
        for (field, value) in [
            ("orientation", 1),
            ("screenWidthDp", 360),
            ("screenHeightDp", 640),
            ("smallestScreenWidthDp", 360),
        ] {
            assert_eq!(
                app.vm.heap.get_field(config, field).unwrap(),
                Value::Int(value)
            );
        }
        backend.reject_rotation.set(true);
        assert!(request_orientation(&mut app, 0)
            .unwrap_err()
            .to_string()
            .contains("DISPLAY_TEST_REJECTED"));
        assert_eq!(requested_orientation(&mut app), 1);
        assert_eq!(app.android.graphics_caps().unwrap().rotation, 1);
        backend.reject_rotation.set(false);
        let view = app.android.gl_requests().unwrap()[0].view;
        app.android
            .gl_choose_config(&mut app.vm, view, 0x100)
            .unwrap();
        let before = backend.rotations.borrow().len();
        let error = request_orientation(&mut app, 0).unwrap_err().to_string();
        assert!(error.contains("DISPLAY_ORIENTATION_UNAVAILABLE"), "{error}");
        assert_eq!(backend.rotations.borrow().len(), before);
        assert_eq!(requested_orientation(&mut app), 1);
        app.android.touch(&mut app.vm, 0, 680., 1200.).unwrap();
        assert!(status(&mut app).contains("T0:1:680.0:1200.0;"));
    }
}
#[test]
#[ignore = "requires scripts/build-glsurface-fixture.ps1"]
fn negotiated_portrait_cpu_frame_recreates_on_active_orientation_changes() {
    let (mut app, backend) = make("surface", true);
    app.vm
        .invoke_static(
            ACTIVITY,
            "configureInitialOrientation",
            "(IZ)V",
            &[Value::Int(1), Value::Int(1)],
        )
        .unwrap();
    app.launch().unwrap();
    assert!(app.android.gl_requests().unwrap().is_empty());
    assert_eq!((app.frame.width, app.frame.height), (360, 640));
    assert_eq!(app.android.views()[0].bounds, [0, 0, 360, 640]);
    assert!(app.frame.pixels.iter().all(|pixel| *pixel == 0xff336699));
    let before = app.android.activity_object().unwrap();
    request_orientation(&mut app, 0).unwrap();
    assert_eq!(requested_orientation(&mut app), 0);
    app.draw().unwrap();
    assert_ne!(app.android.activity_object().unwrap(), before);
    // This app explicitly requests portrait again in each onCreate.
    assert_eq!(requested_orientation(&mut app), 1);
    assert_eq!(*backend.rotations.borrow(), [1, 0, 1]);
}
#[test]
#[ignore = "requires scripts/build-glsurface-fixture.ps1"]
fn dex_custom_egl_chooser_renderer_gc_queue_dirty_pause_and_recreation_share_contracts() {
    for fixture in ["surface", "renamed"] {
        let (mut app, backend) = make(fixture, true);
        let software = app.frame.checksum();
        app.launch().unwrap();
        let before = status(&mut app);
        assert!(
            before.starts_with("M1280:720:2") && before.ends_with(":131072;"),
            "{before}"
        );
        assert_eq!(
            app.frame.checksum(),
            software,
            "software render must not fabricate a GL frame"
        );
        let request = app.android.gl_requests().unwrap().remove(0);
        assert_eq!(
            (
                request.client_version,
                request.render_mode,
                request.queued_count
            ),
            (2, 0, 1)
        );
        assert_eq!(app.android.views()[0].bounds, [0, 0, 1280, 720]);
        let view = request.view;
        app.vm.gc();
        assert_eq!(
            app.android
                .gl_choose_config(&mut app.vm, view, 0x100)
                .unwrap(),
            0x11
        );
        assert_eq!(backend.calls.borrow().len(), 2);
        app.vm.gc();
        app.android
            .gl_callback(
                &mut app.vm,
                view,
                GlEvent::SurfaceCreated {
                    context_token: 0x200,
                    config_token: 0x11,
                },
            )
            .unwrap();
        app.android
            .gl_callback(
                &mut app.vm,
                view,
                GlEvent::SurfaceChanged {
                    width: 1280,
                    height: 720,
                },
            )
            .unwrap();
        app.android
            .gl_callback(&mut app.vm, view, GlEvent::DrainQueue)
            .unwrap();
        app.android
            .gl_callback(&mut app.vm, view, GlEvent::DrawFrame)
            .unwrap();
        assert!(status(&mut app).ends_with("C16;S;Z1280:720;Q;D;"));
        assert!(!app.android.gl_requests().unwrap()[0].dirty);
        app.vm
            .invoke_static(ACTIVITY, "requestDuringNextFrame", "()V", &[])
            .unwrap();
        app.vm
            .invoke_instance(view, "requestRender", "()V", &[])
            .unwrap();
        app.android
            .gl_callback(&mut app.vm, view, GlEvent::DrawFrame)
            .unwrap();
        assert!(
            app.android.gl_requests().unwrap()[0].dirty,
            "requestRender during onDrawFrame must survive"
        );
        app.android
            .gl_callback(&mut app.vm, view, GlEvent::DrawFrame)
            .unwrap();
        assert!(!app.android.gl_requests().unwrap()[0].dirty);
        app.vm.invoke_instance(view, "onPause", "()V", &[]).unwrap();
        assert!(app.android.gl_requests().unwrap()[0].paused);
        assert!(app
            .android
            .gl_callback(&mut app.vm, view, GlEvent::DrawFrame)
            .is_err());
        app.android
            .gl_callback(&mut app.vm, view, GlEvent::SurfaceDestroyed)
            .unwrap();
        app.vm
            .invoke_instance(view, "onResume", "()V", &[])
            .unwrap();
        app.android
            .gl_choose_config(&mut app.vm, view, 0x100)
            .unwrap();
        app.android
            .gl_callback(
                &mut app.vm,
                view,
                GlEvent::SurfaceCreated {
                    context_token: 0x201,
                    config_token: 0x11,
                },
            )
            .unwrap();
        app.android
            .gl_callback(
                &mut app.vm,
                view,
                GlEvent::SurfaceChanged {
                    width: 1280,
                    height: 720,
                },
            )
            .unwrap();
        app.android
            .gl_callback(&mut app.vm, view, GlEvent::DrawFrame)
            .unwrap();
        assert!(status(&mut app).ends_with("C16;S;Z1280:720;D;"));
        app.android.touch(&mut app.vm, 0, 1100., 600.).unwrap();
        let touch = status(&mut app);
        assert!(touch.ends_with("T0:1:1100.0:600.0;"), "{touch}");
        app.vm
            .invoke_static(ACTIVITY, "consumeTouches", "()V", &[])
            .unwrap();
        app.vm.gc();
        app.android.touch(&mut app.vm, 1, 1100., 600.).unwrap();
        assert!(status(&mut app).ends_with("V;"));
        let duplicate = app
            .vm
            .invoke_instance(
                view,
                "setRenderer",
                "(Landroid/opengl/GLSurfaceView$Renderer;)V",
                &[Value::Obj(request.renderer)],
            )
            .unwrap_err()
            .to_string();
        assert!(duplicate.contains("IllegalStateException"), "{duplicate}");
        app.android.stop(&mut app.vm).unwrap();
    }
}
#[test]
#[ignore = "requires scripts/build-glsurface-fixture.ps1"]
fn missing_backend_and_unsupported_java_gl_calls_fail_explicitly() {
    let (mut missing, _) = make("surface", false);
    let error = missing.launch().unwrap_err().to_string();
    assert!(error.contains("GL_BACKEND_UNAVAILABLE"), "{error}");
    let (mut app, _) = make("surface", true);
    let view = initialize_surface(&mut app);
    let egl = (1..=app.vm.heap.len() as ObjRef)
        .find(|&object| {
            app.vm.heap.class_of(object).ok() == Some("Ljavax/microedition/khronos/egl/EGL10;")
        })
        .expect("custom chooser received a platform EGL10 proxy");
    for method in ["eglChooseConfig", "eglGetConfigAttrib"] {
        let error = app
            .vm
            .invoke_instance(egl, method, "()Z", &[])
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("API_UNAVAILABLE") && error.contains(&format!("{method}()Z")),
            "unsupported descriptors must fail before reading arguments: {error}"
        );
    }
    app.vm
        .invoke_static(ACTIVITY, "queryGlDuringNextFrame", "()V", &[])
        .unwrap();
    let error = app
        .android
        .gl_callback(&mut app.vm, view, GlEvent::DrawFrame)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("API_UNAVAILABLE") && error.contains("GL10.glClear"),
        "{error}"
    );
    assert!(app.android.gl_requests().unwrap()[0].dirty);
}
#[test]
#[ignore = "requires scripts/build-glsurface-fixture.ps1"]
fn renderer_thread_identity_is_not_fabricated_and_callback_scope_restores_main_looper() {
    let (mut app, _) = make("surface", true);
    let view = initialize_surface(&mut app);
    let main = app
        .vm
        .invoke_static(
            "Ljava/lang/Thread;",
            "currentThread",
            "()Ljava/lang/Thread;",
            &[],
        )
        .unwrap();
    app.vm
        .invoke_static(ACTIVITY, "queryThreadDuringNextFrame", "()V", &[])
        .unwrap();
    let error = app
        .android
        .gl_callback(&mut app.vm, view, GlEvent::DrawFrame)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("API_UNAVAILABLE") && error.contains("Thread identity"),
        "{error}"
    );
    assert!(matches!(
        app.vm
            .invoke_static(
                "Landroid/os/Looper;",
                "myLooper",
                "()Landroid/os/Looper;",
                &[]
            )
            .unwrap(),
        Value::Obj(_)
    ));
    assert_eq!(
        app.vm
            .invoke_static(
                "Ljava/lang/Thread;",
                "currentThread",
                "()Ljava/lang/Thread;",
                &[]
            )
            .unwrap(),
        main,
        "single interpreter main identity remains valid outside the logical callback"
    );
}
struct ThreadIdentity(Cell<u64>);
impl NativeHost for ThreadIdentity {
    fn current_thread_id(&self) -> Option<u64> {
        Some(self.0.get())
    }
    fn load(&self, _: &mut Engine, _: &str, _: bool) -> Result<(), VmExit> {
        Err(rd_vm::err::vm_error(
            "TEST_UNEXPECTED_NATIVE_LOAD",
            "no native code in SDK GL fixture",
        )
        .into())
    }
    fn invoke(
        &self,
        _: &mut Engine,
        _: &str,
        _: &str,
        _: &str,
        _: bool,
        _: &[Value],
    ) -> Result<Value, VmExit> {
        Err(rd_vm::err::vm_error(
            "TEST_UNEXPECTED_NATIVE_CALL",
            "no native code in SDK GL fixture",
        )
        .into())
    }
    fn roots(&self) -> Vec<ObjRef> {
        vec![]
    }
}
#[test]
#[ignore = "requires scripts/build-glsurface-fixture.ps1"]
fn renderer_uses_actual_platform_thread_identity_and_restores_main_after_success() {
    for fixture in ["surface", "renamed"] {
        let (mut app, _) = make(fixture, true);
        let identity = Rc::new(ThreadIdentity(Cell::new(11)));
        app.vm.bind_native(identity.clone());
        let view = initialize_surface(&mut app);
        let main = app
            .vm
            .invoke_static(
                "Ljava/lang/Thread;",
                "currentThread",
                "()Ljava/lang/Thread;",
                &[],
            )
            .unwrap();
        let main_looper = app
            .vm
            .invoke_static(
                "Landroid/os/Looper;",
                "myLooper",
                "()Landroid/os/Looper;",
                &[],
            )
            .unwrap();
        assert!(matches!(main_looper, Value::Obj(_)));
        app.vm
            .invoke_static(ACTIVITY, "queryThreadDuringNextFrame", "()V", &[])
            .unwrap();
        identity.0.set(44);
        app.android
            .gl_callback(&mut app.vm, view, GlEvent::DrawFrame)
            .unwrap();
        let renderer = app
            .vm
            .statics
            .get(&(ACTIVITY.into(), "renderThread".into()))
            .unwrap()
            .clone();
        assert!(matches!(renderer, Value::Obj(_)));
        assert_ne!(
            renderer, main,
            "renderer returned its own actual native-owner Java Thread"
        );
        app.vm.gc();
        assert_eq!(
            app.vm
                .invoke_static(
                    "Ljava/lang/Thread;",
                    "currentThread",
                    "()Ljava/lang/Thread;",
                    &[]
                )
                .unwrap(),
            renderer
        );
        identity.0.set(11);
        assert_eq!(
            app.vm
                .invoke_static(
                    "Ljava/lang/Thread;",
                    "currentThread",
                    "()Ljava/lang/Thread;",
                    &[]
                )
                .unwrap(),
            main
        );
        assert_eq!(
            app.vm
                .invoke_static(
                    "Landroid/os/Looper;",
                    "myLooper",
                    "()Landroid/os/Looper;",
                    &[]
                )
                .unwrap(),
            main_looper
        );
        assert_eq!(
            app.vm
                .invoke_static(
                    "Landroid/os/Looper;",
                    "getMainLooper",
                    "()Landroid/os/Looper;",
                    &[]
                )
                .unwrap(),
            main_looper
        );
    }
}
