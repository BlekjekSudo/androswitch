//! Regression tests for Issue #266: `jio_chars.rs` bounds/overflow hardening,
//! DEX `Reader`/`InputStream` oversized return clamping, UTF-16 surrogate pair
//! preservation across `Writer`/`PrintWriter`/`PrintStream`, closed `Reader`
//! semantics, and JDK `NullPointerException` contracts.

mod common;

use common::*;
use rd_vm::err::VmExit;
use rd_vm::heap::ElemKind;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef};

const SW: &str = "Ljava/io/StringWriter;";
const OSW: &str = "Ljava/io/OutputStreamWriter;";
const PW: &str = "Ljava/io/PrintWriter;";
const PS: &str = "Ljava/io/PrintStream;";
const FW: &str = "Ljava/io/FileWriter;";
const SR: &str = "Ljava/io/StringReader;";
const ISR: &str = "Ljava/io/InputStreamReader;";
const BR: &str = "Ljava/io/BufferedReader;";
const FR: &str = "Ljava/io/FileReader;";
const BAOS: &str = "Ljava/io/ByteArrayOutputStream;";
const BAIS: &str = "Ljava/io/ByteArrayInputStream;";
const BOS: &str = "Ljava/io/BufferedOutputStream;";

fn alloc_obj(vm: &mut Engine, class: &str) -> ObjRef {
    vm.heap
        .alloc_instance(class.to_string(), Vec::new())
        .expect("alloc_instance")
}

fn alloc_jstr(vm: &mut Engine, s: &str) -> Value {
    Value::Obj(vm.heap.alloc_string(s.to_string()).expect("alloc_string"))
}

fn alloc_chars(vm: &mut Engine, len: usize) -> ObjRef {
    vm.heap
        .alloc_array_zeroed(ElemKind::Int, len, "[C")
        .expect("alloc [C")
}

fn alloc_bytes(vm: &mut Engine, data: &[u8]) -> ObjRef {
    let arr = vm
        .heap
        .alloc_array_zeroed(ElemKind::Int, data.len(), "[B")
        .expect("alloc [B");
    let elems = vm.heap.array_elems_mut(arr).expect("elems");
    for (slot, b) in elems.iter_mut().zip(data) {
        *slot = Value::Int(i32::from(*b as i8));
    }
    arr
}

fn read_bytes(vm: &Engine, arr: ObjRef) -> Vec<u8> {
    vm.heap
        .as_array_elems(arr)
        .expect("as_array_elems")
        .0
        .iter()
        .map(|v| v.as_int().unwrap_or(0) as u8)
        .collect()
}

fn assert_thrown(res: Result<Value, VmExit>, expected_class: &str) {
    match res {
        Err(VmExit::Exception(t)) => {
            assert_eq!(
                t.class, expected_class,
                "expected {expected_class}, got {}",
                t.class
            );
        }
        other => panic!("expected Exception({expected_class}), got {other:?}"),
    }
}

