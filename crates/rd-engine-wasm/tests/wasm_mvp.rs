//! Integration tests for the S1 WebAssembly subset (ADR 013).
//! Modules are assembled by hand in `support/`, so each test is self-contained.

mod support;

use rd_engine_wasm::{limits, ExportKind, Instance, Module, Value};
use support::*;

fn fib_module() -> Vec<u8> {
    // fib(n) = n < 2 ? n : fib(n-1) + fib(n-2)
    let mut b = ModuleBuilder::default();
    b.types.push(functype(&[I32], &[I32]));
    let mut body = Vec::new();
    body.extend(local_get(0));
    body.extend(i32_const(2));
    body.push(0x48); // i32.lt_s
    body.extend([0x04, I32]); // if (result i32)
    body.extend(local_get(0));
    body.push(0x05); // else
    body.extend(local_get(0));
    body.extend(i32_const(1));
    body.push(0x6b); // i32.sub
    body.extend(call(0));
    body.extend(local_get(0));
    body.extend(i32_const(2));
    body.push(0x6b);
    body.extend(call(0));
    body.push(0x6a); // i32.add
    body.push(0x0b); // end if
    b.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body,
    });
    b.exports.push(("fib".into(), 0, 0));
    b.build()
}

fn run(
    bytes: &[u8],
    name: &str,
    args: &[Value],
    fuel: u64,
) -> Result<Vec<Value>, rd_engine_wasm::WasmError> {
    let m = Module::parse(bytes)?;
    let mut inst = Instance::new(m, limits::MAX_FUEL)?;
    Ok(inst.invoke(name, args, fuel)?.results)
}

#[test]
fn add_returns_result_and_counts_instructions_as_fuel() {
    let mut b = ModuleBuilder::default();
    b.types.push(functype(&[I32, I32], &[I32]));
    let mut body = local_get(0);
    body.extend(local_get(1));
    body.push(0x6a);
    b.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body,
    });
    b.exports.push(("add".into(), 0, 0));
    let m = Module::parse(&b.build()).expect("valid module");
    assert_eq!(m.exports(), vec![("add", ExportKind::Func)]);
    let mut inst = Instance::new(m, 1000).unwrap();
    let out = inst
        .invoke("add", &[Value::I32(2), Value::I32(40)], 1000)
        .unwrap();
    assert_eq!(out.results, vec![Value::I32(42)]);
    // local.get, local.get, i32.add, return(end): four executed instructions.
    assert_eq!(out.fuel_used, 4);
}

#[test]
fn i32_arithmetic_wraps_and_i64_multiplies() {
    let mut b = ModuleBuilder::default();
    b.types.push(functype(&[I32, I32], &[I32]));
    b.types.push(functype(&[I64, I64], &[I64]));
    let mut mul32 = local_get(0);
    mul32.extend(local_get(1));
    mul32.push(0x6c);
    b.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: mul32,
    });
    let mut mul64 = local_get(0);
    mul64.extend(local_get(1));
    mul64.push(0x7e);
    b.funcs.push(FuncDef {
        type_idx: 1,
        locals: vec![],
        body: mul64,
    });
    b.exports.push(("mul32".into(), 0, 0));
    b.exports.push(("mul64".into(), 0, 1));
    let bytes = b.build();
    assert_eq!(
        run(&bytes, "mul32", &[Value::I32(i32::MAX), Value::I32(2)], 100).unwrap(),
        vec![Value::I32(-2)]
    );
    assert_eq!(
        run(
            &bytes,
            "mul64",
            &[Value::I64(1 << 40), Value::I64(1 << 30)],
            100
        )
        .unwrap(),
        vec![Value::I64(0)]
    );
}

#[test]
fn recursive_fibonacci_is_correct() {
    let bytes = fib_module();
    assert_eq!(
        run(&bytes, "fib", &[Value::I32(0)], 100_000).unwrap(),
        vec![Value::I32(0)]
    );
    assert_eq!(
        run(&bytes, "fib", &[Value::I32(1)], 100_000).unwrap(),
        vec![Value::I32(1)]
    );
    assert_eq!(
        run(&bytes, "fib", &[Value::I32(20)], 1_000_000).unwrap(),
        vec![Value::I32(6765)]
    );
}

