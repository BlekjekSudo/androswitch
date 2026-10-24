//! Guest-level Parcel/Binder proofs for Appendix C literal-completion
//! Round 2: a DEX `Parcelable` is marshalled to bytes and materialized by
//! its own `CREATOR`, and a DEX `Binder` stub's `onTransact` is reached
//! through `Binder.transact` with real Parcel request/reply streams.

mod common;

use common::*;
use rd_vm::framework::HostObj;
use rd_vm::intrinsics::alloc_string;
use rd_vm::value::Value;

const ACC_CONSTRUCTOR: u32 = 0x1_0000;
const PARCEL: &str = "Landroid/os/Parcel;";
const POINT: &str = "Lrd/test/ParcelPoint;";
const CREATOR_CLASS: &str = "Lrd/test/ParcelPoint$Creator;";
const CREATOR_IFACE: &str = "Landroid/os/Parcelable$Creator;";
const BINDER: &str = "Landroid/os/Binder;";
const ECHO: &str = "Lrd/test/EchoBinder;";
const BINDER_IDENTITY: &str = "Lrd/test/BinderIdentity;";
const INTERFACE_ARRAY_FACTORY: &str = "Lrd/test/InterfaceArrayFactory;";
const RECORDING_PROTO: &str = "Lrd/test/RecordingProto;";
const RECORDING_WRITER: &str = "Lrd/test/RecordingIndentingPrintWriter;";
const BUNDLE_CLASS: &str = "Landroid/os/Bundle;";
const BASE_BUNDLE_CLASS: &str = "Landroid/os/BaseBundle;";
const DESCRIPTOR: &str = "rd.test.IEcho";

fn obj(v: Value) -> u32 {
    match v {
        Value::Obj(r) => r,
        other => panic!("expected object, got {other:?}"),
    }
}