#[test]
fn reader_and_bais_read_reject_negative_and_overflowing_bounds() {
    let mut vm = engine_of(&DexBuilder::new());

    // StringReader("abcd")
    let sr = alloc_obj(&mut vm, SR);
    let s = alloc_jstr(&mut vm, "abcd");
    vm.invoke_instance(sr, "<init>", "(Ljava/lang/String;)V", &[s])
        .expect("StringReader.<init>");
    let cbuf = alloc_chars(&mut vm, 4);

    for (off, len) in [
        (-1, 1),
        (1, -1),
        (-1, -1),
        (2, i32::MAX),
        (i32::MAX, 2),
        (3, 2),
        (5, 0),
    ] {
        let res = vm.invoke_instance(
            sr,
            "read",
            "([CII)I",
            &[Value::Obj(cbuf), Value::Int(off), Value::Int(len)],
        );
        assert_thrown(res, "Ljava/lang/IndexOutOfBoundsException;");
    }

    // Valid zero-length read at exact end of buffer succeeds and returns 0.
    let zero = vm
        .invoke_instance(
            sr,
            "read",
            "([CII)I",
            &[Value::Obj(cbuf), Value::Int(4), Value::Int(0)],
        )
        .expect("zero-len read");
    assert_eq!(zero, Value::Int(0));

    // ByteArrayInputStream([10, 20, 30, 40])
    let src = alloc_bytes(&mut vm, &[10, 20, 30, 40]);
    let bais = alloc_obj(&mut vm, BAIS);
    vm.invoke_instance(bais, "<init>", "([B)V", &[Value::Obj(src)])
        .expect("BAIS.<init>");
    let bbuf = alloc_bytes(&mut vm, &[0; 4]);

    for (off, len) in [
        (-1, 1),
        (0, -1),
        (2, i32::MAX),
        (i32::MAX, i32::MAX),
        (3, 2),
        (5, 0),
    ] {
        let res = vm.invoke_instance(
            bais,
            "read",
            "([BII)I",
            &[Value::Obj(bbuf), Value::Int(off), Value::Int(len)],
        );
        assert_thrown(res, "Ljava/lang/IndexOutOfBoundsException;");
    }

    let zero_b = vm
        .invoke_instance(
            bais,
            "read",
            "([BII)I",
            &[Value::Obj(bbuf), Value::Int(4), Value::Int(0)],
        )
        .expect("BAIS zero-len read");
    assert_eq!(zero_b, Value::Int(0));
}

#[test]
fn writer_append_and_slice_reject_overflowing_indices() {
    let mut vm = engine_of(&DexBuilder::new());
    let sw = alloc_obj(&mut vm, SW);
    vm.invoke_instance(sw, "<init>", "()V", &[])
        .expect("SW.<init>");
    let hello = alloc_jstr(&mut vm, "hello");

    // Previously `b - a` overflowed i32 on `(1, i32::MIN)`.
    for (start, end) in [(1, i32::MIN), (-1, 2), (3, 2), (0, 6), (2, i32::MAX)] {
        let res = vm.invoke_instance(
            sw,
            "append",
            "(Ljava/lang/CharSequence;II)Ljava/io/StringWriter;",
            &[hello.clone(), Value::Int(start), Value::Int(end)],
        );
        assert_thrown(res, "Ljava/lang/IndexOutOfBoundsException;");
    }

    // Valid `append(null, 1, 3)` appends `"ul"` per Appendable contract.
    vm.invoke_instance(
        sw,
        "append",
        "(Ljava/lang/CharSequence;II)Ljava/io/StringWriter;",
        &[Value::Null, Value::Int(1), Value::Int(3)],
    )
    .expect("append null subseq");

    // `write(String, off, len)` with `off + len` overflowing usize must throw IOOBE.
    let res = vm.invoke_instance(
        sw,
        "write",
        "(Ljava/lang/String;II)V",
        &[hello, Value::Int(2), Value::Int(i32::MAX)],
    );
    assert_thrown(res, "Ljava/lang/IndexOutOfBoundsException;");

    // `ByteArrayOutputStream.write([B, off, len)` and `BufferedOutputStream.write([B, off, len)`
    let baos = alloc_obj(&mut vm, BAOS);
    vm.invoke_instance(baos, "<init>", "()V", &[])
        .expect("BAOS.<init>");
    let bytes = alloc_bytes(&mut vm, b"abcd");
    let res = vm.invoke_instance(
        baos,
        "write",
        "([BII)V",
        &[Value::Obj(bytes), Value::Int(2), Value::Int(i32::MAX)],
    );
    assert_thrown(res, "Ljava/lang/IndexOutOfBoundsException;");

    let bos = alloc_obj(&mut vm, BOS);
    vm.invoke_instance(
        bos,
        "<init>",
        "(Ljava/io/OutputStream;)V",
        &[Value::Obj(baos)],
    )
    .expect("BOS.<init>");
    let res = vm.invoke_instance(
        bos,
        "write",
        "([BII)V",
        &[Value::Obj(bytes), Value::Int(2), Value::Int(i32::MAX)],
    );
    assert_thrown(res, "Ljava/lang/IndexOutOfBoundsException;");

    let out = vm
        .invoke_instance(sw, "toString", "()Ljava/lang/String;", &[])
        .expect("SW.toString");
    let Value::Obj(out_ref) = out else {
        panic!("expected String");
    };
    assert_eq!(vm.heap.as_str(out_ref).expect("str"), "ul");
}

