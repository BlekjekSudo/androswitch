//! Regression tests for issue #270 (`java.lang.String` in `jlstring.rs`):
//! 1. `String.repeat` and `String.format` check `vm.heap.budget()` BEFORE allocating multi-gigabyte host buffers, throwing catchable `OutOfMemoryError`.
//! 2. `String(byte[], offset, length)` / `String(int[], offset, count)` and `startsWith(prefix, offset)` use checked `usize` arithmetic instead of panicking on `i32` overflow (`1 + Integer.MAX_VALUE`).
//! 3. `jlstring::decode` decodes `UTF-16` (with `FE FF` / `FF FE` BOM detection and BE default), `UTF-16BE`, and `UTF-16LE` (including emoji surrogate pairs), matching `getBytes(...)`.
//! 4. `String.format("%0$s", ...)` throws format exception instead of silently aliasing `%1$s`, and `String.matches("(?x)abc # comment")` succeeds without trailing newline.

use rd_vm::err::VmExit;
use rd_vm::heap::ElemKind;
use rd_vm::value::Value;
use rd_vm::{Engine, VmConfig};

const STRING: &str = "Ljava/lang/String;";
const OBJECT: &str = "Ljava/lang/Object;";

fn engine_with_budget(heap_budget: usize) -> Engine {
    Engine::new(
        Vec::new(),
        VmConfig {
            heap_budget,
            ..VmConfig::default()
        },
    )
}

fn s(e: &mut Engine, txt: &str) -> u32 {
    e.heap.alloc_string(txt.to_string()).unwrap()
}

fn assert_exc_class(res: Result<Value, VmExit>, expected: &str) {
    match res {
        Err(VmExit::Exception(t)) => assert_eq!(
            t.class, expected,
            "unexpected exception {} ({:?})",
            t.class, t.message
        ),
        other => panic!("expected exception {expected}, got {other:?}"),
    }
}

#[test]
fn issue_270_repeat_and_format_enforce_heap_budget_without_host_oom_or_panic() {
    let mut e = engine_with_budget(4 * 1024 * 1024); // 4 MiB heap budget
    let abcd = s(&mut e, "abcd");

    // 1. `"abcd".repeat(Integer.MAX_VALUE)` would be 8.59 GiB (or capacity overflow on 32-bit);
    // must fail closed with Java OutOfMemoryError before host allocation!
    assert_exc_class(
        e.invoke_instance(
            abcd,
            "repeat",
            "(I)Ljava/lang/String;",
            &[Value::Int(i32::MAX)],
        ),
        "Ljava/lang/OutOfMemoryError;",
    );

    // `"".repeat(Integer.MAX_VALUE)` is 0 bytes in JDK -> succeeds with `""`!
    let empty = s(&mut e, "");
    let rep_empty = e
        .invoke_instance(
            empty,
            "repeat",
            "(I)Ljava/lang/String;",
            &[Value::Int(i32::MAX)],
        )
        .unwrap();
    match rep_empty {
        Value::Obj(r) => assert_eq!(e.heap.as_str(r).unwrap(), ""),
        other => panic!("expected empty string, got {other:?}"),
    }

    // Small valid repeat still works!
    let rep3 = e
        .invoke_instance(abcd, "repeat", "(I)Ljava/lang/String;", &[Value::Int(3)])
        .unwrap();
    match rep3 {
        Value::Obj(r) => assert_eq!(e.heap.as_str(r).unwrap(), "abcdabcdabcd"),
        other => panic!("expected repeated string, got {other:?}"),
    }

    // 2. `String.format("%9999999999s", "x")` and `String.format("%.9999999999f", 1.0)`
    // must throw OutOfMemoryError instead of allocating ~10 GiB on host!
    let x_str = Value::Obj(s(&mut e, "x"));
    let args_s = e
        .heap
        .alloc_array_zeroed(ElemKind::Obj, 1, "[Ljava/lang/Object;")
        .unwrap();
    e.heap.array_elems_mut(args_s).unwrap()[0] = x_str;

    let fmt_huge_width = Value::Obj(s(&mut e, "%9999999999s"));
    assert_exc_class(
        e.invoke_static(
            STRING,
            "format",
            "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
            &[fmt_huge_width, Value::Obj(args_s)],
        ),
        "Ljava/lang/OutOfMemoryError;",
    );

    let boxed_d = e
        .heap
        .alloc_instance(
            "Ljava/lang/Double;".to_string(),
            vec![("value".to_string(), Value::Double(1.0))],
        )
        .unwrap();
    let args_f = e
        .heap
        .alloc_array_zeroed(ElemKind::Obj, 1, "[Ljava/lang/Object;")
        .unwrap();
    e.heap.array_elems_mut(args_f).unwrap()[0] = Value::Obj(boxed_d);

    let fmt_huge_prec = Value::Obj(s(&mut e, "%.9999999999f"));
    assert_exc_class(
        e.invoke_static(
            STRING,
            "format",
            "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
            &[fmt_huge_prec, Value::Obj(args_f)],
        ),
        "Ljava/lang/OutOfMemoryError;",
    );

    let _ = OBJECT;
}