fn build() -> DexBuilder {
    let mut b = DexBuilder::new();

    // Method refs used by the DEX bodies below.
    let void0 = b.proto_idx("V", vec![]);
    let obj_init = b.method_idx("Ljava/lang/Object;", void0, "<init>");
    let binder_init = b.method_idx(BINDER, void0, "<init>");
    let write_int = {
        let p = b.proto_idx("V", vec!["I".into()]);
        b.method_idx(PARCEL, p, "writeInt")
    };
    let write_string = {
        let p = b.proto_idx("V", vec!["Ljava/lang/String;".into()]);
        b.method_idx(PARCEL, p, "writeString")
    };
    let read_int = {
        let p = b.proto_idx("I", vec![]);
        b.method_idx(PARCEL, p, "readInt")
    };
    let read_string = {
        let p = b.proto_idx("Ljava/lang/String;", vec![]);
        b.method_idx(PARCEL, p, "readString")
    };
    let enforce = {
        let p = b.proto_idx("V", vec!["Ljava/lang/String;".into()]);
        b.method_idx(PARCEL, p, "enforceInterface")
    };
    let write_no_exc = {
        let p = b.proto_idx("V", vec![]);
        b.method_idx(PARCEL, p, "writeNoException")
    };
    let creator_init = {
        let p = b.proto_idx("V", vec![]);
        b.method_idx(CREATOR_CLASS, p, "<init>")
    };

    // ── ParcelPoint implements Parcelable (host does not need the interface
    // list to prove Creator materialization) ───────────────────────────────
    let point = b.class(POINT, "Ljava/lang/Object;");
    b.instance_field(point, "x", "I");
    b.instance_field(point, "label", "Ljava/lang/String;");
    b.static_field(point, "CREATOR", CREATOR_IFACE, SVal::Int(0));
    let fx = b.field_idx(POINT, "I", "x");
    let flabel = b.field_idx(POINT, "Ljava/lang/String;", "label");
    let fcreator = b.field_idx(POINT, CREATOR_IFACE, "CREATOR");
    let point_tidx = b.type_idx_cached(POINT);
    let creator_tidx = b.type_idx(CREATOR_CLASS);

    // DexBuilder's class_data delta encoding needs each class's methods in
    // method-index order, so ParcelPoint's directs are declared in the order
    // their refs are first interned below.
    // <init>(Parcel)V: this=v2, parcel=v3
    b.direct(
        point,
        "<init>",
        "V",
        vec![PARCEL],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(4, 2, 1, {
            let mut u = op35c(0x70, 1, obj_init, [2, 0, 0, 0, 0]);
            u.extend(op35c(0x6e, 1, read_int, [3, 0, 0, 0, 0]));
            u.extend(op11x(0x0a, 0)); // move-result v0
            u.extend(op22c(0x59, 0, 2, fx)); // iput v0, v2, x
            u.extend(op35c(0x6e, 1, read_string, [3, 0, 0, 0, 0]));
            u.extend(op11x(0x0c, 0)); // move-result-object v0
            u.extend(op22c(0x5b, 0, 2, flabel)); // iput-object v0, v2, label
            u.extend(op10x(0x0e));
            u
        })),
    );
    // <init>()V
    b.direct(
        point,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, obj_init, [0, 0, 0, 0, 0]);
            u.extend(op10x(0x0e));
            u
        })),
    );
    // <clinit>: CREATOR = new ParcelPoint$Creator()
    b.direct(
        point,
        "<clinit>",
        "V",
        vec![],
        ACC_STATIC | ACC_CONSTRUCTOR,
        Some(b.code(2, 0, 1, {
            let mut u = op21c(0x22, 0, creator_tidx);
            u.extend(op35c(0x70, 1, creator_init, [0, 0, 0, 0, 0]));
            u.extend(op21c(0x69, 0, fcreator)); // sput-object v0, CREATOR
            u.extend(op10x(0x0e));
            u
        })),
    );
    // writeToParcel(Parcel, I)V: this=v2, parcel=v3, flags=v4
    b.r#virtual(
        point,
        "writeToParcel",
        "V",
        vec![PARCEL, "I"],
        ACC_PUBLIC,
        Some(b.code(5, 3, 2, {
            let mut u = op22c(0x52, 0, 2, fx); // iget v0, v2, x
            u.extend(op35c(0x6e, 2, write_int, [3, 0, 0, 0, 0]));
            u.extend(op22c(0x54, 0, 2, flabel)); // iget-object v0, v2, label
            u.extend(op35c(0x6e, 2, write_string, [3, 0, 0, 0, 0]));
            u.extend(op10x(0x0e));
            u
        })),
    );
    // describeContents()I = 0
    b.r#virtual(
        point,
        "describeContents",
        "I",
        vec![],
        ACC_PUBLIC,
        Some(b.code(2, 1, 0, {
            let mut u = op11n(0x12, 0, 0);
            u.extend(op11x(0x0f, 0));
            u
        })),
    );

    // ── ParcelPoint$Creator ────────────────────────────────────────────────
    let creator = b.class(CREATOR_CLASS, "Ljava/lang/Object;");
    b.direct(
        creator,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, obj_init, [0, 0, 0, 0, 0]);
            u.extend(op10x(0x0e));
            u
        })),
    );
    let point_parcel_init = {
        let p = b.proto_idx("V", vec![PARCEL.into()]);
        b.method_idx(POINT, p, "<init>")
    };
    // createFromParcel(Parcel)Object: this=v2, parcel=v3, out=v0
    b.r#virtual(
        creator,
        "createFromParcel",
        "Ljava/lang/Object;",
        vec![PARCEL],
        ACC_PUBLIC,
        Some(b.code(4, 2, 2, {
            let mut u = op21c(0x22, 0, point_tidx);
            u.extend(op35c(0x70, 2, point_parcel_init, [0, 3, 0, 0, 0]));
            u.extend(op11x(0x11, 0)); // return-object v0
            u
        })),
    );
    // newArray(I)[Object: this=v1, size=v2, out=v0
    let point_array_tidx = b.type_idx("[Lrd/test/ParcelPoint;");
    b.r#virtual(
        creator,
        "newArray",
        "[Ljava/lang/Object;",
        vec!["I"],
        ACC_PUBLIC,
        Some(b.code(3, 2, 0, {
            let mut u = op22c(0x23, 0, 2, point_array_tidx);
            u.extend(op11x(0x11, 0));
            u
        })),
    );

    // ── EchoBinder extends Binder: onTransact echoes readInt()+1 ──────────
    let echo = b.class(ECHO, BINDER);
    b.direct(
        echo,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, binder_init, [0, 0, 0, 0, 0]);
            u.extend(op10x(0x0e));
            u
        })),
    );
    let descriptor_sidx = b.intern(DESCRIPTOR) as u16;
    // onTransact(I,Parcel,Parcel,I)Z: this=v3 code=v4 data=v5 reply=v6 flags=v7
    b.r#virtual(
        echo,
        "onTransact",
        "Z",
        vec!["I", PARCEL, PARCEL, "I"],
        ACC_PUBLIC,
        Some(b.code(8, 5, 2, {
            let mut u = op21c(0x1a, 0, descriptor_sidx); // const-string v0
            u.extend(op35c(0x6e, 2, enforce, [5, 0, 0, 0, 0]));
            u.extend(op35c(0x6e, 1, read_int, [5, 0, 0, 0, 0]));
            u.extend(op11x(0x0a, 0)); // move-result v0
            u.extend(op22b(0xd8, 0, 0, 1)); // add-int/lit8 v0, v0, 1
            u.extend(op35c(0x6e, 1, write_no_exc, [6, 0, 0, 0, 0]));
            u.extend(op35c(0x6e, 2, write_int, [6, 0, 0, 0, 0]));
            u.extend(op11n(0x12, 0, 1)); // const/4 v0, 1
            u.extend(op11x(0x0f, 0)); // return v0
            u
        })),
    );

    // ── Function<IBinder, IInterface> identity ───────────────────────────
    let identity = b.class(BINDER_IDENTITY, "Ljava/lang/Object;");
    b.direct(
        identity,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, obj_init, [0, 0, 0, 0, 0]);
            u.extend(op10x(0x0e));
            u
        })),
    );
    // apply(Object)Object: this=v1, arg=v2.
    b.r#virtual(
        identity,
        "apply",
        "Ljava/lang/Object;",
        vec!["Ljava/lang/Object;"],
        ACC_PUBLIC,
        Some(b.code(3, 2, 0, op11x(0x11, 2))),
    );

    // ── IntFunction<IInterface[]> ─────────────────────────────────────────
    let factory = b.class(INTERFACE_ARRAY_FACTORY, "Ljava/lang/Object;");
    b.direct(
        factory,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, obj_init, [0, 0, 0, 0, 0]);
            u.extend(op10x(0x0e));
            u
        })),
    );
    let interface_array_tidx = b.type_idx("[Landroid/os/IInterface;");
    // apply(I)Object: this=v1, size=v2, out=v0.
    b.r#virtual(
        factory,
        "apply",
        "Ljava/lang/Object;",
        vec!["I"],
        ACC_PUBLIC,
        Some(b.code(3, 2, 0, {
            let mut u = op22c(0x23, 0, 2, interface_array_tidx);
            u.extend(op11x(0x11, 0));
            u
        })),
    );

    // ── Recording ProtoOutputStream-shaped sink ───────────────────────────
    let proto = b.class(RECORDING_PROTO, "Ljava/lang/Object;");
    for (name, ftype) in [
        ("startField", "J"),
        ("token", "J"),
        ("startCount", "I"),
        ("intField", "J"),
        ("intValue", "I"),
        ("hasInt", "I"),
        ("stringField", "J"),
        ("stringValue", "Ljava/lang/String;"),
        ("hasString", "I"),
        ("endToken", "J"),
        ("endCount", "I"),
    ] {
        b.instance_field(proto, name, ftype);
    }
    let f_proto_start_field = b.field_idx(RECORDING_PROTO, "J", "startField");
    let f_proto_token = b.field_idx(RECORDING_PROTO, "J", "token");
    let f_proto_start_count = b.field_idx(RECORDING_PROTO, "I", "startCount");
    let f_proto_int_field = b.field_idx(RECORDING_PROTO, "J", "intField");
    let f_proto_int_value = b.field_idx(RECORDING_PROTO, "I", "intValue");
    let f_proto_has_int = b.field_idx(RECORDING_PROTO, "I", "hasInt");
    let f_proto_string_field = b.field_idx(RECORDING_PROTO, "J", "stringField");
    let f_proto_string_value = b.field_idx(RECORDING_PROTO, "Ljava/lang/String;", "stringValue");
    let f_proto_has_string = b.field_idx(RECORDING_PROTO, "I", "hasString");
    let f_proto_end_token = b.field_idx(RECORDING_PROTO, "J", "endToken");
    let f_proto_end_count = b.field_idx(RECORDING_PROTO, "I", "endCount");
    b.direct(
        proto,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, obj_init, [0, 0, 0, 0, 0]);
            u.extend(op10x(0x0e));
            u
        })),
    );
    // start(J)J: locals v0/v1, this=v2, fieldId=v3/v4.
    b.r#virtual(
        proto,
        "start",
        "J",
        vec!["J"],
        ACC_PUBLIC,
        Some(b.code(5, 3, 0, {
            let mut u = op22c(0x5a, 3, 2, f_proto_start_field);
            u.extend(op22c(0x52, 0, 2, f_proto_start_count));
            u.extend(op22b(0xd8, 0, 0, 1));
            u.extend(op22c(0x59, 0, 2, f_proto_start_count));
            u.extend(op22c(0x53, 0, 2, f_proto_token));
            u.extend(op11x(0x10, 0));
            u
        })),
    );
    // write(JI)V: locals v0/v1, this=v2, fieldId=v3/v4, value=v5.
    b.r#virtual(
        proto,
        "write",
        "V",
        vec!["J", "I"],
        ACC_PUBLIC,
        Some(b.code(6, 4, 0, {
            let mut u = op22c(0x5a, 3, 2, f_proto_int_field);
            u.extend(op22c(0x59, 5, 2, f_proto_int_value));
            u.extend(op11n(0x12, 0, 1));
            u.extend(op22c(0x59, 0, 2, f_proto_has_int));
            u.extend(op10x(0x0e));
            u
        })),
    );
    // write(J,String;)V: locals v0/v1, this=v2, fieldId=v3/v4, value=v5.
    b.r#virtual(
        proto,
        "write",
        "V",
        vec!["J", "Ljava/lang/String;"],
        ACC_PUBLIC,
        Some(b.code(6, 4, 0, {
            let mut u = op22c(0x5a, 3, 2, f_proto_string_field);
            u.extend(op22c(0x5b, 5, 2, f_proto_string_value));
            u.extend(op11n(0x12, 0, 1));
            u.extend(op22c(0x59, 0, 2, f_proto_has_string));
            u.extend(op10x(0x0e));
            u
        })),
    );
    // end(J)V: locals v0/v1, this=v2, token=v3/v4.
    b.r#virtual(
        proto,
        "end",
        "V",
        vec!["J"],
        ACC_PUBLIC,
        Some(b.code(5, 3, 0, {
            let mut u = op22c(0x5a, 3, 2, f_proto_end_token);
            u.extend(op22c(0x52, 0, 2, f_proto_end_count));
            u.extend(op22b(0xd8, 0, 0, 1));
            u.extend(op22c(0x59, 0, 2, f_proto_end_count));
            u.extend(op10x(0x0e));
            u
        })),
    );

    // ── Recording IndentingPrintWriter-shaped sink ────────────────────────
    let writer = b.class(RECORDING_WRITER, "Ljava/lang/Object;");
    for (name, ftype) in [
        ("indent", "I"),
        ("increases", "I"),
        ("decreases", "I"),
        ("lineCount", "I"),
        ("lastLine", "Ljava/lang/String;"),
    ] {
        b.instance_field(writer, name, ftype);
    }
    let f_writer_indent = b.field_idx(RECORDING_WRITER, "I", "indent");
    let f_writer_increases = b.field_idx(RECORDING_WRITER, "I", "increases");
    let f_writer_decreases = b.field_idx(RECORDING_WRITER, "I", "decreases");
    let f_writer_line_count = b.field_idx(RECORDING_WRITER, "I", "lineCount");
    let f_writer_last_line = b.field_idx(RECORDING_WRITER, "Ljava/lang/String;", "lastLine");
    b.direct(
        writer,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, obj_init, [0, 0, 0, 0, 0]);
            u.extend(op10x(0x0e));
            u
        })),
    );
    // increaseIndent(): locals v0/v1, this=v2.
    b.r#virtual(
        writer,
        "increaseIndent",
        "Lcom/android/internal/util/IndentingPrintWriter;",
        vec![],
        ACC_PUBLIC,
        Some(b.code(3, 1, 0, {
            let mut u = op22c(0x52, 0, 2, f_writer_indent);
            u.extend(op22b(0xd8, 0, 0, 1));
            u.extend(op22c(0x59, 0, 2, f_writer_indent));
            u.extend(op22c(0x52, 0, 2, f_writer_increases));
            u.extend(op22b(0xd8, 0, 0, 1));
            u.extend(op22c(0x59, 0, 2, f_writer_increases));
            u.extend(op11x(0x11, 2));
            u
        })),
    );
    // decreaseIndent(): locals v0/v1, this=v2.
    b.r#virtual(
        writer,
        "decreaseIndent",
        "Lcom/android/internal/util/IndentingPrintWriter;",
        vec![],
        ACC_PUBLIC,
        Some(b.code(3, 1, 0, {
            let mut u = op22c(0x52, 0, 2, f_writer_indent);
            u.extend(op22b(0xd8, 0, 0, -1));
            u.extend(op22c(0x59, 0, 2, f_writer_indent));
            u.extend(op22c(0x52, 0, 2, f_writer_decreases));
            u.extend(op22b(0xd8, 0, 0, 1));
            u.extend(op22c(0x59, 0, 2, f_writer_decreases));
            u.extend(op11x(0x11, 2));
            u
        })),
    );
    // println(String;)V: locals v0/v1, this=v2, line=v3.
    b.r#virtual(
        writer,
        "println",
        "V",
        vec!["Ljava/lang/String;"],
        ACC_PUBLIC,
        Some(b.code(4, 2, 0, {
            let mut u = op22c(0x5b, 3, 2, f_writer_last_line);
            u.extend(op22c(0x52, 0, 2, f_writer_line_count));
            u.extend(op22b(0xd8, 0, 0, 1));
            u.extend(op22c(0x59, 0, 2, f_writer_line_count));
            u.extend(op10x(0x0e));
            u
        })),
    );

    b
}

