//! Real std contracts. Passing these does not prove Android API compatibility.
use std::cell::Cell;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Barrier, Condvar, Mutex, OnceLock, RwLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
const ROOT: &str = "sdmc:/switch/androswitch-std-probe";
type Outcome = Result<String, String>;
type Probe = (&'static str, fn(&Path) -> Outcome);
static DTORS: AtomicUsize = AtomicUsize::new(0);
struct Sentinel;
impl Drop for Sentinel {
    fn drop(&mut self) {
        DTORS.fetch_add(1, Ordering::SeqCst);
    }
}
thread_local! {
    static LOCAL: Cell<usize> = const { Cell::new(0) };
    static SENTINEL: Sentinel = const { Sentinel };
}
fn require(condition: bool, detail: impl Into<String>) -> Outcome {
    if condition {
        Ok(detail.into())
    } else {
        Err(detail.into())
    }
}
fn io(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[allow(unsafe_code)]
mod bridge {
    use super::*;
    unsafe extern "C" {
        static as_sdk_abi: [u64; 33];
        fn as_std_c_exchange(path: *const std::ffi::c_char) -> i32;
        fn as_std_detach_result() -> i32;
        fn as_std_reaper_drain(timeout_ms: u32) -> i32;
        fn as_std_reaped_threads() -> u32;
        fn as_std_live_threads() -> u32;
        fn as_std_process_core_count() -> i64;
    }
    pub fn abi() -> Outcome {
        use std::mem::{align_of, offset_of, size_of};
        let rust = [
            size_of::<libc::dev_t>() as u64,
            size_of::<libc::ino_t>() as u64,
            size_of::<libc::uid_t>() as u64,
            size_of::<libc::gid_t>() as u64,
            size_of::<libc::blksize_t>() as u64,
            size_of::<libc::blkcnt_t>() as u64,
            size_of::<libc::clockid_t>() as u64,
            size_of::<libc::stat>() as u64,
            align_of::<libc::stat>() as u64,
            offset_of!(libc::stat, st_size) as u64,
            offset_of!(libc::stat, st_atim) as u64,
            offset_of!(libc::stat, st_blksize) as u64,
            size_of::<libc::dirent>() as u64,
            offset_of!(libc::dirent, d_name) as u64,
            libc::CLOCK_REALTIME as u64,
            libc::CLOCK_MONOTONIC as u64,
            libc::O_CREAT as u64,
            libc::O_EXCL as u64,
            libc::O_TRUNC as u64,
            libc::O_APPEND as u64,
            libc::AT_FDCWD as u64,
            size_of::<libc::pthread_attr_t>() as u64,
            align_of::<libc::pthread_attr_t>() as u64,
            size_of::<libc::pthread_mutex_t>() as u64,
            align_of::<libc::pthread_mutex_t>() as u64,
            size_of::<libc::pthread_mutexattr_t>() as u64,
            align_of::<libc::pthread_mutexattr_t>() as u64,
            size_of::<libc::pthread_cond_t>() as u64,
            align_of::<libc::pthread_cond_t>() as u64,
            size_of::<libc::pthread_condattr_t>() as u64,
            align_of::<libc::pthread_condattr_t>() as u64,
            size_of::<libc::pthread_t>() as u64,
            align_of::<libc::pthread_t>() as u64,
        ];
        // SAFETY: immutable fixed-size table exported by the C SDK probe.
        let sdk = unsafe { as_sdk_abi };
        require(rust == sdk, format!("rust={rust:?}; sdk={sdk:?}"))
    }
    pub fn exchange(path: &Path) -> Outcome {
        let name = std::ffi::CString::new(path.to_str().ok_or("non-UTF8 path")?).map_err(io)?;
        // SAFETY: valid NUL-terminated path borrowed only during this call.
        let result = unsafe { as_std_c_exchange(name.as_ptr()) };
        require(result == 0, format!("C exchange return={result}"))
    }
    pub fn detach_result() -> (i32, i32) {
        // SAFETY: the PAL drains its real detached-thread resource reaper.
        unsafe { (as_std_detach_result(), as_std_reaper_drain(5000)) }
    }
    pub fn thread_counts() -> (u32, u32) {
        // SAFETY: bounded counters, no borrowed pointers.
        unsafe { (as_std_reaped_threads(), as_std_live_threads()) }
    }
    pub fn core_count() -> i64 {
        // SAFETY: direct current-process kernel query with no arguments.
        unsafe { as_std_process_core_count() }
    }
}
fn journal(root: &Path, id: &str, result: &Outcome) -> Result<(), String> {
    let (status, detail) = match result {
        Ok(s) => ("pass", s),
        Err(s) => ("fail", s),
    };
    let detail = detail.replace(['\t', '\r', '\n'], " ");
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("results.tsv"))
        .map_err(io)?;
    writeln!(f, "{id}\t{status}\t{detail}").map_err(io)
}
const CASES: &[Probe] = &[
    ("abi.sdk_layouts", |_| bridge::abi()),
    ("alloc.vec_string", |_| {
        let mut v: Vec<u64> = (0..8192).collect();
        v.reverse();
        let s = format!("{}:{}", "Horizon🦀", v.iter().sum::<u64>());
        require(s == "Horizon🦀:33550336", s)
    }),
    ("alloc.aligned", |_| {
        #[repr(align(4096))]
        struct Page([u8; 4096]);
        let p = Box::new(Page([37; 4096]));
        require(
            (&*p as *const Page as usize) % 4096 == 0 && p.0.iter().all(|b| *b == 37),
            "4096-byte aligned Box",
        )
    }),
    ("hashmap.entropy", |_| {
        let mut m = HashMap::new();
        for i in 0..1024 {
            m.insert(format!("key-{i}"), i * i);
        }
        require(
            m.get("key-1000") == Some(&1_000_000),
            "1024 randomized HashMap entries",
        )
    }),
    ("io.write_read", |r| {
        let b: Vec<u8> = (0..131_072).map(|n| (n % 251) as u8).collect();
        fs::write(r.join("bytes.bin"), &b).map_err(io)?;
        require(
            fs::read(r.join("bytes.bin")).map_err(io)? == b,
            "128 KiB binary round trip",
        )
    }),
    ("io.seek_metadata", |r| {
        let mut f = File::open(r.join("bytes.bin")).map_err(io)?;
        f.seek(SeekFrom::Start(65_537)).map_err(io)?;
        let mut b = [0u8; 17];
        f.read_exact(&mut b).map_err(io)?;
        require(
            f.metadata().map_err(io)?.len() == 131_072 && b[0] == (65_537 % 251) as u8,
            "seek and native stat",
        )
    }),
    ("io.c_shared_file", |r| {
        let p = r.join("shared.bin");
        fs::write(&p, b"Rust").map_err(io)?;
        bridge::exchange(&p)?;
        require(
            fs::read(p).map_err(io)? == b"Rust+C",
            "Rust -> C -> Rust shared bytes",
        )
    }),
    ("io.rename", |r| {
        fs::write(r.join("a.txt"), b"persistent").map_err(io)?;
        fs::rename(r.join("a.txt"), r.join("b.txt")).map_err(io)?;
        require(
            !r.join("a.txt").exists() && fs::read(r.join("b.txt")).map_err(io)? == b"persistent",
            "renamed with exact contents",
        )
    }),
    ("io.copy", |r| {
        let n = fs::copy(r.join("b.txt"), r.join("c.txt")).map_err(io)?;
        let file = File::open(r.join("c.txt")).map_err(io)?;
        let permission_error = file
            .set_permissions(file.metadata().map_err(io)?.permissions())
            .expect_err("SD chmod must remain unsupported");
        require(
            n == 10
                && fs::read(r.join("c.txt")).map_err(io)? == b"persistent"
                && permission_error.kind() == std::io::ErrorKind::Unsupported,
            format!(
                "copy contents; explicit chmod stays {:?}",
                permission_error.kind()
            ),
        )
    }),
    ("io.directory", |r| {
        let names: Result<Vec<_>, _> = fs::read_dir(r)
            .map_err(io)?
            .map(|e| e.map(|e| e.file_name()))
            .collect();
        require(
            names.map_err(io)?.iter().any(|n| n == "bytes.bin"),
            "native dirent decoded",
        )
    }),
    ("io.not_found", |r| {
        match File::open(r.join("absent-file")) {
            Err(e) => require(
                e.kind() == std::io::ErrorKind::NotFound,
                format!("kind={:?}, errno={:?}", e.kind(), e.raw_os_error()),
            ),
            Ok(_) => Err("missing file opened".into()),
        }
    }),
    ("io.exclusive_create", |r| {
        let p = r.join("exclusive.txt");
        File::create(&p).map_err(io)?;
        match OpenOptions::new().write(true).create_new(true).open(p) {
            Err(e) => require(
                e.kind() == std::io::ErrorKind::AlreadyExists,
                format!("kind={:?}", e.kind()),
            ),
            Ok(_) => Err("exclusive create overwrote file".into()),
        }
    }),
    ("io.sync_all", |r| {
        let mut f = File::create(r.join("synced.txt")).map_err(io)?;
        f.write_all(b"synced").map_err(io)?;
        f.sync_all().map_err(io)?;
        Ok("actual fsync succeeded".into())
    }),
    ("time.monotonic_sleep", |_| {
        let t = Instant::now();
        thread::sleep(Duration::from_millis(40));
        let ms = t.elapsed().as_millis();
        require((20..10_000).contains(&ms), format!("elapsed_ms={ms}"))
    }),
    ("time.system_clock", |_| {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io)?
            .as_secs();
        require(n > 1_577_836_800, format!("epoch_seconds={n}"))
    }),
    ("thread.spawn_join", |_| {
        let main = thread::current().id();
        let w = thread::Builder::new()
            .name("std-proof".into())
            .spawn(move || (thread::current().id() != main, 6 * 7))
            .map_err(io)?;
        require(
            w.join().map_err(|_| "worker panic")? == (true, 42),
            "distinct real thread returns 42",
        )
    }),
    ("thread.tls_destructors", |_| {
        DTORS.store(0, Ordering::SeqCst);
        LOCAL.set(99);
        let mut jobs = Vec::new();
        for i in 1..=8 {
            jobs.push(
                thread::Builder::new()
                    .spawn(move || {
                        let initial = LOCAL.get();
                        LOCAL.set(i);
                        SENTINEL.with(|_| ());
                        thread::sleep(Duration::from_millis(2));
                        (initial, LOCAL.get())
                    })
                    .map_err(io)?,
            );
        }
        let mut valid = true;
        for (i, w) in jobs.into_iter().enumerate() {
            valid &= w.join().map_err(|_| "TLS panic")? == (0, i + 1);
        }
        let drops = DTORS.load(Ordering::SeqCst);
        require(
            valid && LOCAL.get() == 99 && drops == 8,
            format!("isolated={valid}; main={}; drops={drops}", LOCAL.get()),
        )
    }),
    ("thread.repeated_join_tls", |_| {
        let before = DTORS.load(Ordering::SeqCst);
        let mut valid = true;
        for _ in 0..8 {
            let mut jobs = Vec::new();
            for i in 1..=8 {
                jobs.push(
                    thread::Builder::new()
                        .spawn(move || {
                            let initial = LOCAL.get();
                            LOCAL.set(i);
                            SENTINEL.with(|_| ());
                            thread::yield_now();
                            (initial, LOCAL.get())
                        })
                        .map_err(io)?,
                );
            }
            for (i, w) in jobs.into_iter().enumerate() {
                valid &= w.join().map_err(|_| "repeated TLS panic")? == (0, i + 1);
            }
        }
        let drops = DTORS.load(Ordering::SeqCst) - before;
        require(
            valid && drops == 64,
            format!("workers=64; isolated={valid}; destructors={drops}"),
        )
    }),
    ("thread.detached_handle", |_| {
        let before = bridge::thread_counts().0;
        let dtor_before = DTORS.load(Ordering::SeqCst);
        let done = Arc::new(AtomicUsize::new(0));
        for _ in 0..4 {
            for _ in 0..32 {
                let done = done.clone();
                let w = thread::Builder::new()
                    .spawn(move || {
                        SENTINEL.with(|_| ());
                        thread::sleep(Duration::from_millis(2));
                        done.fetch_add(1, Ordering::Release);
                    })
                    .map_err(io)?;
                drop(w);
            }
            let (detach, drain) = bridge::detach_result();
            if detach != 0 || drain != 0 {
                return Err(format!("detach={detach}; drain={drain}"));
            }
        }
        let (after, live) = bridge::thread_counts();
        let drops = DTORS.load(Ordering::SeqCst) - dtor_before;
        require(
            after - before == 128
                && live == 0
                && drops == 128
                && done.load(Ordering::Acquire) == 128,
            format!(
                "detached=128; reclaimed={}; live={live}; destructors={drops}",
                after - before
            ),
        )
    }),
    ("sync.mutex_contention", |_| {
        let n = Arc::new(Mutex::new(0));
        let b = Arc::new(Barrier::new(5));
        let mut jobs = Vec::new();
        for _ in 0..4 {
            let n = n.clone();
            let b = b.clone();
            jobs.push(
                thread::Builder::new()
                    .spawn(move || {
                        b.wait();
                        for _ in 0..2000 {
                            *n.lock().unwrap() += 1;
                        }
                    })
                    .map_err(io)?,
            );
        }
        b.wait();
        for w in jobs {
            w.join().map_err(|_| "mutex panic")?;
        }
        let actual = *n.lock().map_err(io)?;
        require(actual == 8000, format!("counter={actual}"))
    }),
    ("sync.condvar_notify", |_| {
        let pair = Arc::new((Mutex::new(false), Condvar::new()));
        let other = pair.clone();
        let w = thread::Builder::new()
            .spawn(move || {
                thread::sleep(Duration::from_millis(20));
                let (m, c) = &*other;
                *m.lock().unwrap() = true;
                c.notify_all();
            })
            .map_err(io)?;
        let (m, c) = &*pair;
        let (g, t) = c
            .wait_timeout_while(m.lock().map_err(io)?, Duration::from_secs(3), |v| !*v)
            .map_err(io)?;
        let ok = *g && !t.timed_out();
        drop(g);
        w.join().map_err(|_| "condvar panic")?;
        require(ok, "notification before deadline")
    }),
    ("sync.condvar_timeout", |_| {
        let m = Mutex::new(false);
        let c = Condvar::new();
        let start = Instant::now();
        let (g, t) = c
            .wait_timeout_while(m.lock().map_err(io)?, Duration::from_millis(40), |v| !*v)
            .map_err(io)?;
        drop(g);
        let ms = start.elapsed().as_millis();
        require(
            t.timed_out() && (20..10_000).contains(&ms),
            format!("timeout={}; elapsed_ms={ms}", t.timed_out()),
        )
    }),
    ("sync.rwlock_once", |_| {
        let s = Arc::new((RwLock::new(0), OnceLock::new()));
        let mut jobs = Vec::new();
        for _ in 0..4 {
            let s = s.clone();
            jobs.push(
                thread::Builder::new()
                    .spawn(move || {
                        let fixed = *s.1.get_or_init(|| 41);
                        *s.0.write().unwrap() += 1;
                        fixed
                    })
                    .map_err(io)?,
            );
        }
        for w in jobs {
            if w.join().map_err(|_| "rwlock panic")? != 41 {
                return Err("OnceLock mismatch".into());
            }
        }
        let n = *s.0.read().map_err(io)?;
        require(n == 4, "RwLock and shared OnceLock")
    }),
    ("sync.channel_timeout", |_| {
        let (tx, rx) = mpsc::channel();
        let w = thread::Builder::new()
            .spawn(move || {
                thread::sleep(Duration::from_millis(20));
                tx.send(42).unwrap();
            })
            .map_err(io)?;
        let n = rx.recv_timeout(Duration::from_secs(3)).map_err(io)?;
        w.join().map_err(|_| "channel panic")?;
        require(n == 42, "cross-thread channel message")
    }),
    ("sync.park_unpark", |_| {
        let w = thread::Builder::new()
            .spawn(|| {
                let t = Instant::now();
                thread::park_timeout(Duration::from_secs(3));
                t.elapsed().as_millis()
            })
            .map_err(io)?;
        thread::sleep(Duration::from_millis(20));
        w.thread().unpark();
        let ms = w.join().map_err(|_| "park panic")?;
        require(ms < 2500, format!("park_ms={ms}"))
    }),
    ("env.set_get_remove", |_| {
        std::env::set_var("ANDROSWITCH_STD_PROBE", "42");
        let v = std::env::var("ANDROSWITCH_STD_PROBE").map_err(io)?;
        std::env::remove_var("ANDROSWITCH_STD_PROBE");
        require(
            v == "42" && std::env::var("ANDROSWITCH_STD_PROBE").is_err(),
            "process environment",
        )
    }),
    ("thread.available_parallelism", |_| {
        let n = thread::available_parallelism().map_err(io)?.get();
        let kernel = bridge::core_count();
        require(
            kernel > 0 && n as i64 == kernel,
            format!("reported={n}; kernel_process_mask_count={kernel}"),
        )
    }),
    (
        "process.unsupported",
        |_| match std::process::Command::new("androswitch-nonexistent-child").status() {
            Err(e) => require(
                e.kind() == std::io::ErrorKind::Unsupported,
                format!("kind={:?}; {e}", e.kind()),
            ),
            Ok(status) => Err(format!("unexpected child: {status}")),
        },
    ),
];
fn execute() -> Result<i32, String> {
    let base = Path::new(ROOT);
    let request = fs::read_to_string(base.join("request.txt")).map_err(io)?;
    let mut lines = request.lines();
    let session = lines.next().ok_or("session missing")?;
    let mode = lines.next().ok_or("mode missing")?;
    if session.is_empty()
        || !session
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("invalid session".into());
    }
    let root = base.join(session);
    fs::create_dir_all(&root).map_err(io)?;
    fs::write(root.join("started.txt"), format!("{session}\n{mode}\n")).map_err(io)?;
    {
        let root = root.clone();
        let file = if mode == "panic" {
            "panic.txt"
        } else {
            "unexpected-panic.txt"
        };
        std::panic::set_hook(Box::new(move |info| {
            let _ = fs::write(root.join(file), info.to_string());
        }));
    }
    if mode == "panic" {
        panic!("intentional std probe panic: abort policy");
    }
    let mut failures = 0;
    if mode == "restart" {
        let v = fs::read_to_string(base.join("persist.txt")).map_err(io)?;
        let result = require(v == format!("{session}\n"), format!("persisted={v:?}"));
        failures += i32::from(result.is_err());
        journal(&root, "io.fresh_process_persistence", &result)?;
    } else if mode == "suite" {
        for (id, case) in CASES {
            fs::write(root.join("active-case.txt"), id).map_err(io)?;
            let result = case(&root);
            failures += i32::from(result.is_err());
            journal(&root, id, &result)?;
            if *id == "abi.sdk_layouts" && result.is_err() {
                return Err("ABI mismatch; remaining cases refused".into());
            }
        }
        fs::write(base.join("persist.txt"), format!("{session}\n")).map_err(io)?;
    } else {
        return Err("unknown mode".into());
    }
    fs::write(
        root.join(format!("complete-{mode}.txt")),
        format!("{session}\n{mode}\n{failures}\n"),
    )
    .map_err(io)?;
    Ok(failures)
}
/// Called once from initialized libnx C main; no pointers cross this boundary.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn as_std_probe_run() -> i32 {
    match execute() {
        Ok(n) => n,
        Err(e) => {
            let _ = fs::write(Path::new(ROOT).join("fatal.txt"), &e);
            eprintln!("std probe: {e}");
            100
        }
    }
}