#[test]
fn utf16_surrogate_pairs_preserved_across_writers_and_print_streams() {
    // Build a tiny DEX subclass of java.io.Writer that records chars passed to write([CII)V
    // into a StringWriter delegate so we also verify dex_base `(WRITER, "write", "(I)V")`
    // and `(WRITER, "append", "(C)Ljava/io/Writer;")`.
    let mut b = DexBuilder::new();
    let writer_init_p = b.proto_idx("V", vec![]);
    let writer_init_m = b.method_idx("Ljava/io/Writer;", writer_init_p, "<init>");
    let sw_write_cii_p = b.proto_idx("V", vec!["[C".into(), "I".into(), "I".into()]);
    let sw_write_cii_m = b.method_idx(SW, sw_write_cii_p, "write");

    let cls = b.class("Ltest/ProxyWriter;", "Ljava/io/Writer;");
    b.instance_field(cls, "sink", SW);
    let f_sink = b.field_idx("Ltest/ProxyWriter;", SW, "sink");

    let mut init_u = Vec::new();
    init_u.extend(op35c(0x70, 1, writer_init_m, [0, 0, 0, 0, 0])); // invoke-direct Writer.<init>(p0)
    init_u.extend(op22c(0x5b, 1, 0, f_sink)); // iput-object p1, p0, sink
    init_u.extend(op10x(0x0e)); // return-void
    let init_code = b.code(2, 2, 1, init_u);
    b.direct(cls, "<init>", "V", vec![SW], 0x10001, Some(init_code));

    // write([C p1, I p2, I p3) -> sink.write(p1, p2, p3)
    let mut w_u = Vec::new();
    w_u.extend(op22c(0x54, 0, 1, f_sink)); // iget-object v0, p0(v1), sink
    w_u.extend(op35c(0x6e, 4, sw_write_cii_m, [0, 2, 3, 4, 0])); // invoke-virtual v0.write(p1, p2, p3)
    w_u.extend(op10x(0x0e));
    let w_code = b.code(5, 4, 4, w_u);
    b.r#virtual(
        cls,
        "write",
        "V",
        vec!["[C", "I", "I"],
        0x0001,
        Some(w_code),
    );

    let nop1 = b.code(1, 1, 0, op10x(0x0e));
    let nop2 = b.code(1, 1, 0, op10x(0x0e));
    b.r#virtual(cls, "flush", "V", vec![], 0x0001, Some(nop1));
    b.r#virtual(cls, "close", "V", vec![], 0x0001, Some(nop2));

    let mut vm = engine_of(&b);

    // 1. StringWriter: write(int) + append(char) with split surrogate pairs ("😀" = D83D DE00).
    let sw = alloc_obj(&mut vm, SW);
    vm.invoke_instance(sw, "<init>", "()V", &[])
        .expect("SW.<init>");
    vm.invoke_instance(sw, "write", "(I)V", &[Value::Int(0xD83D)])
        .expect("hi");
    vm.invoke_instance(sw, "write", "(I)V", &[Value::Int(0xDE00)])
        .expect("lo");
    vm.invoke_instance(
        sw,
        "append",
        "(C)Ljava/io/StringWriter;",
        &[Value::Int(0xD83D)],
    )
    .expect("append hi");
    vm.invoke_instance(
        sw,
        "append",
        "(C)Ljava/io/StringWriter;",
        &[Value::Int(0xDE80)],
    )
    .expect("append lo");
    let Value::Obj(s_ref) = vm
        .invoke_instance(sw, "toString", "()Ljava/lang/String;", &[])
        .expect("toString")
    else {
        panic!("expected String");
    };
    assert_eq!(vm.heap.as_str(s_ref).expect("str"), "😀🚀");

    // 2. OutputStreamWriter over ByteArrayOutputStream: exact 4-byte UTF-8 F0 9F 98 80.
    let baos = alloc_obj(&mut vm, BAOS);
    vm.invoke_instance(baos, "<init>", "()V", &[])
        .expect("BAOS.<init>");
    let osw = alloc_obj(&mut vm, OSW);
    vm.invoke_instance(
        osw,
        "<init>",
        "(Ljava/io/OutputStream;)V",
        &[Value::Obj(baos)],
    )
    .expect("OSW.<init>");
    vm.invoke_instance(osw, "write", "(I)V", &[Value::Int(0xD83D)])
        .expect("osw hi");
    vm.invoke_instance(osw, "write", "(I)V", &[Value::Int(0xDE00)])
        .expect("osw lo");
    vm.invoke_instance(osw, "flush", "()V", &[])
        .expect("osw flush");
    let Value::Obj(raw_arr) = vm
        .invoke_instance(baos, "toByteArray", "()[B", &[])
        .expect("toByteArray")
    else {
        panic!("expected [B");
    };
    assert_eq!(read_bytes(&vm, raw_arr), "😀".as_bytes());

    // 3. PrintStream.print(char) auto-flushes after writes, yet holds a trailing
    //    high surrogate until its low surrogate arrives!
    let baos_ps = alloc_obj(&mut vm, BAOS);
    vm.invoke_instance(baos_ps, "<init>", "()V", &[])
        .expect("BAOS.<init>");
    let ps = alloc_obj(&mut vm, PS);
    vm.invoke_instance(
        ps,
        "<init>",
        "(Ljava/io/OutputStream;)V",
        &[Value::Obj(baos_ps)],
    )
    .expect("PS.<init>");
    vm.invoke_instance(ps, "print", "(C)V", &[Value::Int(0xD83D)])
        .expect("ps hi");
    vm.invoke_instance(ps, "print", "(C)V", &[Value::Int(0xDE00)])
        .expect("ps lo");
    vm.invoke_instance(ps, "flush", "()V", &[])
        .expect("ps flush");
    let Value::Obj(ps_arr) = vm
        .invoke_instance(baos_ps, "toByteArray", "()[B", &[])
        .expect("toByteArray")
    else {
        panic!("expected [B");
    };
    assert_eq!(read_bytes(&vm, ps_arr), "😀".as_bytes());

    // 4. PrintWriter.print(char) + println(char) over StringWriter.
    let sw_pw = alloc_obj(&mut vm, SW);
    vm.invoke_instance(sw_pw, "<init>", "()V", &[])
        .expect("SW.<init>");
    let pw = alloc_obj(&mut vm, PW);
    vm.invoke_instance(pw, "<init>", "(Ljava/io/Writer;)V", &[Value::Obj(sw_pw)])
        .expect("PW.<init>");
    vm.invoke_instance(pw, "print", "(C)V", &[Value::Int(0xD83D)])
        .expect("pw hi");
    vm.invoke_instance(pw, "println", "(C)V", &[Value::Int(0xDE00)])
        .expect("pw lo");
    vm.invoke_instance(pw, "flush", "()V", &[])
        .expect("pw flush");
    let Value::Obj(pw_str) = vm
        .invoke_instance(sw_pw, "toString", "()Ljava/lang/String;", &[])
        .expect("SW.toString")
    else {
        panic!("expected String");
    };
    assert_eq!(vm.heap.as_str(pw_str).expect("str"), "😀\n");

    // 5. DEX subclass of Writer calling inherited Writer.write(int) and Writer.append(char).
    let sw_dex = alloc_obj(&mut vm, SW);
    vm.invoke_instance(sw_dex, "<init>", "()V", &[])
        .expect("SW.<init>");
    let proxy = alloc_obj(&mut vm, "Ltest/ProxyWriter;");
    vm.invoke_instance(
        proxy,
        "<init>",
        "(Ljava/io/StringWriter;)V",
        &[Value::Obj(sw_dex)],
    )
    .expect("ProxyWriter.<init>");
    vm.invoke_instance(proxy, "write", "(I)V", &[Value::Int(0xD83D)])
        .expect("proxy write hi");
    vm.invoke_instance(proxy, "write", "(I)V", &[Value::Int(0xDE00)])
        .expect("proxy write lo");
    vm.invoke_instance(
        proxy,
        "append",
        "(C)Ljava/io/Writer;",
        &[Value::Int(0xD83D)],
    )
    .expect("proxy append hi");
    vm.invoke_instance(
        proxy,
        "append",
        "(C)Ljava/io/Writer;",
        &[Value::Int(0xDE80)],
    )
    .expect("proxy append lo");
    let Value::Obj(dex_str) = vm
        .invoke_instance(sw_dex, "toString", "()Ljava/lang/String;", &[])
        .expect("SW.toString")
    else {
        panic!("expected String");
    };
    assert_eq!(vm.heap.as_str(dex_str).expect("str"), "😀🚀");
}