#[test]
fn loop_with_br_if_sums_to_expected_value() {
    // sum(n) = n + (n-1) + ... + 1, using block/loop/br_if.
    let mut b = ModuleBuilder::default();
    b.types.push(functype(&[I32], &[I32]));
    let mut body = Vec::new();
    body.extend([0x02, 0x40]); // block
    body.extend([0x03, 0x40]); // loop
    body.extend(local_get(0));
    body.push(0x45); // i32.eqz
    body.extend(br_if(1));
    body.extend(local_get(1));
    body.extend(local_get(0));
    body.push(0x6a);
    body.extend(local_set(1));
    body.extend(local_get(0));
    body.extend(i32_const(1));
    body.push(0x6b);
    body.extend(local_set(0));
    body.extend(br(0));
    body.push(0x0b); // end loop
    body.push(0x0b); // end block
    body.extend(local_get(1));
    b.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![(1, I32)],
        body,
    });
    b.exports.push(("sum".into(), 0, 0));
    let bytes = b.build();
    assert_eq!(
        run(&bytes, "sum", &[Value::I32(100)], 100_000).unwrap(),
        vec![Value::I32(5050)]
    );
    assert_eq!(
        run(&bytes, "sum", &[Value::I32(0)], 100).unwrap(),
        vec![Value::I32(0)]
    );
}

#[test]
fn if_else_and_block_result_carry_values() {
    // f(x) = if x < 0 { -x } else { block(result i32) { 7 + x; br 0 } }
    let mut b = ModuleBuilder::default();
    b.types.push(functype(&[I32], &[I32]));
    let mut body = Vec::new();
    body.extend(local_get(0));
    body.extend(i32_const(0));
    body.push(0x48);
    body.extend([0x04, I32]);
    body.extend(i32_const(0));
    body.extend(local_get(0));
    body.push(0x6b);
    body.push(0x05);
    body.extend([0x02, I32]);
    body.extend(i32_const(7));
    body.extend(local_get(0));
    body.push(0x6a);
    body.extend(br(0));
    body.push(0x0b); // end block
    body.push(0x0b); // end if (the builder appends the function's end)
    b.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body,
    });
    b.exports.push(("f".into(), 0, 0));
    let bytes = b.build();
    assert_eq!(
        run(&bytes, "f", &[Value::I32(-5)], 100).unwrap(),
        vec![Value::I32(5)]
    );
    assert_eq!(
        run(&bytes, "f", &[Value::I32(3)], 100).unwrap(),
        vec![Value::I32(10)]
    );
}

#[test]
fn br_table_selects_the_right_label_and_default() {
    // 0 -> 10, 1 -> 20, anything else -> 30
    let mut b = ModuleBuilder::default();
    b.types.push(functype(&[I32], &[I32]));
    let mut body = Vec::new();
    body.extend([0x02, 0x40]); // block A (outer, depth 2 from inside)
    body.extend([0x02, 0x40]); // block B (depth 1)
    body.extend([0x02, 0x40]); // block C (depth 0)
    body.extend(local_get(0));
    body.extend([0x0e, 0x02, 0x00, 0x01, 0x02]); // br_table [0,1] default 2
    body.push(0x0b); // end C
    body.extend(i32_const(10));
    body.push(0x0f); // return
    body.push(0x0b); // end B
    body.extend(i32_const(20));
    body.push(0x0f);
    body.push(0x0b); // end A
    body.extend(i32_const(30));
    b.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body,
    });
    b.exports.push(("sw".into(), 0, 0));
    let bytes = b.build();
    assert_eq!(
        run(&bytes, "sw", &[Value::I32(0)], 100).unwrap(),
        vec![Value::I32(10)]
    );
    assert_eq!(
        run(&bytes, "sw", &[Value::I32(1)], 100).unwrap(),
        vec![Value::I32(20)]
    );
    assert_eq!(
        run(&bytes, "sw", &[Value::I32(5)], 100).unwrap(),
        vec![Value::I32(30)]
    );
    assert_eq!(
        run(&bytes, "sw", &[Value::I32(-1)], 100).unwrap(),
        vec![Value::I32(30)]
    );
}