fn parcel(e: &mut rd_vm::engine::Engine) -> u32 {
    obj(e
        .invoke_static(PARCEL, "obtain", "()Landroid/os/Parcel;", &[])
        .unwrap())
}

#[test]
fn dex_parcelable_creator_materializes_from_bytes() {
    let b = build();
    let mut e = engine_of(&b);

    let point = e.new_instance(POINT).unwrap();
    e.heap.put_field(point, "x", Value::Int(1234)).unwrap();
    let label = alloc_string(&mut e, "creator-roundtrip".to_string()).unwrap();
    e.heap.put_field(point, "label", Value::Obj(label)).unwrap();

    let p = parcel(&mut e);
    e.invoke_instance(
        p,
        "writeParcelable",
        "(Landroid/os/Parcelable;I)V",
        &[Value::Obj(point), Value::Int(0)],
    )
    .unwrap();
    let size = e.invoke_instance(p, "dataSize", "()I", &[]).unwrap();
    let bytes = e.invoke_instance(p, "marshall", "()[B", &[]).unwrap();

    let p2 = parcel(&mut e);
    e.invoke_instance(p2, "unmarshall", "([BII)V", &[bytes, Value::Int(0), size])
        .unwrap();
    let got = obj(e
        .invoke_instance(
            p2,
            "readParcelable",
            "(Ljava/lang/ClassLoader;)Landroid/os/Parcelable;",
            &[Value::Null],
        )
        .unwrap());
    assert_ne!(got, point, "Creator must materialize a fresh instance");
    assert_eq!(e.heap.class_of(got).unwrap(), POINT);
    assert_eq!(e.heap.get_field(got, "x").unwrap(), Value::Int(1234));
    let got_label = obj(e.heap.get_field(got, "label").unwrap());
    assert_eq!(e.heap.as_str(got_label).unwrap(), "creator-roundtrip");
}