#[test]
fn load_clamps_oversized_return_counts_from_dex_reader_and_input_stream() {
    let mut b = DexBuilder::new();
    let reader_init_p = b.proto_idx("V", vec![]);
    let reader_init_m = b.method_idx("Ljava/io/Reader;", reader_init_p, "<init>");
    let is_init_p = b.proto_idx("V", vec![]);
    let is_init_m = b.method_idx("Ljava/io/InputStream;", is_init_p, "<init>");

    // Ltest/BogusReader; extends java.io.Reader:
    // First read([C, 0, 4096) writes 'O' into cbuf[0] and returns 30_000 (> 4096!);
    // Second read returns -1.
    let r_cls = b.class("Ltest/BogusReader;", "Ljava/io/Reader;");
    b.instance_field(r_cls, "done", "I");
    let f_rdone = b.field_idx("Ltest/BogusReader;", "I", "done");

    let mut r_init = Vec::new();
    r_init.extend(op35c(0x70, 1, reader_init_m, [0, 0, 0, 0, 0]));
    r_init.extend(op10x(0x0e));
    let r_init_code = b.code(1, 1, 1, r_init);
    b.direct(r_cls, "<init>", "V", vec![], 0x10001, Some(r_init_code));

    // regs: v0..v2 scratch, v3=this, v4=cbuf, v5=off, v6=len
    let mut r_read = Vec::new();
    r_read.extend(op22c(0x52, 0, 3, f_rdone)); // pc 0..2: iget v0, p0, done
    r_read.extend(op21t_or_22t_nez(0, 13)); // pc 2..4: if-nez v0, +13 -> pc 15
    r_read.extend(op11n(0x12, 1, 1)); // pc 4..5: const/4 v1, 1
    r_read.extend(op22c(0x59, 1, 3, f_rdone)); // pc 5..7: iput v1, p0, done
    r_read.extend(op21s(0x13, 1, b'O' as i16)); // pc 7..9: const/16 v1, 'O'
    r_read.extend(op11n(0x12, 2, 0)); // pc 9..10: const/4 v2, 0
    r_read.extend(op23x(0x50, 1, 4, 2)); // pc 10..12: aput-char v1, p1(v4), v2
    r_read.extend(op21s(0x13, 0, 30_000)); // pc 12..14: const/16 v0, 30000 (> 4096!)
    r_read.extend(op11x(0x0f, 0)); // pc 14..15: return v0
    r_read.extend(op11n(0x12, 0, -1)); // pc 15..16: const/4 v0, -1
    r_read.extend(op11x(0x0f, 0)); // pc 16..17: return v0
    let r_read_code = b.code(7, 4, 0, r_read);
    b.r#virtual(
        r_cls,
        "read",
        "I",
        vec!["[C", "I", "I"],
        0x0001,
        Some(r_read_code),
    );
    let r_nop = b.code(1, 1, 0, op10x(0x0e));
    b.r#virtual(r_cls, "close", "V", vec![], 0x0001, Some(r_nop));

    // Ltest/BogusInputStream; extends java.io.InputStream:
    // First read([B, 0, 8192) writes b'K' into b[0] and returns 30_000 (> 8192!);
    // Second read returns -1.
    let i_cls = b.class("Ltest/BogusInputStream;", "Ljava/io/InputStream;");
    b.instance_field(i_cls, "done", "I");
    let f_idone = b.field_idx("Ltest/BogusInputStream;", "I", "done");

    let mut i_init = Vec::new();
    i_init.extend(op35c(0x70, 1, is_init_m, [0, 0, 0, 0, 0]));
    i_init.extend(op10x(0x0e));
    let i_init_code = b.code(1, 1, 1, i_init);
    b.direct(i_cls, "<init>", "V", vec![], 0x10001, Some(i_init_code));

    let mut i_read = Vec::new();
    i_read.extend(op22c(0x52, 0, 3, f_idone)); // pc 0..2: iget v0, p0, done
    i_read.extend(op21t_or_22t_nez(0, 13)); // pc 2..4: if-nez v0, +13 -> pc 15
    i_read.extend(op11n(0x12, 1, 1)); // pc 4..5: const/4 v1, 1
    i_read.extend(op22c(0x59, 1, 3, f_idone)); // pc 5..7: iput v1, p0, done
    i_read.extend(op21s(0x13, 1, b'K' as i16)); // pc 7..9: const/16 v1, 'K'
    i_read.extend(op11n(0x12, 2, 0)); // pc 9..10: const/4 v2, 0
    i_read.extend(op23x(0x4f, 1, 4, 2)); // pc 10..12: aput-byte v1, p1(v4), v2
    i_read.extend(op21s(0x13, 0, 30_000)); // pc 12..14: const/16 v0, 30000 (> 8192!)
    i_read.extend(op11x(0x0f, 0)); // pc 14..15: return v0
    i_read.extend(op11n(0x12, 0, -1)); // pc 15..16: const/4 v0, -1
    i_read.extend(op11x(0x0f, 0)); // pc 16..17: return v0
    let i_read_code = b.code(7, 4, 0, i_read);
    b.r#virtual(
        i_cls,
        "read",
        "I",
        vec!["[B", "I", "I"],
        0x0001,
        Some(i_read_code),
    );
    let i_nop = b.code(1, 1, 0, op10x(0x0e));
    b.r#virtual(i_cls, "close", "V", vec![], 0x0001, Some(i_nop));

    let mut vm = engine_of(&b);

    let bogus_r = alloc_obj(&mut vm, "Ltest/BogusReader;");
    vm.invoke_instance(bogus_r, "<init>", "()V", &[])
        .expect("BogusReader.<init>");
    let br = alloc_obj(&mut vm, BR);
    vm.invoke_instance(br, "<init>", "(Ljava/io/Reader;)V", &[Value::Obj(bogus_r)])
        .expect("BufferedReader.<init>");
    // Must clamp 30_000 -> 4096 instead of panicking `&got[..30000]`.
    let ch = vm
        .invoke_instance(br, "read", "()I", &[])
        .expect("BufferedReader.read");
    assert_eq!(ch, Value::Int(b'O' as i32));

    let bogus_is = alloc_obj(&mut vm, "Ltest/BogusInputStream;");
    vm.invoke_instance(bogus_is, "<init>", "()V", &[])
        .expect("BogusInputStream.<init>");
    let isr = alloc_obj(&mut vm, ISR);
    vm.invoke_instance(
        isr,
        "<init>",
        "(Ljava/io/InputStream;)V",
        &[Value::Obj(bogus_is)],
    )
    .expect("ISR.<init>");
    // `getEncoding()` before read must NOT drain BogusInputStream prematurely.
    let Value::Obj(enc) = vm
        .invoke_instance(isr, "getEncoding", "()Ljava/lang/String;", &[])
        .expect("getEncoding")
    else {
        panic!("expected encoding String");
    };
    assert_eq!(vm.heap.as_str(enc).expect("str"), "UTF8");
    // Now read() drains and clamps 30_000 -> 8192 without panicking.
    let ch2 = vm
        .invoke_instance(isr, "read", "()I", &[])
        .expect("ISR.read");
    assert_eq!(ch2, Value::Int(b'K' as i32));
}