#[test]
fn memory_round_trip_and_sign_extension() {
    let mut b = ModuleBuilder::default();
    b.types.push(functype(&[I32, I32], &[I32]));
    b.types.push(functype(&[I32], &[I32]));
    b.memory = Some((1, None));
    let mut store = local_get(0);
    store.extend(local_get(1));
    store.extend(mem(0x36, 2, 0)); // i32.store
    store.extend(local_get(0));
    store.extend(mem(0x28, 2, 0)); // i32.load
    b.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: store,
    });
    let mut load8s = local_get(0);
    load8s.extend(mem(0x2c, 0, 0)); // i32.load8_s
    b.funcs.push(FuncDef {
        type_idx: 1,
        locals: vec![],
        body: load8s,
    });
    let mut load8u = local_get(0);
    load8u.extend(mem(0x2d, 0, 0)); // i32.load8_u
    b.funcs.push(FuncDef {
        type_idx: 1,
        locals: vec![],
        body: load8u,
    });
    b.exports.push(("store_load".into(), 0, 0));
    b.exports.push(("load8s".into(), 0, 1));
    b.exports.push(("load8u".into(), 0, 2));
    b.data.push((16, vec![1, 2, 3, 0xff]));
    let bytes = b.build();
    let m = Module::parse(&bytes).unwrap();
    let mut inst = Instance::new(m, 1000).unwrap();
    assert_eq!(&inst.memory()[16..20], &[1, 2, 3, 0xff]);
    let out = inst
        .invoke("store_load", &[Value::I32(100), Value::I32(-12345)], 100)
        .unwrap();
    assert_eq!(out.results, vec![Value::I32(-12345)]);
    assert_eq!(
        inst.invoke("load8s", &[Value::I32(19)], 100)
            .unwrap()
            .results,
        vec![Value::I32(-1)]
    );
    assert_eq!(
        inst.invoke("load8u", &[Value::I32(19)], 100)
            .unwrap()
            .results,
        vec![Value::I32(255)]
    );
}

#[test]
fn memory_grow_is_bounded_by_declared_maximum() {
    let mut b = ModuleBuilder::default();
    b.types.push(functype(&[I32], &[I32]));
    b.types.push(functype(&[], &[I32]));
    b.memory = Some((1, Some(2)));
    let mut grow = local_get(0);
    grow.extend([0x40, 0x00]);
    b.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: grow,
    });
    b.funcs.push(FuncDef {
        type_idx: 1,
        locals: vec![],
        body: vec![0x3f, 0x00],
    });
    b.exports.push(("grow".into(), 0, 0));
    b.exports.push(("size".into(), 0, 1));
    let m = Module::parse(&b.build()).unwrap();
    let mut inst = Instance::new(m, 1000).unwrap();
    assert_eq!(
        inst.invoke("grow", &[Value::I32(1)], 100).unwrap().results,
        vec![Value::I32(1)]
    );
    assert_eq!(
        inst.invoke("grow", &[Value::I32(1)], 100).unwrap().results,
        vec![Value::I32(-1)]
    );
    assert_eq!(
        inst.invoke("size", &[], 100).unwrap().results,
        vec![Value::I32(2)]
    );
    assert_eq!(inst.memory().len(), 2 * limits::PAGE_SIZE);
}

#[test]
fn globals_persist_across_calls_and_start_runs_at_instantiation() {
    let mut b = ModuleBuilder::default();
    b.types.push(functype(&[], &[I32]));
    b.types.push(functype(&[], &[]));
    b.globals.push(Global {
        ty: I32,
        mutable: true,
        init: i32_const(5),
    });
    let mut bump = global_get(0);
    bump.extend(i32_const(1));
    bump.push(0x6a);
    bump.extend(global_set(0));
    bump.extend(global_get(0));
    b.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: bump,
    });
    // start: global = 99
    let mut init = i32_const(99);
    init.extend(global_set(0));
    b.funcs.push(FuncDef {
        type_idx: 1,
        locals: vec![],
        body: init,
    });
    b.exports.push(("bump".into(), 0, 0));
    b.start = Some(1);
    let m = Module::parse(&b.build()).unwrap();
    let mut inst = Instance::new(m, 1000).unwrap();
    assert_eq!(inst.globals(), &[Value::I32(99)]);
    assert_eq!(
        inst.invoke("bump", &[], 100).unwrap().results,
        vec![Value::I32(100)]
    );
    assert_eq!(
        inst.invoke("bump", &[], 100).unwrap().results,
        vec![Value::I32(101)]
    );
}

#[test]
fn execution_is_deterministic() {
    let bytes = fib_module();
    let m = Module::parse(&bytes).unwrap();
    let mut a = Instance::new(m.clone(), 1000).unwrap();
    let mut b = Instance::new(m, 1000).unwrap();
    let x = a.invoke("fib", &[Value::I32(15)], 100_000).unwrap();
    let y = b.invoke("fib", &[Value::I32(15)], 100_000).unwrap();
    assert_eq!(x, y);
    assert!(x.fuel_used > 0);
}

