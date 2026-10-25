//! Regression tests for issue #300 (`crates/rd-vm/src/jfile.rs` + `android.system.Os`).

use std::sync::Mutex;

use rd_vm::err::VmExit;
use rd_vm::heap::{ElemKind, ObjRef};
use rd_vm::value::Value;
use rd_vm::{Engine, VmConfig};

const ANDROID_OS: &str = "Landroid/system/Os;";
const FILE: &str = "Ljava/io/File;";
const FIS: &str = "Ljava/io/FileInputStream;";
const FOS: &str = "Ljava/io/FileOutputStream;";

static VFS_LOCK: Mutex<()> = Mutex::new(());

fn engine() -> Engine {
    Engine::new(Vec::new(), VmConfig::default())
}

fn jstr(e: &mut Engine, txt: &str) -> Value {
    Value::Obj(e.heap.alloc_string(txt.to_string()).expect("alloc string"))
}

fn byte_buf(e: &mut Engine, data: &[u8]) -> (ObjRef, Value) {
    let arr = e
        .heap
        .alloc_array_zeroed(ElemKind::Int, data.len(), "[B")
        .expect("alloc byte[]");
    let elems = e.heap.array_elems_mut(arr).expect("elems mut");
    for (dst, &b) in elems.iter_mut().zip(data.iter()) {
        *dst = Value::Int(b as i8 as i32);
    }
    (arr, Value::Obj(arr))
}

fn as_obj(v: Value) -> ObjRef {
    match v {
        Value::Obj(r) => r,
        other => panic!("expected Obj, got {other:?}"),
    }
}

fn as_str(e: &Engine, v: Value) -> String {
    e.heap.as_str(as_obj(v)).expect("string").to_string()
}

fn assert_exc(res: Result<Value, VmExit>, expected_class: &str) -> ObjRef {
    match res {
        Err(VmExit::Exception(t)) => {
            assert_eq!(
                t.class, expected_class,
                "unexpected exception {} ({:?})",
                t.class, t.message
            );
            t.obj.unwrap_or(0)
        }
        other => panic!("expected exception {expected_class}, got {other:?}"),
    }
}

fn assert_errno(e: &Engine, res: Result<Value, VmExit>, expected_errno: i32) {
    let exc_obj = assert_exc(res, "Landroid/system/ErrnoException;");
    let errno = e.heap.get_field(exc_obj, "errno").expect("errno field");
    assert_eq!(errno, Value::Int(expected_errno));
}

fn new_java_file(e: &mut Engine, path: &str) -> ObjRef {
    let f = e
        .heap
        .alloc_instance(FILE.to_string(), Vec::new())
        .expect("alloc File");
    let arg = jstr(e, path);
    e.invoke_instance(f, "<init>", "(Ljava/lang/String;)V", &[arg])
        .expect("File.<init>");
    f
}