#[test]
fn issue_270_string_ctors_and_starts_with_do_not_panic_on_i32_overflow() {
    let mut e = Engine::new(Vec::new(), VmConfig::default());

    // `new String(new byte[]{65}, 1, Integer.MAX_VALUE)` -> StringIndexOutOfBoundsException (not i32 overflow panic!)
    let bytes = e.heap.alloc_array_zeroed(ElemKind::Int, 1, "[B").unwrap();
    e.heap.array_elems_mut(bytes).unwrap()[0] = Value::Int(65);
    let shell_b = e
        .heap
        .alloc_instance(STRING.to_string(), Vec::new())
        .unwrap();
    assert_exc_class(
        e.invoke_instance(
            shell_b,
            "<init>",
            "([BII)V",
            &[Value::Obj(bytes), Value::Int(1), Value::Int(i32::MAX)],
        ),
        "Ljava/lang/StringIndexOutOfBoundsException;",
    );

    // `new String(new int[]{65}, 1, Integer.MAX_VALUE)` -> StringIndexOutOfBoundsException
    let ints = e.heap.alloc_array_zeroed(ElemKind::Int, 1, "[I").unwrap();
    e.heap.array_elems_mut(ints).unwrap()[0] = Value::Int(65);
    let shell_i = e
        .heap
        .alloc_instance(STRING.to_string(), Vec::new())
        .unwrap();
    assert_exc_class(
        e.invoke_instance(
            shell_i,
            "<init>",
            "([III)V",
            &[Value::Obj(ints), Value::Int(1), Value::Int(i32::MAX)],
        ),
        "Ljava/lang/StringIndexOutOfBoundsException;",
    );

    // `"hello".startsWith("he", Integer.MAX_VALUE)` -> false (no usize overflow!)
    let hello = s(&mut e, "hello");
    let he = Value::Obj(s(&mut e, "he"));
    assert_eq!(
        e.invoke_instance(
            hello,
            "startsWith",
            "(Ljava/lang/String;I)Z",
            &[he, Value::Int(i32::MAX)],
        )
        .unwrap()
        .as_int()
        .unwrap(),
        0
    );
}

#[test]
fn issue_270_utf16_be_le_and_bom_roundtrip_cleanly() {
    let mut e = Engine::new(Vec::new(), VmConfig::default());
    let orig_txt = "Olá 😀 UTF-16!";
    let orig = s(&mut e, orig_txt);

    for cs_name in ["UTF-16LE", "UTF-16BE", "UTF-16"] {
        let cs = Value::Obj(s(&mut e, cs_name));
        let encoded = e
            .invoke_instance(
                orig,
                "getBytes",
                "(Ljava/lang/String;)[B",
                std::slice::from_ref(&cs),
            )
            .unwrap();
        let shell = e
            .heap
            .alloc_instance(STRING.to_string(), Vec::new())
            .unwrap();
        e.invoke_instance(shell, "<init>", "([BLjava/lang/String;)V", &[encoded, cs])
            .unwrap();
        assert_eq!(
            e.heap.as_str(shell).unwrap(),
            orig_txt,
            "failed round-trip for {cs_name}"
        );
    }

    // UTF-16 with little-endian BOM `FF FE` decodes as little-endian and strips BOM!
    let le_bom_bytes: [u8; 6] = [0xFF, 0xFE, b'h', 0x00, b'i', 0x00];
    let b_arr = e
        .heap
        .alloc_array_zeroed(ElemKind::Int, le_bom_bytes.len(), "[B")
        .unwrap();
    for (slot, b) in e
        .heap
        .array_elems_mut(b_arr)
        .unwrap()
        .iter_mut()
        .zip(le_bom_bytes.iter())
    {
        *slot = Value::Int((*b as i8) as i32);
    }
    let cs_utf16 = Value::Obj(s(&mut e, "UTF-16"));
    let shell_bom = e
        .heap
        .alloc_instance(STRING.to_string(), Vec::new())
        .unwrap();
    e.invoke_instance(
        shell_bom,
        "<init>",
        "([BLjava/lang/String;)V",
        &[Value::Obj(b_arr), cs_utf16],
    )
    .unwrap();
    assert_eq!(e.heap.as_str(shell_bom).unwrap(), "hi");
}

#[test]
fn issue_270_format_rejects_zero_arg_index_and_string_matches_handles_comments() {
    let mut e = Engine::new(Vec::new(), VmConfig::default());
    let arg_a = Value::Obj(s(&mut e, "a"));
    let arr = e
        .heap
        .alloc_array_zeroed(ElemKind::Obj, 1, "[Ljava/lang/Object;")
        .unwrap();
    e.heap.array_elems_mut(arr).unwrap()[0] = arg_a;

    // `%0$s` must NOT silently return `"a"` (`%1$s`).
    let bad_fmt = Value::Obj(s(&mut e, "%0$s"));
    assert_exc_class(
        e.invoke_static(
            STRING,
            "format",
            "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
            &[bad_fmt, Value::Obj(arr)],
        ),
        "Ljava/util/UnknownFormatConversionException;",
    );

    // Valid `%1$s` still works!
    let good_fmt = Value::Obj(s(&mut e, "%1$s"));
    let formatted = e
        .invoke_static(
            STRING,
            "format",
            "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
            &[good_fmt, Value::Obj(arr)],
        )
        .unwrap();
    match formatted {
        Value::Obj(r) => assert_eq!(e.heap.as_str(r).unwrap(), "a"),
        other => panic!("expected String Obj, got {other:?}"),
    }

    // `"abc".matches("(?x)abc # trailing comment")` works without trailing `\n`!
    let abc = s(&mut e, "abc");
    let pat = Value::Obj(s(&mut e, "(?x)abc # trailing comment without newline"));
    assert_eq!(
        e.invoke_instance(abc, "matches", "(Ljava/lang/String;)Z", &[pat])
            .unwrap()
            .as_int()
            .unwrap(),
        1
    );
}
