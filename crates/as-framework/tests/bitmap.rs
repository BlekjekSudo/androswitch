//! Original SDK PNG/resource/raster fixtures on the actual no_std VM.
use as_framework::{App, GraphicsCaps};
use as_platform::{host::HostPlatform, host_fs::HostFileSystem};
use rd_vm::{Value, VmExit};
use std::{path::PathBuf, rc::Rc};
const ACTIVITY: &str = "Lorg/androswitch/contracts/BitmapActivity;";
#[allow(clippy::assertions_on_constants)]
fn app(name: &str, density: i32) -> App {
    assert!(rd_vm::PORTABLE_CORE);
    let root = std::env::temp_dir().join(format!(
        "as-bitmap-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let fs = Rc::new(HostFileSystem::new(root).unwrap());
    let pm = as_package::PackageManager::new(fs.clone());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../target/bitmap-fixture/{name}.apk"));
    let record = pm
        .install(
            std::fs::read(path).expect("run fixtures/m9/bitmap/build.ps1"),
            true,
            None,
        )
        .unwrap();
    let mut app = App::new(
        pm.load_apk(&record.package, None).unwrap().1,
        fs,
        Rc::new(HostPlatform::default()),
        64,
        32,
        None,
    )
    .unwrap();
    app.android
        .bind_graphics_caps(GraphicsCaps {
            width: 64 * (density / 160),
            height: 32 * (density / 160),
            density: density as f32 / 160.,
            density_dpi: density,
            // Host-supplied display metadata only; no EGL/GPU execution is
            // claimed or requested by this software Canvas fixture.
            gles_version: 0x20000,
            rotation: 0,
        })
        .unwrap();
    app.launch().unwrap();
    app
}
fn field(app: &App, name: &str) -> Value {
    app.vm.statics[&(ACTIVITY.into(), name.into())].clone()
}
fn call(app: &mut App, name: &str, sig: &str, args: &[Value]) -> Result<Value, VmExit> {
    app.vm.invoke_static(ACTIVITY, name, sig, args)
}
fn bitmap(
    app: &mut App,
    object: &Value,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Value, VmExit> {
    app.vm
        .invoke_instance(object.as_ref().unwrap().unwrap(), name, sig, args)
}
fn decode(app: &mut App, id_method: &str) -> Result<Value, VmExit> {
    let id = call(app, id_method, "()I", &[])?;
    call(app, "decode", "(I)Landroid/graphics/Bitmap;", &[id])
}
fn integer(value: Value) -> i32 {
    value.as_int().unwrap()
}
fn error(result: Result<Value, VmExit>, code: &str) {
    assert!(
        matches!(result,Err(VmExit::Error(e)) if e.code==code),
        "expected {code}"
    );
}
fn exception(result: Result<Value, VmExit>, class: &str) {
    assert!(
        matches!(result,Err(VmExit::Exception(e)) if e.class==class),
        "expected {class}"
    );
}

#[test]
fn renamed_apks_decode_real_rgba_palette_alias_density_and_sdk_variants() {
    for name in ["bitmap", "renamed"] {
        let mut app = app(name, 320);
        let object = field(&app, "bitmap");
        let density = field(&app, "density");
        let alpha = field(&app, "alpha");
        assert_eq!(
            integer(bitmap(&mut app, &object, "getWidth", "()I", &[]).unwrap()),
            2
        );
        assert_eq!(
            integer(bitmap(&mut app, &object, "getHeight", "()I", &[]).unwrap()),
            2
        );
        assert_eq!(
            integer(bitmap(&mut app, &object, "getDensity", "()I", &[]).unwrap()),
            320
        );
        assert_eq!(
            integer(
                bitmap(
                    &mut app,
                    &object,
                    "getPixel",
                    "(II)I",
                    &[Value::Int(0), Value::Int(0)]
                )
                .unwrap()
            ) as u32,
            0xffff0000
        );
        assert_eq!(
            integer(bitmap(&mut app, &density, "getWidth", "()I", &[]).unwrap()),
            4
        );
        assert_eq!(
            integer(bitmap(&mut app, &density, "getDensity", "()I", &[]).unwrap()),
            320
        );
        assert_eq!(
            integer(
                bitmap(
                    &mut app,
                    &density,
                    "getPixel",
                    "(II)I",
                    &[Value::Int(0), Value::Int(0)]
                )
                .unwrap()
            ) as u32,
            0xff0000ff
        );
        assert_eq!(
            integer(
                bitmap(
                    &mut app,
                    &alpha,
                    "getPixel",
                    "(II)I",
                    &[Value::Int(0), Value::Int(0)]
                )
                .unwrap()
            ) as u32,
            0x80ffffff
        );
        assert_eq!(
            integer(
                bitmap(
                    &mut app,
                    &alpha,
                    "getPixel",
                    "(II)I",
                    &[Value::Int(1), Value::Int(0)]
                )
                .unwrap()
            ),
            0
        );
        let versioned = decode(&mut app, "futureId").unwrap();
        assert_eq!(
            integer(bitmap(&mut app, &versioned, "getWidth", "()I", &[]).unwrap()),
            8
        );
        assert_eq!(
            integer(
                bitmap(
                    &mut app,
                    &versioned,
                    "getPixel",
                    "(II)I",
                    &[Value::Int(0), Value::Int(0)]
                )
                .unwrap()
            ) as u32,
            0xff0000ff
        );
        app.vm.gc();
        assert_eq!(
            integer(
                bitmap(
                    &mut app,
                    &object,
                    "getPixel",
                    "(II)I",
                    &[Value::Int(1), Value::Int(1)]
                )
                .unwrap()
            ) as u32,
            0xffffff00
        );
    }
}
#[test]
fn canvas_uses_real_pixels_alpha_rects_and_clips_to_the_actual_frame() {
    let mut app = app("bitmap", 160);
    assert_eq!((app.frame.width, app.frame.height), (64, 32));
    let pixel = |x: usize, y: usize| app.frame.pixels[y * 64 + x];
    assert_eq!(pixel(20, 4), 0xffff0000);
    assert_eq!(pixel(48, 4), 0xff00ff00);
    assert_eq!(pixel(20, 24), 0xff0000ff);
    assert_eq!(pixel(48, 24), 0xffffff00);
    assert_eq!(pixel(2, 2), 0xffff8080); // half-transparent white over red
    assert_eq!(pixel(12, 12), 0xffff0000);
    assert_eq!(pixel(13, 12), 0xff00ff00);
    call(&mut app, "rects", "()V", &[]).unwrap();
    assert_eq!(field(&app, "rectResult"), Value::Int(5890));
    assert!(integer(field(&app, "draws")) > 0);
    app.vm
        .statics
        .insert((ACTIVITY.into(), "mode".into()), Value::Int(1));
    assert!(matches!(app.draw(),Err(VmExit::Error(error)) if error.code=="API_UNAVAILABLE"));
    app.vm
        .statics
        .insert((ACTIVITY.into(), "mode".into()), Value::Int(2));
    assert!(
        matches!(app.draw(),Err(VmExit::Exception(error)) if error.class=="Ljava/lang/IllegalArgumentException;")
    );
    app.vm
        .statics
        .insert((ACTIVITY.into(), "mode".into()), Value::Int(3));
    assert!(
        matches!(app.draw(),Err(VmExit::Exception(error)) if error.class=="Ljava/lang/IllegalArgumentException;")
    );
    app.vm
        .statics
        .insert((ACTIVITY.into(), "mode".into()), Value::Int(0));
    app.draw().unwrap();
}
#[test]
fn malformed_missing_unsupported_options_qualifiers_and_size_fail_truthfully() {
    let mut app = app("bitmap", 160);
    assert_eq!(decode(&mut app, "corruptId").unwrap(), Value::Null);
    assert_eq!(decode(&mut app, "missingId").unwrap(), Value::Null);
    error(decode(&mut app, "unsupportedId"), "API_UNAVAILABLE");
    error(decode(&mut app, "hugeId"), "BITMAP_MEMORY_BUDGET");
    error(decode(&mut app, "localeId"), "API_UNAVAILABLE");
    error(decode(&mut app, "orientationId"), "API_UNAVAILABLE");
    error(
        call(&mut app, "options", "()Landroid/graphics/Bitmap;", &[]),
        "API_UNAVAILABLE",
    );
    assert!(
        call(&mut app, "nullOptions", "()Landroid/graphics/Bitmap;", &[])
            .unwrap()
            .as_ref()
            .unwrap()
            .is_some()
    );
    assert_eq!(
        call(
            &mut app,
            "nullResources",
            "()Landroid/graphics/Bitmap;",
            &[]
        )
        .unwrap(),
        Value::Null
    );
    exception(
        call(&mut app, "invalidPixel", "()I", &[]),
        "Ljava/lang/IllegalArgumentException;",
    );
}
#[test]
fn recycle_and_gc_release_pixel_budget_without_reusing_stale_objects() {
    let mut app = app("bitmap", 160);
    let before = app.android.bitmap_memory_bytes();
    assert!(before > 0);
    call(&mut app, "retain", "(I)V", &[Value::Int(7)]).unwrap();
    assert!(app.android.bitmap_memory_bytes() > 28 << 20);
    error(
        call(&mut app, "retain", "(I)V", &[Value::Int(8)]),
        "BITMAP_MEMORY_BUDGET",
    );
    call(&mut app, "clearRetained", "()V", &[]).unwrap();
    app.vm.gc();
    assert!(app.android.bitmap_memory_bytes() <= before);
    let object = field(&app, "bitmap");
    call(&mut app, "recycle", "()V", &[]).unwrap();
    assert_eq!(
        bitmap(&mut app, &object, "isRecycled", "()Z", &[]).unwrap(),
        Value::Int(1)
    );
    exception(
        call(&mut app, "recycledPixel", "()I", &[]),
        "Ljava/lang/IllegalStateException;",
    );
    let bytes = app.android.bitmap_memory_bytes();
    assert!(bytes < before);
    app.vm
        .statics
        .insert((ACTIVITY.into(), "bitmap".into()), Value::Null);
    app.vm
        .statics
        .insert((ACTIVITY.into(), "density".into()), Value::Null);
    app.vm
        .statics
        .insert((ACTIVITY.into(), "alpha".into()), Value::Null);
    app.vm.gc();
    assert_eq!(app.android.bitmap_memory_bytes(), 0);
}