fn op21t_or_22t_nez(reg: u8, off: i16) -> Vec<u16> {
    vec![0x39u16 | ((reg as u16) << 8), off as u16]
}

#[test]
fn runaway_infinite_reader_hits_io_exception_ceiling_without_heap_leak() {
    let mut b = DexBuilder::new();
    let reader_init_p = b.proto_idx("V", vec![]);
    let reader_init_m = b.method_idx("Ljava/io/Reader;", reader_init_p, "<init>");

    let cls = b.class("Ltest/InfiniteReader;", "Ljava/io/Reader;");
    let mut init_u = Vec::new();
    init_u.extend(op35c(0x70, 1, reader_init_m, [0, 0, 0, 0, 0]));
    init_u.extend(op10x(0x0e));
    let init_code = b.code(1, 1, 1, init_u);
    b.direct(cls, "<init>", "V", vec![], 0x10001, Some(init_code));

    // Always returns 4096 without ever signalling EOF (-1).
    let mut read_u = Vec::new();
    read_u.extend(op21s(0x13, 0, 4096));
    read_u.extend(op11x(0x0f, 0));
    let read_code = b.code(5, 4, 0, read_u);
    b.r#virtual(
        cls,
        "read",
        "I",
        vec!["[C", "I", "I"],
        0x0001,
        Some(read_code),
    );
    let nop = b.code(1, 1, 0, op10x(0x0e));
    b.r#virtual(cls, "close", "V", vec![], 0x0001, Some(nop));

    let mut vm = engine_of(&b);
    let inf = alloc_obj(&mut vm, "Ltest/InfiniteReader;");
    vm.invoke_instance(inf, "<init>", "()V", &[])
        .expect("InfiniteReader.<init>");
    let br = alloc_obj(&mut vm, BR);
    vm.invoke_instance(br, "<init>", "(Ljava/io/Reader;)V", &[Value::Obj(inf)])
        .expect("BufferedReader.<init>");

    let res = vm.invoke_instance(br, "read", "()I", &[]);
    assert_thrown(res, "Ljava/io/IOException;");
}