#[test]
fn issue_300_os_ftruncate_pwrite_and_lseek_write_enforce_efbig_and_enospc() {
    let _guard = VFS_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut e = engine();
    let dir = new_java_file(&mut e, "/t300_quota");
    let _ = e.invoke_instance(dir, "mkdirs", "()Z", &[]);

    let p = jstr(&mut e, "/t300_quota/a.bin");
    // O_RDWR (2) | O_CREAT (0o100 = 64) | O_TRUNC (0o1000 = 512)
    let open_flags = 2 | 0o100 | 0o1000;
    let fd = as_obj(
        e.invoke_static(
            ANDROID_OS,
            "open",
            "(Ljava/lang/String;II)Ljava/io/FileDescriptor;",
            &[p, Value::Int(open_flags), Value::Int(0o600)],
        )
        .expect("Os.open"),
    );

    // 1. Os.ftruncate(fd, 64 MiB + 1 / 4 GiB / Long.MAX_VALUE) -> ErrnoException(EFBIG = 27)
    // Note: put 64 MiB + 1 first so before-fix fails cleanly on assertion instead of 8 EiB host SIGABRT!
    for huge_len in [64 * 1024 * 1024 + 1, 1i64 << 32, i64::MAX] {
        let res = e.invoke_static(
            ANDROID_OS,
            "ftruncate",
            "(Ljava/io/FileDescriptor;J)V",
            &[Value::Obj(fd), Value::Long(huge_len)],
        );
        assert_errno(&e, res, 27);
    }

    // 2. Os.pwrite at Long.MAX_VALUE - 1 -> ErrnoException(EFBIG = 27)
    let (_, one_byte) = byte_buf(&mut e, &[0x42]);
    let res_pwrite = e.invoke_static(
        ANDROID_OS,
        "pwrite",
        "(Ljava/io/FileDescriptor;[BIIJ)I",
        &[
            Value::Obj(fd),
            one_byte.clone(),
            Value::Int(0),
            Value::Int(1),
            Value::Long(i64::MAX - 1),
        ],
    );
    assert_errno(&e, res_pwrite, 27);

    // 3. Os.lseek(fd, 2_000_000_000L, SEEK_SET) + Os.write -> ErrnoException(EFBIG = 27)
    e.invoke_static(
        ANDROID_OS,
        "lseek",
        "(Ljava/io/FileDescriptor;JI)J",
        &[Value::Obj(fd), Value::Long(2_000_000_000), Value::Int(0)],
    )
    .expect("lseek");
    let res_write = e.invoke_static(
        ANDROID_OS,
        "write",
        "(Ljava/io/FileDescriptor;[BII)I",
        &[Value::Obj(fd), one_byte, Value::Int(0), Value::Int(1)],
    );
    assert_errno(&e, res_write, 27);

    // 4. Total VFS quota: 2 x 64 MiB fills 128 MiB budget; 3rd file ftruncate -> ENOSPC (28)
    let max_file: i64 = 64 * 1024 * 1024;
    e.invoke_static(
        ANDROID_OS,
        "ftruncate",
        "(Ljava/io/FileDescriptor;J)V",
        &[Value::Obj(fd), Value::Long(max_file)],
    )
    .expect("first 64 MiB ftruncate");

    let p2 = jstr(&mut e, "/t300_quota/b.bin");
    let fd2 = as_obj(
        e.invoke_static(
            ANDROID_OS,
            "open",
            "(Ljava/lang/String;II)Ljava/io/FileDescriptor;",
            &[p2, Value::Int(open_flags), Value::Int(0o600)],
        )
        .expect("Os.open 2"),
    );
    e.invoke_static(
        ANDROID_OS,
        "ftruncate",
        "(Ljava/io/FileDescriptor;J)V",
        &[Value::Obj(fd2), Value::Long(max_file)],
    )
    .expect("second 64 MiB ftruncate");

    let p3 = jstr(&mut e, "/t300_quota/c.bin");
    let fd3 = as_obj(
        e.invoke_static(
            ANDROID_OS,
            "open",
            "(Ljava/lang/String;II)Ljava/io/FileDescriptor;",
            &[p3, Value::Int(open_flags), Value::Int(0o600)],
        )
        .expect("Os.open 3"),
    );
    let res_nospc = e.invoke_static(
        ANDROID_OS,
        "ftruncate",
        "(Ljava/io/FileDescriptor;J)V",
        &[Value::Obj(fd3), Value::Long(4096)],
    );
    assert_errno(&e, res_nospc, 28);

    for f in [fd, fd2, fd3] {
        let _ = e.invoke_static(
            ANDROID_OS,
            "close",
            "(Ljava/io/FileDescriptor;)V",
            &[Value::Obj(f)],
        );
    }
    for name in [
        "/t300_quota/a.bin",
        "/t300_quota/b.bin",
        "/t300_quota/c.bin",
    ] {
        let arg = jstr(&mut e, name);
        let _ = e.invoke_static(ANDROID_OS, "unlink", "(Ljava/lang/String;)V", &[arg]);
    }
}

