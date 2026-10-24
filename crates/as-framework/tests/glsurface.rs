//! A recording backend checks API scheduling; it does not claim GPU execution.
use as_framework::{App, AppOptions, EglConfigBackend, EglConfigs, GlEvent, GraphicsCaps};
use as_platform::{host::HostPlatform, host_fs::HostFileSystem};
use rd_vm::{ObjRef, Value, VmExit};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};
const ACTIVITY: &str = "Lorg/androswitch/graphics/SurfaceActivity;";
static NEXT: AtomicU64 = AtomicU64::new(0);
#[derive(Default)]
struct RecordingEgl {
    calls: RefCell<Vec<(u64, Vec<i32>, usize)>>,
}
impl EglConfigBackend for RecordingEgl {
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
        assert!(
            touch.contains("T0:1:1100") && touch.ends_with("600;"),
            "{touch}"
        );
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
}
