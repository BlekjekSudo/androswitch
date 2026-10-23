use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-env-changed=CC");
    println!("cargo:rerun-if-env-changed=AR");
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let cache = root.join("local-artifacts/dependencies/sqlite");
    let lock: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("third_party/sqlite.lock.json")).unwrap())
            .unwrap();
    for name in ["sqlite3.c", "sqlite3.h"] {
        let path = cache.join(name);
        let bytes =
            fs::read(&path).expect("run python scripts/fetch-sqlite.py before building SQLite");
        let actual = format!("{:x}", Sha256::digest(&bytes));
        assert_eq!(
            actual,
            lock["source_sha256"][name]
                .as_str()
                .expect("pinned source digest")
        );
        println!("cargo:rerun-if-changed={}", path.display());
    }
    println!(
        "cargo:rerun-if-changed={}",
        root.join("third_party/sqlite.lock.json").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        root.join("platform/sqlite/memory_vfs.c").display()
    );
    if env::var("TARGET").unwrap().contains("nintendo-switch") {
        // Switch objects are compiled/linked with libnx by the product Makefile.
        return;
    }
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let windows = env::var("TARGET").unwrap().contains("windows");
    let cc = env::var("CC").unwrap_or_else(|_| {
        if windows {
            "C:/w64devkit/bin/gcc.exe".into()
        } else {
            "cc".into()
        }
    });
    let ar = env::var("AR").unwrap_or_else(|_| {
        if windows {
            "C:/w64devkit/bin/ar.exe".into()
        } else {
            "ar".into()
        }
    });
    let mut objects = Vec::new();
    for (source, name) in [
        (cache.join("sqlite3.c"), "sqlite3"),
        (root.join("platform/sqlite/memory_vfs.c"), "memory_vfs"),
    ] {
        let object = out.join(format!("{name}.o"));
        let status = Command::new(&cc)
            .args([
                "-c",
                "-O2",
                "-std=c11",
                "-DSQLITE_OS_OTHER=1",
                "-DSQLITE_THREADSAFE=0",
                "-DSQLITE_TEMP_STORE=3",
                "-DSQLITE_OMIT_LOAD_EXTENSION",
                "-DSQLITE_OMIT_WAL",
                "-DSQLITE_DEFAULT_MEMSTATUS=1",
                "-DSQLITE_DEFAULT_CACHE_SIZE=-1024",
            ])
            .arg(format!("-I{}", cache.display()))
            .arg(source)
            .arg("-o")
            .arg(&object)
            .status()
            .expect("C compiler");
        assert!(status.success(), "SQLite compilation failed");
        objects.push(object);
    }
    let status = Command::new(ar)
        .arg("crs")
        .arg(out.join("libas_sqlite.a"))
        .args(objects)
        .status()
        .expect("archive tool");
    assert!(status.success());
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=as_sqlite");
    if windows {
        println!("cargo:rustc-link-lib=bcrypt");
    }
}