#[test]
fn dex_typed_list_uses_the_supplied_creator() {
    let b = build();
    let mut e = engine_of(&b);
    let point = e.new_instance(POINT).unwrap();
    e.heap.put_field(point, "x", Value::Int(77)).unwrap();
    let label = alloc_string(&mut e, "typed".to_string()).unwrap();
    e.heap.put_field(point, "label", Value::Obj(label)).unwrap();

    let creator = e.new_instance(CREATOR_CLASS).unwrap();
    let list = e
        .heap
        .alloc_instance("Ljava/util/ArrayList;".to_string(), Vec::new())
        .unwrap();
    e.fw.objects.insert(
        list,
        HostObj::List {
            elems: vec![Value::Obj(point), Value::Null],
        },
    );

    let p = parcel(&mut e);
    e.invoke_instance(
        p,
        "writeTypedList",
        "(Ljava/util/List;)V",
        &[Value::Obj(list)],
    )
    .unwrap();
    e.invoke_instance(p, "setDataPosition", "(I)V", &[Value::Int(0)])
        .unwrap();
    let got_list = obj(e
        .invoke_instance(
            p,
            "createTypedArrayList",
            "(Landroid/os/Parcelable$Creator;)Ljava/util/ArrayList;",
            &[Value::Obj(creator)],
        )
        .unwrap());
    let elems = match e.fw.get(got_list) {
        Some(HostObj::List { elems }) => elems.clone(),
        other => panic!("typed list result: {other:?}"),
    };
    assert_eq!(elems.len(), 2);
    let got = obj(elems[0].clone());
    assert_eq!(e.heap.get_field(got, "x").unwrap(), Value::Int(77));
    let got_label = obj(e.heap.get_field(got, "label").unwrap());
    assert_eq!(e.heap.as_str(got_label).unwrap(), "typed");
    assert_eq!(elems[1], Value::Null);
}