#[test]
fn issue_300_fis_read_and_fos_write_reject_null_and_bad_bounds_without_panic_or_corruption() {
    let _guard = VFS_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut e = engine();
    let dir = new_java_file(&mut e, "/t300_io");
    let _ = e.invoke_instance(dir, "mkdirs", "()Z", &[]);

    let fos = e
        .heap
        .alloc_instance(FOS.to_string(), Vec::new())
        .expect("alloc FOS");
    let p_arg = jstr(&mut e, "/t300_io/sample.bin");
    e.invoke_instance(fos, "<init>", "(Ljava/lang/String;)V", &[p_arg])
        .expect("FOS.<init>");

    assert_exc(
        e.invoke_instance(fos, "write", "([B)V", &[Value::Null]),
        "Ljava/lang/NullPointerException;",
    );
    assert_exc(
        e.invoke_instance(
            fos,
            "write",
            "([BII)V",
            &[Value::Null, Value::Int(0), Value::Int(1)],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let (_, payload) = byte_buf(&mut e, b"ABCDEFGH");
    for (off, len) in [(-1, 1), (0, -1), (0, 9), (9, 0), (i32::MAX, 1)] {
        assert_exc(
            e.invoke_instance(
                fos,
                "write",
                "([BII)V",
                &[payload.clone(), Value::Int(off), Value::Int(len)],
            ),
            "Ljava/lang/IndexOutOfBoundsException;",
        );
    }
    e.invoke_instance(fos, "write", "([B)V", &[payload])
        .expect("FOS.write valid");

    let fis = e
        .heap
        .alloc_instance(FIS.to_string(), Vec::new())
        .expect("alloc FIS");
    let p_arg2 = jstr(&mut e, "/t300_io/sample.bin");
    e.invoke_instance(fis, "<init>", "(Ljava/lang/String;)V", &[p_arg2])
        .expect("FIS.<init>");

    // Advance pos to 1 so pos + (-1 as usize) previously wrapped to 0 and panicked f[1..0]!
    assert_eq!(
        e.invoke_instance(fis, "read", "()I", &[]).expect("read 1"),
        Value::Int(b'A' as i32)
    );

    let (buf_ref, buf_val) = byte_buf(&mut e, &[0x7a, 0x7a, 0x7a, 0x7a]);
    assert_exc(
        e.invoke_instance(fis, "read", "([B)I", &[Value::Null]),
        "Ljava/lang/NullPointerException;",
    );
    assert_exc(
        e.invoke_instance(
            fis,
            "read",
            "([BII)I",
            &[Value::Null, Value::Int(0), Value::Int(1)],
        ),
        "Ljava/lang/NullPointerException;",
    );

    for (off, len) in [(0, -1), (-1, 2), (0, 100), (5, 0), (i32::MAX, 1)] {
        assert_exc(
            e.invoke_instance(
                fis,
                "read",
                "([BII)I",
                &[buf_val.clone(), Value::Int(off), Value::Int(len)],
            ),
            "Ljava/lang/IndexOutOfBoundsException;",
        );
    }
    let elems = e.heap.as_array_elems(buf_ref).expect("elems").0;
    assert_eq!(elems[0], Value::Int(0x7a));

    assert_eq!(
        e.invoke_instance(
            fis,
            "read",
            "([BII)I",
            &[buf_val, Value::Int(0), Value::Int(3)],
        )
        .expect("valid read"),
        Value::Int(3)
    );
    let elems_after = e.heap.as_array_elems(buf_ref).expect("elems").0;
    assert_eq!(elems_after[0], Value::Int(b'B' as i32));
    assert_eq!(elems_after[1], Value::Int(b'C' as i32));
    assert_eq!(elems_after[2], Value::Int(b'D' as i32));
    assert_eq!(elems_after[3], Value::Int(0x7a));
}

#[test]
fn issue_300_absolute_normalization_realpath_empty_file_npe_and_rename_subtree() {
    let _guard = VFS_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut e = engine();

    // 1. Canonicalization of `/./` and `/../` across File and Os + Os.realpath
    let files_dir = new_java_file(&mut e, "/t300_paths/pkg/files");
    e.invoke_instance(files_dir, "mkdirs", "()Z", &[])
        .expect("mkdirs");
    let cache_dir = new_java_file(&mut e, "/t300_paths/pkg/cache");
    e.invoke_instance(cache_dir, "mkdirs", "()Z", &[])
        .expect("mkdirs cache");

    let f_hello = new_java_file(&mut e, "/t300_paths/pkg/files/a.txt");
    assert_eq!(
        e.invoke_instance(f_hello, "createNewFile", "()Z", &[])
            .expect("createNewFile"),
        Value::Int(1)
    );

    let unnorm = jstr(&mut e, "/t300_paths/pkg/cache/../files/./a.txt");
    let fd = as_obj(
        e.invoke_static(
            ANDROID_OS,
            "open",
            "(Ljava/lang/String;II)Ljava/io/FileDescriptor;",
            &[unnorm, Value::Int(0), Value::Int(0)],
        )
        .expect("Os.open unnormalized path"),
    );
    e.invoke_static(
        ANDROID_OS,
        "close",
        "(Ljava/io/FileDescriptor;)V",
        &[Value::Obj(fd)],
    )
    .expect("Os.close");

    let rp_arg = jstr(&mut e, "/t300_paths/../t300_paths/pkg/cache/../files");
    let rp = e
        .invoke_static(
            ANDROID_OS,
            "realpath",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[rp_arg],
        )
        .expect("Os.realpath");
    assert_eq!(as_str(&e, rp), "/t300_paths/pkg/files");

    // 2. Relative leading `..` preserved in File.getPath()
    let rel = new_java_file(&mut e, "../a");
    let rel_path = e
        .invoke_instance(rel, "getPath", "()Ljava/lang/String;", &[])
        .expect("getPath");
    assert_eq!(as_str(&e, rel_path), "../a");

    // 3. `new File("")` does NOT alias root `/`
    let empty_f = new_java_file(&mut e, "");
    assert_eq!(
        e.invoke_instance(empty_f, "exists", "()Z", &[])
            .expect("exists"),
        Value::Int(0)
    );
    assert_eq!(
        e.invoke_instance(empty_f, "isDirectory", "()Z", &[])
            .expect("isDirectory"),
        Value::Int(0)
    );
    assert_eq!(
        e.invoke_instance(empty_f, "mkdirs", "()Z", &[])
            .expect("mkdirs"),
        Value::Int(0)
    );
    assert_exc(
        e.invoke_instance(empty_f, "createNewFile", "()Z", &[]),
        "Ljava/io/IOException;",
    );
    let empty_os_arg = jstr(&mut e, "");
    let empty_stat = e.invoke_static(
        ANDROID_OS,
        "stat",
        "(Ljava/lang/String;)Landroid/system/StructStat;",
        &[empty_os_arg],
    );
    assert_errno(&e, empty_stat, 2);

    // 4. NullPointerException on null constructor / compareTo / renameTo / createTempFile args
    let raw_f = e
        .heap
        .alloc_instance(FILE.to_string(), Vec::new())
        .expect("alloc File");
    assert_exc(
        e.invoke_instance(raw_f, "<init>", "(Ljava/lang/String;)V", &[Value::Null]),
        "Ljava/lang/NullPointerException;",
    );
    assert_exc(
        e.invoke_instance(
            raw_f,
            "<init>",
            "(Ljava/io/File;Ljava/lang/String;)V",
            &[Value::Obj(files_dir), Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_exc(
        e.invoke_instance(f_hello, "compareTo", "(Ljava/io/File;)I", &[Value::Null]),
        "Ljava/lang/NullPointerException;",
    );
    assert_exc(
        e.invoke_instance(f_hello, "renameTo", "(Ljava/io/File;)Z", &[Value::Null]),
        "Ljava/lang/NullPointerException;",
    );
    let tmp_suf = jstr(&mut e, ".tmp");
    assert_exc(
        e.invoke_static(
            FILE,
            "createTempFile",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/io/File;",
            &[Value::Null, tmp_suf],
        ),
        "Ljava/lang/NullPointerException;",
    );

    // 5. File.renameTo moves entire directory subtrees atomically
    let tree_sub = new_java_file(&mut e, "/t300_paths/pkg/src_dir/nested");
    assert_eq!(
        e.invoke_instance(tree_sub, "mkdirs", "()Z", &[])
            .expect("mkdirs subtree"),
        Value::Int(1)
    );
    let leaf = new_java_file(&mut e, "/t300_paths/pkg/src_dir/nested/leaf.txt");
    assert_eq!(
        e.invoke_instance(leaf, "createNewFile", "()Z", &[])
            .expect("create leaf"),
        Value::Int(1)
    );
    let src_dir = new_java_file(&mut e, "/t300_paths/pkg/src_dir");
    let dst_dir = new_java_file(&mut e, "/t300_paths/pkg/dst_dir");
    assert_eq!(
        e.invoke_instance(
            src_dir,
            "renameTo",
            "(Ljava/io/File;)Z",
            &[Value::Obj(dst_dir)],
        )
        .expect("renameTo dir"),
        Value::Int(1)
    );
    let moved_leaf = new_java_file(&mut e, "/t300_paths/pkg/dst_dir/nested/leaf.txt");
    assert_eq!(
        e.invoke_instance(moved_leaf, "isFile", "()Z", &[])
            .expect("moved leaf isFile"),
        Value::Int(1)
    );
    assert_eq!(
        e.invoke_instance(leaf, "exists", "()Z", &[])
            .expect("old leaf gone"),
        Value::Int(0)
    );
}
