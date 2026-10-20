#![cfg(feature = "native")]
use rd_testsupport::{apkfix::*, vm::*};
use rd_vm::{Engine, Value, VmConfig, VmExit};
const C: &str = "Ltest/Native;";
fn engine(abi: &str) -> Engine {
    let mut b = DexBuilder::new();
    let c = b.class(C, "Ljava/lang/Object;");
    for (name, ret, params) in [
        ("add", "I", vec!["I", "I"]),
        ("wide", "J", vec!["I", "J", "I"]),
        ("floating", "D", vec!["F", "D"]),
        ("many", "I", vec!["I"; 8]),
        ("greeting", "Ljava/lang/String;", vec![]),
        ("echo", "Ljava/lang/String;", vec!["Ljava/lang/String;"]),
        ("sum", "I", vec!["[I"]),
        ("mutate", "V", vec!["[I"]),
        ("throws", "V", vec![]),
        ("callback", "I", vec!["I"]),
        ("registered", "I", vec!["I"]),
        ("spin", "I", vec![]),
        ("format", "Ljava/lang/String;", vec!["I"]),
        ("missing", "I", vec![]),
        ("callbackv", "I", vec!["I"]),
        ("mixv", "J", vec![]),
        (
            "extras",
            "Ljava/lang/String;",
            vec!["Ljava/lang/String;", "F"],
        ),
    ] {
        b.direct(c, name, ret, params, 0x109, None);
    }
    let mut units = op23x(0x90, 0, 1, 1);
    units.extend(op11x(0x0f, 0));
    let code = b.code(2, 1, 0, units);
    b.direct(c, "twice", "I", vec!["I"], 0x9, Some(code));
    // static long mix(int a, long b, double d) { return a + b + (long) d; }
    // locals v0..v3; ins: a=v4, b=v5:v6, d=v7:v8
    let mut units = op12x(0x81, 0, 4); // int-to-long v0, v4
    units.extend(op12x(0xbb, 0, 5)); // add-long/2addr v0, v5
    units.extend(op12x(0x8b, 2, 7)); // double-to-long v2, v7
    units.extend(op12x(0xbb, 0, 2)); // add-long/2addr v0, v2
    units.extend(op11x(0x10, 0)); // return-wide v0
    let code = b.code(9, 5, 0, units);
    b.direct(c, "mix", "J", vec!["I", "J", "D"], 0x9, Some(code));
    let dex = b.finish();
    let mut z = ZipFix::new();
    z.add("AndroidManifest.xml", build_manifest_axml("test", C));
    z.add("classes.dex", dex.clone());
    let path = format!(
        "{}/../../golden/native/artifacts/{abi}.so",
        env!("CARGO_MANIFEST_DIR")
    );
    z.add(
        &format!("lib/{abi}/libfixture.so"),
        std::fs::read(path).expect("run golden/native/build.sh"),
    );
    let apk = rd_apk::Apk::from_bytes(z.build()).unwrap();
    let mut e = Engine::new(vec![rd_dex::Dex::parse(dex).unwrap()], VmConfig::default());
    e.set_resources(apk);
    rd_vm::native::load(&mut e, "fixture").unwrap();
    e
}
#[test]
fn native_dalvik_jni_all_supported_abis() {
    for abi in ["x86_64", "arm64-v8a", "armeabi-v7a"] {
        eprintln!("native ABI {abi}");
        let mut e = engine(abi);
        assert_eq!(
            e.invoke_static(C, "add", "(II)I", &[Value::Int(10), Value::Int(25)])
                .unwrap(),
            Value::Int(42),
            "{abi}"
        );
        assert_eq!(
            e.invoke_static(
                C,
                "wide",
                "(IJI)J",
                &[Value::Int(-3), Value::Long(0x1234_5678_9abc), Value::Int(8)]
            )
            .unwrap(),
            Value::Long(0x1234_5678_9ac1),
            "{abi}"
        );
        assert_eq!(
            e.invoke_static(
                C,
                "floating",
                "(FD)D",
                &[Value::Float(1.25), Value::Double(2.5)]
            )
            .unwrap(),
            Value::Double(3.75),
            "{abi}"
        );
        assert_eq!(
            e.invoke_static(
                C,
                "many",
                "(IIIIIIII)I",
                &(1..=8).map(Value::Int).collect::<Vec<_>>()
            )
            .unwrap(),
            Value::Int(36),
            "{abi}"
        );
        eprintln!("greeting {abi}");
        let Value::Obj(s) = e
            .invoke_static(C, "greeting", "()Ljava/lang/String;", &[])
            .unwrap()
        else {
            panic!("string")
        };
        assert_eq!(e.heap.as_str(s).unwrap(), "hello\0world 😀", "{abi}");
        let Value::Obj(s2) = e
            .invoke_static(
                C,
                "echo",
                "(Ljava/lang/String;)Ljava/lang/String;",
                &[Value::Obj(s)],
            )
            .unwrap()
        else {
            panic!("echo")
        };
        assert_eq!(e.heap.as_str(s2).unwrap(), "hello\0world 😀");
        let a = e
            .heap
            .alloc_array(rd_vm::heap::ElemKind::Int, 4, "[I")
            .unwrap();
        *e.heap.array_elems_mut(a).unwrap() =
            vec![Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(4)];
        assert_eq!(
            e.invoke_static(C, "sum", "([I)I", &[Value::Obj(a)])
                .unwrap(),
            Value::Int(10),
            "{abi}"
        );
        e.invoke_static(C, "mutate", "([I)V", &[Value::Obj(a)])
            .unwrap();
        assert_eq!(
            e.heap.as_array_elems(a).unwrap().0,
            &[Value::Int(1), Value::Int(19), Value::Int(23), Value::Int(4)]
        );
        assert_eq!(
            e.invoke_static(C, "callback", "(I)I", &[Value::Int(21)])
                .unwrap(),
            Value::Int(42),
            "{abi}"
        );
        assert_eq!(
            e.invoke_static(C, "registered", "(I)I", &[Value::Int(14)])
                .unwrap(),
            Value::Int(42),
            "{abi}"
        );
        let err = e.invoke_static(C, "throws", "()V", &[]).unwrap_err();
        let VmExit::Exception(t) = err else {
            panic!("{abi}: {err:?}")
        };
        assert_eq!(t.class, "Ljava/lang/IllegalArgumentException;");
        assert_eq!(t.message.as_deref(), Some("from native"));
        // Pending exception is cleared after crossing back to Java.
        assert_eq!(
            e.invoke_static(C, "add", "(II)I", &[Value::Int(0), Value::Int(0)])
                .unwrap(),
            Value::Int(7)
        );
        // printf family through the libc shim (integer conversions)
        let Value::Obj(f) = e
            .invoke_static(C, "format", "(I)Ljava/lang/String;", &[Value::Int(-42)])
            .unwrap()
        else {
            panic!("format")
        };
        assert_eq!(
            e.heap.as_str(f).unwrap(),
            "rd|-42|000ab|z  |-1234567890123",
            "{abi}"
        );
        // GetStringUTFRegion/GetStringRegion, ExceptionCheck (slot 228),
        // floating varargs (%f %g %e) and libm, per ABI calling convention
        let s = e.heap.alloc_string("héllo".into()).unwrap();
        let Value::Obj(x) = e
            .invoke_static(
                C,
                "extras",
                "(Ljava/lang/String;F)Ljava/lang/String;",
                &[Value::Obj(s), Value::Float(5.5)],
            )
            .unwrap()
        else {
            panic!("extras")
        };
        assert_eq!(
            e.heap.as_str(x).unwrap(),
            "éll|104,233|0|1.500|5.25|1.23e+04",
            "{abi}"
        );
        // Call*MethodV: guest va_list decoded per ABI (int, long, double)
        assert_eq!(
            e.invoke_static(C, "callbackv", "(I)I", &[Value::Int(21)])
                .unwrap(),
            Value::Int(42),
            "{abi}"
        );
        assert_eq!(
            e.invoke_static(C, "mixv", "()J", &[]).unwrap(),
            Value::Long(1_000_000_000_005),
            "{abi}"
        );
        // an absent libc import is a typed NOT_IMPLEMENTED, never a crash
        let err = e.invoke_static(C, "missing", "()I", &[]).unwrap_err();
        let VmExit::Error(x) = err else {
            panic!("{abi}: {err:?}")
        };
        assert_eq!(x.code, "NOT_IMPLEMENTED", "{abi}");
        assert!(x.cause.contains("getaddrinfo"), "{abi}: {}", x.cause);
        assert!(e.native_report()["instructions"].as_u64().unwrap() > 0);
    }
}
#[test]
fn repeat_load_does_not_repeat_constructors() {
    let mut e = engine("x86_64");
    rd_vm::native::load(&mut e, "fixture").unwrap();
    assert_eq!(
        e.invoke_static(C, "registered", "(I)I", &[Value::Int(10)])
            .unwrap(),
        Value::Int(30)
    );
}

#[test]
fn native_calls_use_the_current_fuel_window_not_lifetime_total() {
    for abi in ["x86_64", "arm64-v8a", "armeabi-v7a"] {
        let mut e = engine(abi);
        e.config.fuel = 1000;
        for _ in 0..500 {
            assert_eq!(
                e.invoke_static(C, "twice", "(I)I", &[Value::Int(2)])
                    .unwrap(),
                Value::Int(4)
            );
        }
        assert!(
            matches!(e.invoke_static(C,"twice","(I)I",&[Value::Int(2)]),Err(VmExit::Error(err)) if err.code=="VM_FUEL")
        );
        e.reset_fuel_window();
        assert_eq!(
            e.invoke_static(C, "add", "(II)I", &[Value::Int(10), Value::Int(25)])
                .unwrap(),
            Value::Int(42),
            "{abi}"
        );
        assert_eq!(
            e.invoke_static(C, "twice", "(I)I", &[Value::Int(2)])
                .unwrap(),
            Value::Int(4)
        );
        assert!(
            e.fuel_used() > e.config.fuel,
            "lifetime accounting must remain observable"
        );
    }
}