#[test]
fn dex_binder_stub_on_transact_runs_through_transact() {
    let b = build();
    let mut e = engine_of(&b);
    let binder = e.new_instance(ECHO).unwrap();
    let descriptor = alloc_string(&mut e, DESCRIPTOR.to_string()).unwrap();
    e.invoke_instance(
        binder,
        "attachInterface",
        "(Landroid/os/IInterface;Ljava/lang/String;)V",
        &[Value::Obj(binder), Value::Obj(descriptor)],
    )
    .unwrap();
    assert_eq!(
        e.invoke_instance(
            binder,
            "queryLocalInterface",
            "(Ljava/lang/String;)Landroid/os/IInterface;",
            &[Value::Obj(descriptor)],
        )
        .unwrap(),
        Value::Obj(binder)
    );

    let data = parcel(&mut e);
    let reply = parcel(&mut e);
    e.invoke_instance(
        data,
        "writeInterfaceToken",
        "(Ljava/lang/String;)V",
        &[Value::Obj(descriptor)],
    )
    .unwrap();
    e.invoke_instance(data, "writeInt", "(I)V", &[Value::Int(41)])
        .unwrap();
    e.invoke_instance(data, "setDataPosition", "(I)V", &[Value::Int(0)])
        .unwrap();

    assert_eq!(
        e.invoke_instance(
            binder,
            "transact",
            "(ILandroid/os/Parcel;Landroid/os/Parcel;I)Z",
            &[
                Value::Int(1),
                Value::Obj(data),
                Value::Obj(reply),
                Value::Int(0),
            ],
        )
        .unwrap(),
        Value::Int(1)
    );
    e.invoke_instance(reply, "setDataPosition", "(I)V", &[Value::Int(0)])
        .unwrap();
    e.invoke_instance(reply, "readException", "()V", &[])
        .unwrap();
    assert_eq!(
        e.invoke_instance(reply, "readInt", "()I", &[]).unwrap(),
        Value::Int(42)
    );
}