#[test]
fn closed_reader_and_null_pointer_contracts_enforced() {
    let mut vm = engine_of(&DexBuilder::new());

    // 1. NullPointerException on null constructor / write / read arguments.
    let sr = alloc_obj(&mut vm, SR);
    let res = vm.invoke_instance(sr, "<init>", "(Ljava/lang/String;)V", &[Value::Null]);
    assert_thrown(res, "Ljava/lang/NullPointerException;");

    let fr = alloc_obj(&mut vm, FR);
    let res = vm.invoke_instance(fr, "<init>", "(Ljava/lang/String;)V", &[Value::Null]);
    assert_thrown(res, "Ljava/lang/NullPointerException;");

    let fw = alloc_obj(&mut vm, FW);
    let res = vm.invoke_instance(fw, "<init>", "(Ljava/lang/String;)V", &[Value::Null]);
    assert_thrown(res, "Ljava/lang/NullPointerException;");

    let bais = alloc_obj(&mut vm, BAIS);
    let res = vm.invoke_instance(bais, "<init>", "([B)V", &[Value::Null]);
    assert_thrown(res, "Ljava/lang/NullPointerException;");
    let res = vm.invoke_instance(
        bais,
        "<init>",
        "([BII)V",
        &[Value::Null, Value::Int(0), Value::Int(0)],
    );
    assert_thrown(res, "Ljava/lang/NullPointerException;");

    let baos = alloc_obj(&mut vm, BAOS);
    vm.invoke_instance(baos, "<init>", "()V", &[])
        .expect("BAOS.<init>");
    for (name, sig, args) in [
        ("write", "([B)V", vec![Value::Null]),
        (
            "write",
            "([BII)V",
            vec![Value::Null, Value::Int(0), Value::Int(0)],
        ),
        ("writeBytes", "([B)V", vec![Value::Null]),
    ] {
        let res = vm.invoke_instance(baos, name, sig, &args);
        assert_thrown(res, "Ljava/lang/NullPointerException;");
    }

    let bos = alloc_obj(&mut vm, BOS);
    vm.invoke_instance(
        bos,
        "<init>",
        "(Ljava/io/OutputStream;)V",
        &[Value::Obj(baos)],
    )
    .expect("BOS.<init>");
    for (sig, args) in [
        ("([B)V", vec![Value::Null]),
        ("([BII)V", vec![Value::Null, Value::Int(0), Value::Int(0)]),
    ] {
        let res = vm.invoke_instance(bos, "write", sig, &args);
        assert_thrown(res, "Ljava/lang/NullPointerException;");
    }

    let sw = alloc_obj(&mut vm, SW);
    vm.invoke_instance(sw, "<init>", "()V", &[])
        .expect("SW.<init>");
    for (sig, args) in [
        ("(Ljava/lang/String;)V", vec![Value::Null]),
        (
            "(Ljava/lang/String;II)V",
            vec![Value::Null, Value::Int(0), Value::Int(0)],
        ),
        ("([C)V", vec![Value::Null]),
        ("([CII)V", vec![Value::Null, Value::Int(0), Value::Int(0)]),
    ] {
        let res = vm.invoke_instance(sw, "write", sig, &args);
        assert_thrown(res, "Ljava/lang/NullPointerException;");
    }

    // StringWriter.close() is a no-op in Java: subsequent writes still succeed.
    vm.invoke_instance(sw, "close", "()V", &[])
        .expect("SW.close");
    let ok_s = alloc_jstr(&mut vm, "still-open");
    vm.invoke_instance(sw, "write", "(Ljava/lang/String;)V", &[ok_s])
        .expect("SW.write after close");

    // 2. Closed Reader semantics: all data ops throw IOException("Stream closed"),
    //    while markSupported() returns 1, getEncoding() returns null, and close() is idempotent.
    let sr_ok = alloc_obj(&mut vm, SR);
    let txt = alloc_jstr(&mut vm, "hello\nworld");
    vm.invoke_instance(sr_ok, "<init>", "(Ljava/lang/String;)V", &[txt])
        .expect("SR.<init>");
    assert_eq!(
        vm.invoke_instance(sr_ok, "read", "()I", &[]).expect("read"),
        Value::Int(b'h' as i32)
    );
    // Negative mark readAheadLimit throws IllegalArgumentException while open.
    let neg_mark = vm.invoke_instance(sr_ok, "mark", "(I)V", &[Value::Int(-1)]);
    assert_thrown(neg_mark, "Ljava/lang/IllegalArgumentException;");

    vm.invoke_instance(sr_ok, "close", "()V", &[])
        .expect("SR.close 1");
    vm.invoke_instance(sr_ok, "close", "()V", &[])
        .expect("SR.close 2 idempotent");
    assert_eq!(
        vm.invoke_instance(sr_ok, "markSupported", "()Z", &[])
            .expect("markSupported"),
        Value::Int(1)
    );

    let cbuf = alloc_chars(&mut vm, 4);
    for (name, sig, args) in [
        ("read", "()I", vec![]),
        ("read", "([C)I", vec![Value::Obj(cbuf)]),
        (
            "read",
            "([CII)I",
            vec![Value::Obj(cbuf), Value::Int(0), Value::Int(2)],
        ),
        ("ready", "()Z", vec![]),
        ("skip", "(J)J", vec![Value::Long(1)]),
        ("mark", "(I)V", vec![Value::Int(8)]),
        ("reset", "()V", vec![]),
    ] {
        let res = vm.invoke_instance(sr_ok, name, sig, &args);
        assert_thrown(res, "Ljava/io/IOException;");
    }

    // InputStreamReader.getEncoding() returns null after close().
    let raw_in = alloc_bytes(&mut vm, b"hi");
    let bais_ok = alloc_obj(&mut vm, BAIS);
    vm.invoke_instance(bais_ok, "<init>", "([B)V", &[Value::Obj(raw_in)])
        .expect("BAIS.<init>");
    let isr = alloc_obj(&mut vm, ISR);
    vm.invoke_instance(
        isr,
        "<init>",
        "(Ljava/io/InputStream;)V",
        &[Value::Obj(bais_ok)],
    )
    .expect("ISR.<init>");
    vm.invoke_instance(isr, "close", "()V", &[])
        .expect("ISR.close");
    assert_eq!(
        vm.invoke_instance(isr, "getEncoding", "()Ljava/lang/String;", &[])
            .expect("getEncoding after close"),
        Value::Null
    );
    let closed_read = vm.invoke_instance(isr, "read", "()I", &[]);
    assert_thrown(closed_read, "Ljava/io/IOException;");
}