#[test]
fn runtime_traps_are_structured_errors() {
    let mut b = ModuleBuilder::default();
    b.types.push(functype(&[I32, I32], &[I32]));
    b.types.push(functype(&[], &[I32]));
    b.types.push(functype(&[I32], &[I32]));
    b.memory = Some((1, None));
    let mut div = local_get(0);
    div.extend(local_get(1));
    div.push(0x6d);
    b.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: div,
    });
    b.funcs.push(FuncDef {
        type_idx: 1,
        locals: vec![],
        body: vec![0x00, 0x41, 0x00],
    }); // unreachable
    let mut oob = local_get(0);
    oob.extend(mem(0x28, 2, 0));
    b.funcs.push(FuncDef {
        type_idx: 2,
        locals: vec![],
        body: oob,
    });
    b.exports.push(("div".into(), 0, 0));
    b.exports.push(("unreach".into(), 0, 1));
    b.exports.push(("load".into(), 0, 2));
    let m = Module::parse(&b.build()).unwrap();
    let mut inst = Instance::new(m, 1000).unwrap();
    let e = inst
        .invoke("div", &[Value::I32(1), Value::I32(0)], 100)
        .unwrap_err();
    assert_eq!((e.code, e.module_id), ("TRAP", "rd-engine-wasm"));
    assert!(e.cause.contains("divide by zero"));
    let e = inst
        .invoke("div", &[Value::I32(i32::MIN), Value::I32(-1)], 100)
        .unwrap_err();
    assert!(e.cause.contains("integer overflow"));
    assert_eq!(
        inst.invoke("div", &[Value::I32(-7), Value::I32(2)], 100)
            .unwrap()
            .results,
        vec![Value::I32(-3)]
    );
    assert_eq!(inst.invoke("unreach", &[], 100).unwrap_err().code, "TRAP");
    let e = inst.invoke("load", &[Value::I32(65533)], 100).unwrap_err();
    assert!(e.cause.contains("out of bounds"));
    assert!(inst.invoke("load", &[Value::I32(65532)], 100).is_ok());
}

#[test]
fn fuel_bounds_infinite_loops_and_is_accounted() {
    let mut b = ModuleBuilder::default();
    b.types.push(functype(&[], &[]));
    b.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: vec![0x03, 0x40, 0x0c, 0x00, 0x0b], // loop; br 0; end
    });
    b.exports.push(("spin".into(), 0, 0));
    let m = Module::parse(&b.build()).unwrap();
    let mut inst = Instance::new(m, 0).unwrap();
    let e = inst.invoke("spin", &[], 5000).unwrap_err();
    assert_eq!(e.code, "FUEL_EXHAUSTED");
    let e = inst.invoke("spin", &[], limits::MAX_FUEL + 1).unwrap_err();
    assert_eq!(e.code, "LIMIT_EXCEEDED");
}

#[test]
fn call_depth_is_bounded() {
    let mut b = ModuleBuilder::default();
    b.types.push(functype(&[], &[]));
    b.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: call(0),
    });
    b.exports.push(("rec".into(), 0, 0));
    let m = Module::parse(&b.build()).unwrap();
    let mut inst = Instance::new(m, 0).unwrap();
    let e = inst.invoke("rec", &[], limits::MAX_FUEL).unwrap_err();
    assert_eq!(e.code, "STACK_EXHAUSTED");
}

#[test]
fn recursion_over_large_frames_hits_the_locals_budget() {
    // A recursive function declaring 50 000 locals: the second frame exceeds
    // the 64K-slot budget, so the call must fail instead of growing memory.
    let mut b = ModuleBuilder::default();
    b.types.push(functype(&[], &[]));
    b.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![(50_000, I32)],
        body: call(0),
    });
    b.exports.push(("rec".into(), 0, 0));
    let m = Module::parse(&b.build()).unwrap();
    let mut inst = Instance::new(m, 0).unwrap();
    let e = inst.invoke("rec", &[], limits::MAX_FUEL).unwrap_err();
    assert_eq!(e.code, "STACK_EXHAUSTED");
    assert!(e.cause.contains("locals"));
}

#[test]
fn invoke_checks_name_arity_types_and_argument_count() {
    let bytes = fib_module();
    let m = Module::parse(&bytes).unwrap();
    let mut inst = Instance::new(m, 0).unwrap();
    assert_eq!(inst.invoke("nope", &[], 10).unwrap_err().code, "NOT_FOUND");
    assert_eq!(inst.invoke("fib", &[], 10).unwrap_err().code, "INVALID_ARG");
    assert_eq!(
        inst.invoke("fib", &[Value::I64(1)], 10).unwrap_err().code,
        "INVALID_ARG"
    );
    let many = vec![Value::I32(0); limits::MAX_CALL_ARGS + 1];
    assert_eq!(
        inst.invoke("fib", &many, 10).unwrap_err().code,
        "LIMIT_EXCEEDED"
    );
}