#[test]
fn dex_interface_containers_use_function_callbacks() {
    let b = build();
    let mut e = engine_of(&b);
    let binder = e.new_instance(ECHO).unwrap();
    let identity = e.new_instance(BINDER_IDENTITY).unwrap();
    let factory = e.new_instance(INTERFACE_ARRAY_FACTORY).unwrap();
    let p = parcel(&mut e);

    let source_array = e
        .heap
        .alloc_array_zeroed(
            rd_vm::heap::ElemKind::Obj,
            2,
            "[Landroid/os/IInterface;",
        )
        .unwrap();
    e.heap
        .array_elems_mut(source_array)
        .unwrap()
        .clone_from_slice(&[Value::Obj(binder), Value::Null]);
    e.invoke_instance(
        p,
        "writeInterfaceArray",
        "([Landroid/os/IInterface;)V",
        &[Value::Obj(source_array)],
    )
    .unwrap();

    e.invoke_instance(p, "setDataPosition", "(I)V", &[Value::Int(0)])
        .unwrap();
    let got_array = obj(e
        .invoke_instance(
            p,
            "createInterfaceArray",
            "(Ljava/util/function/IntFunction;Ljava/util/function/Function;)[Landroid/os/IInterface;",
            &[Value::Obj(factory), Value::Obj(identity)],
        )
        .unwrap());
    assert_eq!(
        e.heap.class_of(got_array).unwrap(),
        "[Landroid/os/IInterface;"
    );
    assert_eq!(
        e.heap.as_array_elems(got_array).unwrap().0,
        &[Value::Obj(binder), Value::Null]
    );

    let dest_array = e
        .heap
        .alloc_array_zeroed(
            rd_vm::heap::ElemKind::Obj,
            2,
            "[Landroid/os/IInterface;",
        )
        .unwrap();
    e.invoke_instance(p, "setDataPosition", "(I)V", &[Value::Int(0)])
        .unwrap();
    e.invoke_instance(
        p,
        "readInterfaceArray",
        "([Landroid/os/IInterface;Ljava/util/function/Function;)V",
        &[Value::Obj(dest_array), Value::Obj(identity)],
    )
    .unwrap();
    assert_eq!(
        e.heap.as_array_elems(dest_array).unwrap().0,
        &[Value::Obj(binder), Value::Null]
    );

    let list_pos = e.invoke_instance(p, "dataSize", "()I", &[]).unwrap();
    e.invoke_instance(p, "setDataPosition", "(I)V", &[list_pos.clone()])
        .unwrap();
    let source_list = e
        .heap
        .alloc_instance("Ljava/util/ArrayList;".to_string(), Vec::new())
        .unwrap();
    e.fw.objects.insert(
        source_list,
        HostObj::List {
            elems: vec![Value::Obj(binder)],
        },
    );
    e.invoke_instance(
        p,
        "writeInterfaceList",
        "(Ljava/util/List;)V",
        &[Value::Obj(source_list)],
    )
    .unwrap();
    e.invoke_instance(p, "setDataPosition", "(I)V", &[list_pos.clone()])
        .unwrap();
    let got_list = obj(e
        .invoke_instance(
            p,
            "createInterfaceArrayList",
            "(Ljava/util/function/Function;)Ljava/util/ArrayList;",
            &[Value::Obj(identity)],
        )
        .unwrap());
    assert!(matches!(
        e.fw.get(got_list),
        Some(HostObj::List { elems }) if elems == &[Value::Obj(binder)]
    ));

    let dest_list = e
        .heap
        .alloc_instance("Ljava/util/ArrayList;".to_string(), Vec::new())
        .unwrap();
    e.fw.objects.insert(
        dest_list,
        HostObj::List {
            elems: vec![Value::Null],
        },
    );
    e.invoke_instance(p, "setDataPosition", "(I)V", &[list_pos])
        .unwrap();
    e.invoke_instance(
        p,
        "readInterfaceList",
        "(Ljava/util/List;Ljava/util/function/Function;)V",
        &[Value::Obj(dest_list), Value::Obj(identity)],
    )
    .unwrap();
    assert!(matches!(
        e.fw.get(dest_list),
        Some(HostObj::List { elems }) if elems == &[Value::Obj(binder)]
    ));
}

