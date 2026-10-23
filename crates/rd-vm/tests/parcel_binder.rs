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