#[test]
fn unreachable_code_is_validated_polymorphically() {
    // () -> i32 { unreachable; i32.add }: valid, traps at runtime.
    let mut b = ModuleBuilder::default();
    b.types.push(functype(&[], &[I32]));
    b.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: vec![0x00, 0x6a],
    });
    b.exports.push(("f".into(), 0, 0));
    let m = Module::parse(&b.build()).expect("polymorphic stack is valid");
    let mut inst = Instance::new(m, 0).unwrap();
    assert_eq!(inst.invoke("f", &[], 10).unwrap_err().code, "TRAP");
}

fn expect_err(bytes: &[u8], code: &str) {
    let e = Module::parse(bytes).expect_err("module must be rejected");
    assert_eq!(e.code, code, "{e}");
}

#[test]
fn malformed_headers_and_sections_are_rejected() {
    expect_err(b"\0asn\x01\0\0\0", "PARSE_ERROR");
    expect_err(b"\0asm\x02\0\0\0", "PARSE_ERROR");
    expect_err(b"\0asm", "PARSE_ERROR");
    expect_err(b"\0asm\x01\0\0\0\x0f\x00", "PARSE_ERROR"); // unknown section id 15
                                                           // sections out of order: export (7) before type (1)
    let mut b = b"\0asm\x01\0\0\0".to_vec();
    b.extend(section(7, vec_of(&[])));
    b.extend(section(1, vec_of(&[functype(&[], &[])])));
    expect_err(&b, "PARSE_ERROR");
    // section with declared length past the end of input
    let mut c = b"\0asm\x01\0\0\0".to_vec();
    c.extend([1, 50, 0]);
    expect_err(&c, "PARSE_ERROR");
}

#[test]
fn overlong_leb128_is_rejected() {
    let mut b = ModuleBuilder::default();
    b.types.push(functype(&[], &[I32]));
    // i32.const encoded with 6 LEB bytes; 5 is the maximum for i32
    b.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: vec![0x41, 0x80, 0x80, 0x80, 0x80, 0x80, 0x00],
    });
    expect_err(&b.build(), "PARSE_ERROR");
}

#[test]
fn floats_simd_imports_tables_and_multivalue_are_not_implemented() {
    let mut f32_param = ModuleBuilder::default();
    f32_param.types.push(vec![0x60, 1, 0x7d, 0]);
    expect_err(&f32_param.build(), "NOT_IMPLEMENTED");

    let mut float_const = ModuleBuilder::default();
    float_const.types.push(functype(&[], &[I32]));
    float_const.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: vec![0x43, 0, 0, 0, 0],
    });
    expect_err(&float_const.build(), "NOT_IMPLEMENTED");

    let mut imports = b"\0asm\x01\0\0\0".to_vec();
    imports.extend(section(2, vec_of(&[name("env")])));
    expect_err(&imports, "NOT_IMPLEMENTED");

    let mut multi = ModuleBuilder::default();
    multi.types.push(functype(&[], &[I32, I32]));
    expect_err(&multi.build(), "NOT_IMPLEMENTED");

    let mut indirect = ModuleBuilder::default();
    indirect.types.push(functype(&[], &[]));
    indirect.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: vec![0x41, 0, 0x11, 0, 0],
    });
    expect_err(&indirect.build(), "NOT_IMPLEMENTED");
}