#[test]
fn dex_bundle_dump_debug_and_dump_stats_use_guest_writers() {
    const PARCELLED_DATA_SIZE_FIELD: i64 = 1_120_986_464_257;
    const MAP_DATA_FIELD: i64 = 1_138_166_333_442;

    let b = build();
    let mut e = engine_of(&b);

    fn init_proto(e: &mut rd_vm::engine::Engine, proto: u32, token: i64) {
        for (field, value) in [
            ("startField", Value::Long(0)),
            ("token", Value::Long(token)),
            ("startCount", Value::Int(0)),
            ("intField", Value::Long(0)),
            ("intValue", Value::Int(0)),
            ("hasInt", Value::Int(0)),
            ("stringField", Value::Long(0)),
            ("stringValue", Value::Null),
            ("hasString", Value::Int(0)),
            ("endToken", Value::Long(0)),
            ("endCount", Value::Int(0)),
        ] {
            e.heap.put_field(proto, field, value).unwrap();
        }
    }
    fn init_writer(e: &mut rd_vm::engine::Engine, writer: u32) {
        for field in ["indent", "increases", "decreases", "lineCount"] {
            e.heap.put_field(writer, field, Value::Int(0)).unwrap();
        }
        e.heap.put_field(writer, "lastLine", Value::Null).unwrap();
    }
    fn text_of(e: &rd_vm::engine::Engine, v: Value) -> String {
        let r = obj(v);
        e.heap.as_str(r).unwrap().to_string()
    }

    // Materialized Bundle: dumpDebug writes the map_data string field.
    let bundle = e.new_instance(BUNDLE_CLASS).unwrap();
    let name_key = alloc_string(&mut e, "name".to_string()).unwrap();
    let name_value = alloc_string(&mut e, "Rusty".to_string()).unwrap();
    e.invoke_instance(
        bundle,
        "putString",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[Value::Obj(name_key), Value::Obj(name_value)],
    )
    .unwrap();
    let count_key = alloc_string(&mut e, "count".to_string()).unwrap();
    e.invoke_instance(
        bundle,
        "putInt",
        "(Ljava/lang/String;I)V",
        &[Value::Obj(count_key), Value::Int(3)],
    )
    .unwrap();

    let proto = e.new_instance(RECORDING_PROTO).unwrap();
    init_proto(&mut e, proto, 99);
    e.invoke_instance(
        bundle,
        "dumpDebug",
        "(Landroid/util/proto/ProtoOutputStream;J)V",
        &[Value::Obj(proto), Value::Long(777)],
    )
    .unwrap();
    assert_eq!(e.heap.get_field(proto, "startField").unwrap(), Value::Long(777));
    assert_eq!(e.heap.get_field(proto, "startCount").unwrap(), Value::Int(1));
    assert_eq!(e.heap.get_field(proto, "hasInt").unwrap(), Value::Int(0));
    assert_eq!(e.heap.get_field(proto, "hasString").unwrap(), Value::Int(1));
    assert_eq!(
        e.heap.get_field(proto, "stringField").unwrap(),
        Value::Long(MAP_DATA_FIELD)
    );
    let map_text = text_of(&e, e.heap.get_field(proto, "stringValue").unwrap());
    assert!(map_text.contains("name=Rusty"), "map text: {map_text}");
    assert!(map_text.contains("count=3"), "map text: {map_text}");
    assert_eq!(e.heap.get_field(proto, "endToken").unwrap(), Value::Long(99));
    assert_eq!(e.heap.get_field(proto, "endCount").unwrap(), Value::Int(1));

    // Parcelled Bundle: dumpDebug writes only the retained data size and
    // leaves the Bundle lazy.
    let source = e.new_instance(BUNDLE_CLASS).unwrap();
    let k = alloc_string(&mut e, "k".to_string()).unwrap();
    let v = alloc_string(&mut e, "v".to_string()).unwrap();
    e.invoke_instance(
        source,
        "putString",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[Value::Obj(k), Value::Obj(v)],
    )
    .unwrap();
    let p = parcel(&mut e);
    e.invoke_instance(p, "writeBundle", "(Landroid/os/Bundle;)V", &[Value::Obj(source)])
        .unwrap();
    let frame_size = match e.invoke_instance(p, "dataSize", "()I", &[]).unwrap() {
        Value::Int(n) => n,
        other => panic!("dataSize: {other:?}"),
    };
    e.invoke_instance(p, "setDataPosition", "(I)V", &[Value::Int(0)])
        .unwrap();
    let parcelled = obj(e
        .invoke_instance(
            p,
            "readBundle",
            "(Ljava/lang/ClassLoader;)Landroid/os/Bundle;",
            &[Value::Null],
        )
        .unwrap());
    assert_eq!(
        e.invoke_instance(parcelled, "isParcelled", "()Z", &[]).unwrap(),
        Value::Int(1)
    );
    let proto2 = e.new_instance(RECORDING_PROTO).unwrap();
    init_proto(&mut e, proto2, 55);
    e.invoke_instance(
        parcelled,
        "dumpDebug",
        "(Landroid/util/proto/ProtoOutputStream;J)V",
        &[Value::Obj(proto2), Value::Long(888)],
    )
    .unwrap();
    assert_eq!(e.heap.get_field(proto2, "hasString").unwrap(), Value::Int(0));
    assert_eq!(e.heap.get_field(proto2, "hasInt").unwrap(), Value::Int(1));
    assert_eq!(
        e.heap.get_field(proto2, "intField").unwrap(),
        Value::Long(PARCELLED_DATA_SIZE_FIELD)
    );
    assert_eq!(
        e.heap.get_field(proto2, "intValue").unwrap(),
        Value::Int(frame_size - 8),
        "retained ArrayMap payload excludes the Bundle frame length+magic"
    );
    assert_eq!(e.heap.get_field(proto2, "endToken").unwrap(), Value::Long(55));
    assert_eq!(
        e.invoke_instance(parcelled, "isParcelled", "()Z", &[]).unwrap(),
        Value::Int(1),
        "dumpDebug must not materialize a parcelled Bundle"
    );

    // dumpStats: small values are silent; a >1 KiB value is reported with
    // balanced indentation calls.
    let stats_bundle = e.new_instance(BUNDLE_CLASS).unwrap();
    let small_key = alloc_string(&mut e, "small".to_string()).unwrap();
    e.invoke_instance(
        stats_bundle,
        "putInt",
        "(Ljava/lang/String;I)V",
        &[Value::Obj(small_key), Value::Int(1)],
    )
    .unwrap();
    let big = e
        .heap
        .alloc_array_zeroed(rd_vm::heap::ElemKind::Int, 2048, "[B")
        .unwrap();
    let big_key = alloc_string(&mut e, "big".to_string()).unwrap();
    e.invoke_instance(
        stats_bundle,
        "putByteArray",
        "(Ljava/lang/String;[B)V",
        &[Value::Obj(big_key), Value::Obj(big)],
    )
    .unwrap();
    let writer = e.new_instance(RECORDING_WRITER).unwrap();
    init_writer(&mut e, writer);
    e.invoke_static(
        BASE_BUNDLE_CLASS,
        "dumpStats",
        "(Lcom/android/internal/util/IndentingPrintWriter;Landroid/os/BaseBundle;)V",
        &[Value::Obj(writer), Value::Obj(stats_bundle)],
    )
    .unwrap();
    assert_eq!(e.heap.get_field(writer, "increases").unwrap(), Value::Int(1));
    assert_eq!(e.heap.get_field(writer, "decreases").unwrap(), Value::Int(1));
    assert_eq!(e.heap.get_field(writer, "indent").unwrap(), Value::Int(0));
    assert_eq!(e.heap.get_field(writer, "lineCount").unwrap(), Value::Int(1));
    let line = text_of(&e, e.heap.get_field(writer, "lastLine").unwrap());
    assert!(line.starts_with("big [size="), "stats line: {line}");
    let reported_size: usize = line
        .trim_start_matches("big [size=")
        .trim_end_matches(']')
        .parse()
        .unwrap();
    assert!(reported_size > 1024, "stats line: {line}");

    // The null-Bundle static form prints AOSP's [null] marker.
    let null_writer = e.new_instance(RECORDING_WRITER).unwrap();
    init_writer(&mut e, null_writer);
    e.invoke_static(
        BASE_BUNDLE_CLASS,
        "dumpStats",
        "(Lcom/android/internal/util/IndentingPrintWriter;Landroid/os/BaseBundle;)V",
        &[Value::Obj(null_writer), Value::Null],
    )
    .unwrap();
    assert_eq!(e.heap.get_field(null_writer, "lineCount").unwrap(), Value::Int(1));
    assert_eq!(
        text_of(&e, e.heap.get_field(null_writer, "lastLine").unwrap()),
        "[null]"
    );
    assert_eq!(e.heap.get_field(null_writer, "indent").unwrap(), Value::Int(0));
}