#[test]
fn type_errors_and_stack_errors_are_invalid_format() {
    let mut mismatch = ModuleBuilder::default();
    mismatch.types.push(functype(&[], &[I32]));
    mismatch.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: [i64_const(1), i32_const(1), vec![0x6a]].concat(),
    });
    expect_err(&mismatch.build(), "INVALID_FORMAT");

    let mut underflow = ModuleBuilder::default();
    underflow.types.push(functype(&[], &[I32]));
    underflow.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: vec![0x6a],
    });
    expect_err(&underflow.build(), "INVALID_FORMAT");

    let mut leftover = ModuleBuilder::default();
    leftover.types.push(functype(&[], &[I32]));
    leftover.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: i32_const(1).into_iter().chain(i32_const(2)).collect(),
    });
    expect_err(&leftover.build(), "INVALID_FORMAT");

    let mut bad_local = ModuleBuilder::default();
    bad_local.types.push(functype(&[], &[]));
    bad_local.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: local_get(3),
    });
    expect_err(&bad_local.build(), "INVALID_FORMAT");

    let mut bad_branch = ModuleBuilder::default();
    bad_branch.types.push(functype(&[], &[]));
    bad_branch.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: br(4),
    });
    expect_err(&bad_branch.build(), "INVALID_FORMAT");

    let mut immut = ModuleBuilder::default();
    immut.types.push(functype(&[], &[]));
    immut.globals.push(Global {
        ty: I32,
        mutable: false,
        init: i32_const(0),
    });
    immut.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: [i32_const(1), global_set(0)].concat(),
    });
    expect_err(&immut.build(), "INVALID_FORMAT");

    let mut no_mem = ModuleBuilder::default();
    no_mem.types.push(functype(&[], &[I32]));
    no_mem.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: [i32_const(0), mem(0x28, 2, 0)].concat(),
    });
    expect_err(&no_mem.build(), "INVALID_FORMAT");

    let mut align = ModuleBuilder::default();
    align.types.push(functype(&[], &[I32]));
    align.memory = Some((1, None));
    align.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: [i32_const(0), mem(0x28, 3, 0)].concat(),
    });
    expect_err(&align.build(), "INVALID_FORMAT");
}

#[test]
fn limits_are_enforced_at_parse_time() {
    let big_mem = ModuleBuilder {
        memory: Some((257, None)),
        ..Default::default()
    };
    expect_err(&big_mem.build(), "LIMIT_EXCEEDED");

    let big_max = ModuleBuilder {
        memory: Some((1, Some(300))),
        ..Default::default()
    };
    expect_err(&big_max.build(), "LIMIT_EXCEEDED");

    let huge = vec![0u8; limits::MAX_MODULE_BYTES + 1];
    expect_err(&huge, "LIMIT_EXCEEDED");
}

#[test]
fn exports_are_validated() {
    let mut dup = ModuleBuilder::default();
    dup.types.push(functype(&[], &[]));
    dup.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: vec![],
    });
    dup.exports.push(("f".into(), 0, 0));
    dup.exports.push(("f".into(), 0, 0));
    expect_err(&dup.build(), "INVALID_FORMAT");

    let mut unknown = ModuleBuilder::default();
    unknown.types.push(functype(&[], &[]));
    unknown.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: vec![],
    });
    unknown.exports.push(("g".into(), 0, 9));
    expect_err(&unknown.build(), "INVALID_FORMAT");
}

#[test]
fn data_segments_must_fit_memory() {
    let mut b = ModuleBuilder {
        memory: Some((1, None)),
        ..Default::default()
    };
    b.data
        .push((limits::PAGE_SIZE as u32 - 2, vec![1, 2, 3, 4]));
    let m = Module::parse(&b.build()).unwrap();
    assert_eq!(
        Instance::new(m, 0).unwrap_err().code,
        "INSTANTIATION_FAILED"
    );
}

#[test]
fn custom_sections_are_skipped() {
    let mut b = ModuleBuilder::default();
    b.types.push(functype(&[], &[I32]));
    b.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: i32_const(7),
    });
    b.exports.push(("seven".into(), 0, 0));
    let mut bytes = b.build();
    bytes.extend(section(0, [name("meta"), vec![1, 2, 3]].concat()));
    assert_eq!(run(&bytes, "seven", &[], 10).unwrap(), vec![Value::I32(7)]);
}

#[test]
fn i64_paths_and_conversions() {
    let mut b = ModuleBuilder::default();
    b.types.push(functype(&[I32], &[I64]));
    b.types.push(functype(&[I64], &[I32]));
    // extend_i32_s, then i64.extend8_s on the low byte
    let mut ext = local_get(0);
    ext.push(0xac);
    b.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body: ext,
    });
    let mut wrap = local_get(0);
    wrap.push(0xa7);
    b.funcs.push(FuncDef {
        type_idx: 1,
        locals: vec![],
        body: wrap,
    });
    b.exports.push(("ext".into(), 0, 0));
    b.exports.push(("wrap".into(), 0, 1));
    let bytes = b.build();
    assert_eq!(
        run(&bytes, "ext", &[Value::I32(-3)], 10).unwrap(),
        vec![Value::I64(-3)]
    );
    assert_eq!(
        run(&bytes, "wrap", &[Value::I64(0x1_0000_0005)], 10).unwrap(),
        vec![Value::I32(5)]
    );
}
