//! Real SQLite for `android.database.sqlite`.
//!
//! **Backend.** Each Engine owns one child process running Python's
//! `sqlite3` module (real SQLite, as on Android), driven over a JSON-lines
//! pipe. This follows the subprocess-isolation pattern of ADR 004: no C
//! toolchain and no `unsafe` in RustyDroid. The process is spawned lazily on
//! the first database open and killed when the Engine drops.
//! - Interpreter: `RD_SQLITE_PYTHON`, else `python3`.
//! - `RD_SQLITE=subset` forces the legacy in-memory SQL subset (m8).
//! - If the interpreter cannot be started, m8's subset is used.
//!
//! **API surface:**
//! - `SQLiteOpenHelper`: the AOSP open protocol, run on the helper's DEX
//!   overrides. `onConfigure`, then `PRAGMA user_version` against the
//!   requested version, then `onCreate`/`onUpgrade`/`onDowngrade` in a
//!   transaction, then `onOpen`. The database object is cached per helper.
//! - `SQLiteDatabase`: `execSQL` (with bind args), `rawQuery`,
//!   `rawQueryWithFactory`, the `query` builders, `insert*`/`update*`/
//!   `delete`/`replace` from ContentValues, `compileStatement`, nested
//!   transactions, version, foreign keys, and the static `openDatabase`/
//!   `openOrCreateDatabase`/`create`.
//! - `rawQueryWithFactory` with a factory: Room's path. The query is an
//!   `SQLiteQuery` program; the DEX factory's `newCursor(db, driver,
//!   editTable, query)` binds it (`SQLiteProgram.bind*`) and constructs
//!   `new SQLiteCursor(driver, editTable, query)`, which executes it.
//! - `SQLiteStatement`/`SQLiteProgram`: `bind*`, `clearBindings`,
//!   `execute`, `executeInsert`, `executeUpdateDelete`,
//!   `simpleQueryForLong`/`String`.
//! - `Cursor`: the full row-cursor surface with AOSP CursorWindow
//!   conversions (`getString` on an int, `getLong` on text, …).
//!
//! **Persistence:** named databases are serialized to their app-private
//! VFS path (`jfile`) after committed mutations and deserialized on the next
//! Engine launch that opens the same path. `:memory:`/`createInMemory`
//! databases intentionally remain memory-only. The VFS image is a real
//! SQLite file image when Python's `sqlite3` serialize/deserialize API is
//! available; older interpreters fall back to a SQL dump image.
//!
//! **Deviations:**
//! - SQL errors map to `SQLiteException` subclasses by SQLite error class,
//!   not exact result code.
//! - This is still not literal AOSP completeness: WAL/checkpointing, locale
//!   collation, custom functions/extensions, multi-process locking and every
//!   overload/result-code nuance remain Appendix C gaps.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value as J};

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::{ElemKind, ObjRef};
use crate::value::Value;

const DB: &str = "Landroid/database/sqlite/SQLiteDatabase;";
const HELPER: &str = "Landroid/database/sqlite/SQLiteOpenHelper;";
const STMT: &str = "Landroid/database/sqlite/SQLiteStatement;";
const QUERY: &str = "Landroid/database/sqlite/SQLiteQuery;";
const PROGRAM: &str = "Landroid/database/sqlite/SQLiteProgram;";
const CLOSABLE: &str = "Landroid/database/sqlite/SQLiteClosable;";
const SQL_CURSOR: &str = "Landroid/database/sqlite/SQLiteCursor;";
const DRIVER: &str = "Landroid/database/sqlite/SQLiteDirectCursorDriver;";

const PY: &str = r#"
import sys, json, sqlite3, base64, math
dbs = {}
DUMP_PREFIX = b"-- RustyDroid SQLite dump\n"
def enc(v):
    if isinstance(v, (bytes, bytearray, memoryview)):
        return {"b": base64.b64encode(bytes(v)).decode()}
    if isinstance(v, float) and not math.isfinite(v):
        if math.isnan(v):
            return {"f": "nan"}
        return {"f": "inf" if v > 0 else "-inf"}
    return v
def dec(v):
    if isinstance(v, dict):
        if "b" in v:
            return base64.b64decode(v["b"])
        if "f" in v:
            return float(v["f"])
    return v
def load_db(db, data):
    if not data:
        return
    if data.startswith(DUMP_PREFIX):
        db.executescript(data[len(DUMP_PREFIX):].decode())
    elif hasattr(db, "deserialize"):
        db.deserialize(data)
    else:
        raise sqlite3.DatabaseError("sqlite3.deserialize is unavailable")
def dump_db(db):
    if hasattr(db, "serialize"):
        return db.serialize()
    return DUMP_PREFIX + ("\n".join(db.iterdump())).encode()
for line in sys.stdin:
    r = json.loads(line)
    try:
        name = r["db"]
        db = dbs.get(name)
        if db is None:
            db = sqlite3.connect(":memory:", isolation_level=None, check_same_thread=False)
            dbs[name] = db
        op = r["op"]
        if op == "close":
            db.close(); del dbs[name]; out = {}
        elif op == "load":
            load_db(db, base64.b64decode(r.get("data", ""))); out = {}
        elif op == "dump":
            out = {"data": base64.b64encode(dump_db(db)).decode()}
        else:
            cur = db.execute(r["sql"], [dec(a) for a in r.get("args", [])])
            cols = [d[0] for d in cur.description] if cur.description else []
            rows = [[enc(v) for v in row] for row in cur.fetchall()] if cur.description else []
            ch, last = db.execute("select changes(), last_insert_rowid()").fetchone()
            out = {"cols": cols, "rows": rows, "changes": ch, "last": last}
    except sqlite3.IntegrityError as e:
        out = {"err": str(e), "kind": "constraint"}
    except Exception as e:
        out = {"err": str(e), "kind": type(e).__name__}
    sys.stdout.write(json.dumps(out, allow_nan=False) + "\n"); sys.stdout.flush()
"#;

/// A SQL value as SQLite stores it.
#[derive(Debug, Clone, PartialEq)]
pub enum Sv {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

struct Proc {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Default)]
struct Txn {
    /// One entry per nesting level: `setTransactionSuccessful` was called.
    levels: Vec<bool>,
    /// An inner level ended without success, so the outer commit rolls back.
    failed: bool,
}

#[derive(Default)]
struct Cursor {
    cols: Vec<String>,
    rows: Vec<Vec<Sv>>,
    pos: i64,
    closed: bool,
}

/// Per-Engine SQLite state (lives in `M8State`).
#[derive(Default)]
pub struct SqlState {
    proc: Option<Proc>,
    /// The backend failed to start: use the m8 subset.
    pub(crate) unavailable: bool,
    dbs: HashMap<ObjRef, String>,
    /// Persistent DB paths already deserialized into this Engine's backend
    /// process. A new Engine has an empty set, so reopening a named DB loads
    /// the VFS image again (the cross-launch persistence boundary).
    loaded: HashSet<String>,
    txns: HashMap<String, Txn>,
    programs: HashMap<ObjRef, (String, String, Vec<Sv>)>, // obj -> (db, sql, binds)
    cursors: HashMap<ObjRef, Cursor>,
}

impl std::fmt::Debug for SqlState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqlState")
            .field("dbs", &self.dbs.len())
            .field("programs", &self.programs.len())
            .field("cursors", &self.cursors.len())
            .finish()
    }
}

impl SqlState {
    /// Purge dead heap `ObjRef` keys after mark-sweep GC (#257).
    pub fn gc_purge(&mut self, mut alive: impl FnMut(ObjRef) -> bool) {
        self.dbs.retain(|&r, _| alive(r));
        self.programs.retain(|&r, _| alive(r));
        self.cursors.retain(|&r, _| alive(r));
    }

    fn clear_slot(&mut self, r: ObjRef) {
        self.dbs.remove(&r);
        self.programs.remove(&r);
        self.cursors.remove(&r);
    }
}

struct Res {
    cols: Vec<String>,
    rows: Vec<Vec<Sv>>,
    changes: i64,
    last: i64,
}

fn backend_ready(vm: &mut Engine) -> bool {
    let st = &mut vm.fw.m8.sqlx;
    if st.proc.is_some() {
        return true;
    }
    if st.unavailable
        || std::env::var("RD_SQLITE")
            .map(|v| v == "subset")
            .unwrap_or(false)
    {
        return false;
    }
    let py = std::env::var("RD_SQLITE_PYTHON").unwrap_or_else(|_| "python3".into());
    let spawned = Command::new(py)
        .arg("-u")
        .arg("-c")
        .arg(PY)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    match spawned {
        Ok(mut child) => {
            let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
                st.unavailable = true;
                return false;
            };
            st.proc = Some(Proc {
                child,
                stdin,
                stdout: BufReader::new(stdout),
            });
            true
        }
        Err(_) => {
            st.unavailable = true;
            false
        }
    }
}

fn sv_json(v: &Sv) -> J {
    match v {
        Sv::Null => J::Null,
        Sv::Int(i) => json!(i),
        Sv::Real(f) => {
            if f.is_nan() {
                json!({ "f": "nan" })
            } else if f.is_infinite() {
                json!({ "f": if f.is_sign_positive() { "inf" } else { "-inf" } })
            } else {
                json!(f)
            }
        }
        Sv::Text(s) => json!(s),
        Sv::Blob(b) => json!({ "b": b64(b) }),
    }
}

fn json_sv(v: &J) -> Sv {
    match v {
        J::Null => Sv::Null,
        J::Bool(b) => Sv::Int(*b as i64),
        J::Number(n) => n
            .as_i64()
            .map(Sv::Int)
            .unwrap_or_else(|| Sv::Real(n.as_f64().unwrap_or(0.0))),
        J::String(s) => Sv::Text(s.clone()),
        J::Object(o) => {
            if let Some(tag) = o.get("f").and_then(J::as_str) {
                let f = match tag {
                    "inf" => f64::INFINITY,
                    "-inf" => f64::NEG_INFINITY,
                    _ => f64::NAN,
                };
                Sv::Real(f)
            } else {
                Sv::Blob(
                    o.get("b")
                        .and_then(J::as_str)
                        .map(unb64)
                        .unwrap_or_default(),
                )
            }
        }
        J::Array(_) => Sv::Null,
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
fn b64(data: &[u8]) -> String {
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = ((c[0] as u32) << 16)
            | ((*c.get(1).unwrap_or(&0) as u32) << 8)
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
fn unb64(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0;
    for ch in s.bytes() {
        let Some(v) = B64.iter().position(|&b| b == ch) else {
            continue;
        };
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    out
}

fn sql_exc(kind: &str, msg: &str, sql: &str) -> VmExit {
    let class = match kind {
        "constraint" => "Landroid/database/sqlite/SQLiteConstraintException;",
        _ => "Landroid/database/sqlite/SQLiteException;",
    };
    // AOSP SQLiteConnection message format: "<msg> (code N NAME)"
    let code = if kind != "constraint" {
        "1 SQLITE_ERROR"
    } else if msg.contains("UNIQUE") || msg.contains("PRIMARY KEY") {
        "2067 SQLITE_CONSTRAINT_UNIQUE"
    } else if msg.contains("NOT NULL") {
        "1299 SQLITE_CONSTRAINT_NOTNULL"
    } else if msg.contains("FOREIGN KEY") {
        "787 SQLITE_CONSTRAINT_FOREIGNKEY"
    } else {
        "19 SQLITE_CONSTRAINT"
    };
    let text = if kind == "constraint" {
        format!("{msg} (code {code})")
    } else {
        format!("{msg} (code {code}): , while compiling: {sql}")
    };
    Throwable::new(class, text).into()
}

/// `RD_SQL_TRACE=1`: log every statement, its bound args and any error.
fn sql_trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("RD_SQL_TRACE").is_ok_and(|v| v != "0"))
}

fn send(vm: &mut Engine, req: &J) -> Result<J, VmExit> {
    let line = {
        let p =
            vm.fw.m8.sqlx.proc.as_mut().ok_or_else(|| {
                crate::err::vm_error("INVALID_STATE", "SQLite backend not running")
            })?;
        let io = |e: std::io::Error| {
            VmExit::from(crate::err::vm_error(
                "VM_ERROR",
                format!("SQLite backend I/O: {e}"),
            ))
        };
        writeln!(p.stdin, "{req}").map_err(io)?;
        p.stdin.flush().map_err(io)?;
        let mut line = String::new();
        p.stdout.read_line(&mut line).map_err(io)?;
        line
    };
    serde_json::from_str(&line)
        .map_err(|e| crate::err::vm_error("VM_ERROR", format!("SQLite backend reply: {e}")).into())
}

fn persistent_db(db: &str) -> bool {
    !db.is_empty() && db != ":memory:" && !db.starts_with(":memory:")
}

fn first_keyword(sql: &str) -> String {
    sql.trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_ascii_uppercase()
}

fn in_transaction(vm: &Engine, db: &str) -> bool {
    vm.fw
        .m8
        .sqlx
        .txns
        .get(db)
        .map(|t| !t.levels.is_empty())
        .unwrap_or(false)
}

fn ensure_loaded(vm: &mut Engine, db: &str) -> Result<(), VmExit> {
    if !persistent_db(db) || vm.fw.m8.sqlx.loaded.contains(db) {
        return Ok(());
    }
    // Mark first: loading is a backend-process boundary, and a failed load is
    // a typed corruption error rather than a reason to retry on every SQL op.
    vm.fw.m8.sqlx.loaded.insert(db.to_string());
    let Some(bytes) = crate::jfile::read_bytes(db).filter(|b| !b.is_empty()) else {
        return Ok(());
    };
    let r = send(vm, &json!({"op": "load", "db": db, "data": b64(&bytes)}))?;
    if let Some(e) = r.get("err").and_then(J::as_str) {
        return Err(Throwable::new(
            "Landroid/database/sqlite/SQLiteDatabaseCorruptException;",
            format!("loading persistent database {db}: {e}"),
        )
        .into());
    }
    Ok(())
}

fn persist_db(vm: &mut Engine, db: &str) -> Result<(), VmExit> {
    if !persistent_db(db) {
        return Ok(());
    }
    ensure_loaded(vm, db)?;
    let r = send(vm, &json!({"op": "dump", "db": db}))?;
    if let Some(e) = r.get("err").and_then(J::as_str) {
        return Err(sql_exc(
            r.get("kind").and_then(J::as_str).unwrap_or(""),
            e,
            "<serialize database>",
        ));
    }
    let data = r
        .get("data")
        .and_then(J::as_str)
        .map(unb64)
        .unwrap_or_default();
    if !data.is_empty() {
        crate::jfile::write_bytes(db, data);
    }
    Ok(())
}

fn backend_close(vm: &mut Engine, db: &str) {
    let _ = send(vm, &json!({"op": "close", "db": db}));
    vm.fw.m8.sqlx.loaded.remove(db);
    vm.fw.m8.sqlx.txns.remove(db);
    vm.fw.m8.sqlx.programs.retain(|_, (pdb, _, _)| pdb != db);
    vm.fw.m8.sqlx.dbs.retain(|_, pdb| pdb != db);
}

/// Forgets every in-Engine handle for a database path before its file is
/// deleted (e.g. `Context.deleteDatabase`), so a later mutation through a
/// stale handle cannot resurrect the deleted image. The legacy subset is
/// keyed by helper name, so its basename entry is removed too.
pub(crate) fn forget_database(vm: &mut Engine, path: &str) -> bool {
    let was_open =
        vm.fw.m8.sqlx.dbs.values().any(|db| db == path) || vm.fw.m8.sqlx.loaded.contains(path);
    if was_open {
        backend_close(vm, path);
    }
    let base = path.rsplit('/').next().unwrap_or(path);
    let subset = vm.fw.m8.databases.remove(base).is_some();
    vm.fw.m8.db_by_obj.retain(|_, name| name != base);
    was_open || subset
}

fn should_persist_after(sql: &str) -> bool {
    let kw = first_keyword(sql);
    matches!(
        kw.as_str(),
        "INSERT"
            | "UPDATE"
            | "DELETE"
            | "REPLACE"
            | "CREATE"
            | "DROP"
            | "ALTER"
            | "VACUUM"
            | "REINDEX"
            | "ANALYZE"
            | "COMMIT"
            | "END"
    ) || (kw == "PRAGMA" && sql.contains('='))
}

fn run(vm: &mut Engine, db: &str, sql: &str, args: &[Sv]) -> Result<Res, VmExit> {
    ensure_loaded(vm, db)?;
    let req = json!({"op": "exec", "db": db, "sql": sql, "args": args.iter().map(sv_json).collect::<Vec<_>>()});
    let r = send(vm, &req)?;
    if sql_trace() {
        eprintln!(
            "S/RustyDroid: sql {sql} args={} err={}",
            req["args"],
            r.get("err").and_then(J::as_str).unwrap_or("-")
        );
    }
    if let Some(e) = r.get("err").and_then(J::as_str) {
        return Err(sql_exc(
            r.get("kind").and_then(J::as_str).unwrap_or(""),
            e,
            sql,
        ));
    }
    let cols = r["cols"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| c.as_str().unwrap_or("").to_string())
                .collect()
        })
        .unwrap_or_default();
    let rows = r["rows"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|row| {
                    row.as_array()
                        .map(|c| c.iter().map(json_sv).collect())
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();
    let res = Res {
        cols,
        rows,
        changes: r["changes"].as_i64().unwrap_or(0),
        last: r["last"].as_i64().unwrap_or(0),
    };
    // Durability boundary: flush committed work to the VFS image. Work inside
    // an explicit transaction is not flushed until COMMIT/END runs, matching
    // the commit boundary instead of leaking uncommitted rows across launches.
    let kw = first_keyword(sql);
    // `changes > 0` also catches mutating CTEs (`WITH ... DELETE ...`),
    // whose first keyword is not itself a mutation verb.
    if persistent_db(db)
        && (should_persist_after(sql) || res.changes > 0)
        && (!in_transaction(vm, db) || matches!(kw.as_str(), "COMMIT" | "END"))
    {
        persist_db(vm, db)?;
    }
    Ok(res)
}

// ── Java <-> SQL values ─────────────────────────────────────────────────

fn jstr(vm: &Engine, v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::Obj(r)) => vm.heap.as_str(*r).ok().map(str::to_string),
        _ => None,
    }
}

fn java_to_sv(vm: &Engine, v: &Value) -> Sv {
    match v {
        Value::Null => Sv::Null,
        Value::Int(i) => Sv::Int(*i as i64),
        Value::Long(l) => Sv::Int(*l),
        Value::Float(f) => Sv::Real(*f as f64),
        Value::Double(d) => Sv::Real(*d),
        Value::Obj(r) => {
            if let Ok(s) = vm.heap.as_str(*r) {
                return Sv::Text(s.to_string());
            }
            let class = vm.heap.class_of(*r).unwrap_or("").to_string();
            if class == "[B" {
                return Sv::Blob(
                    vm.heap
                        .as_array_elems(*r)
                        .map(|(e, _)| e.iter().map(|x| x.as_int().unwrap_or(0) as u8).collect())
                        .unwrap_or_default(),
                );
            }
            match vm.heap.get_field(*r, "value") {
                Ok(
                    inner @ (Value::Int(_) | Value::Long(_) | Value::Float(_) | Value::Double(_)),
                ) => {
                    if class == "Ljava/lang/Float;" || class == "Ljava/lang/Double;" {
                        java_to_sv(vm, &inner)
                    } else {
                        match inner {
                            Value::Int(i) => Sv::Int(i as i64),
                            other => java_to_sv(vm, &other),
                        }
                    }
                }
                _ => Sv::Text(format!("{class}@{r}")),
            }
        }
        _ => Sv::Null,
    }
}

fn str_array(vm: &Engine, v: Option<&Value>) -> Vec<Sv> {
    match v {
        Some(Value::Obj(a)) => vm
            .heap
            .as_array_elems(*a)
            .map(|(e, _)| e.iter().map(|x| java_to_sv(vm, x)).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn sv_text(v: &Sv) -> Option<String> {
    Some(match v {
        Sv::Null => return None,
        Sv::Int(i) => i.to_string(),
        Sv::Real(f) => {
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{f:.1}")
            } else {
                f.to_string()
            }
        }
        Sv::Text(s) => s.clone(),
        Sv::Blob(b) => String::from_utf8_lossy(b).into_owned(),
    })
}
fn sv_long(v: &Sv) -> i64 {
    match v {
        Sv::Null => 0,
        Sv::Int(i) => *i,
        Sv::Real(f) => *f as i64,
        Sv::Text(s) => {
            let t = s.trim();
            t.parse::<i64>()
                .ok()
                .or_else(|| t.parse::<f64>().ok().map(|f| f as i64))
                .unwrap_or(0)
        }
        Sv::Blob(_) => 0,
    }
}
fn sv_double(v: &Sv) -> f64 {
    match v {
        Sv::Null | Sv::Blob(_) => 0.0,
        Sv::Int(i) => *i as f64,
        Sv::Real(f) => *f,
        Sv::Text(s) => s.trim().parse().unwrap_or(0.0),
    }
}

fn new_array(
    vm: &mut Engine,
    desc: &str,
    kind: ElemKind,
    vals: Vec<Value>,
) -> Result<Value, VmExit> {
    let arr = vm
        .heap
        .alloc_array(kind, vals.len(), desc)
        .map_err(crate::err::oom_throwable)?;
    *vm.heap
        .array_elems_mut(arr)
        .map_err(|e| crate::err::vm_error("VM_TYPE_ERROR", e))? = vals;
    Ok(Value::Obj(arr))
}

fn jstring(vm: &mut Engine, s: String) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::intrinsics::alloc_string(vm, s)?))
}

// ── objects ─────────────────────────────────────────────────────────────

fn is_db_class(vm: &Engine, cls: &str) -> bool {
    cls == DB || cls == CLOSABLE || vm.cp.is_subtype(cls, DB)
}

fn is_program_class(vm: &Engine, cls: &str) -> bool {
    matches!(cls, STMT | QUERY | PROGRAM | CLOSABLE) || vm.cp.is_subtype(cls, PROGRAM)
}

fn is_cursor_class(vm: &Engine, cls: &str) -> bool {
    matches!(
        cls,
        SQL_CURSOR
            | "Landroid/database/Cursor;"
            | "Landroid/database/AbstractCursor;"
            | "Landroid/database/AbstractWindowedCursor;"
            | "Landroid/database/CrossProcessCursor;"
            | "Ljava/io/Closeable;"
            | "Ljava/lang/AutoCloseable;"
    ) || vm.cp.is_subtype(cls, SQL_CURSOR)
        || vm.cp.is_subtype(cls, "Landroid/database/Cursor;")
}

fn db_name(vm: &Engine, recv: ObjRef) -> Option<String> {
    let cls = vm.heap.class_of(recv).ok()?;
    if !is_db_class(vm, cls) {
        return None;
    }
    vm.fw.m8.sqlx.dbs.get(&recv).cloned()
}

fn new_db(vm: &mut Engine, path: &str) -> Result<ObjRef, VmExit> {
    let r = vm
        .heap
        .alloc_instance(DB.to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    vm.fw.m8.sqlx.clear_slot(r);
    vm.fw.m8.sqlx.dbs.insert(r, path.to_string());
    if path.starts_with('/') {
        crate::jfile::touch(path);
    }
    Ok(r)
}

fn new_program(
    vm: &mut Engine,
    class: &str,
    db: &str,
    sql: String,
    binds: Vec<Sv>,
) -> Result<ObjRef, VmExit> {
    let r = vm
        .heap
        .alloc_instance(class.to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    vm.fw.m8.sqlx.clear_slot(r);
    vm.fw
        .m8
        .sqlx
        .programs
        .insert(r, (db.to_string(), sql, binds));
    Ok(r)
}

fn new_cursor_obj(vm: &mut Engine, res: Res) -> Result<Value, VmExit> {
    let r = vm
        .heap
        .alloc_instance(SQL_CURSOR.to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    vm.fw.m8.sqlx.clear_slot(r);
    vm.fw.m8.sqlx.cursors.insert(
        r,
        Cursor {
            cols: res.cols,
            rows: res.rows,
            pos: -1,
            closed: false,
        },
    );
    Ok(Value::Obj(r))
}

fn version(vm: &mut Engine, db: &str) -> Result<i64, VmExit> {
    let r = run(vm, db, "PRAGMA user_version", &[])?;
    Ok(r.rows
        .first()
        .and_then(|row| row.first())
        .map(sv_long)
        .unwrap_or(0))
}

fn begin(vm: &mut Engine, db: &str) -> Result<(), VmExit> {
    let depth = vm
        .fw
        .m8
        .sqlx
        .txns
        .get(db)
        .map(|t| t.levels.len())
        .unwrap_or(0);
    if depth == 0 {
        run(vm, db, "BEGIN IMMEDIATE", &[])?;
    }
    let t = vm.fw.m8.sqlx.txns.entry(db.to_string()).or_default();
    if depth == 0 {
        t.failed = false;
    }
    t.levels.push(false);
    Ok(())
}

fn end(vm: &mut Engine, db: &str) -> Result<(), VmExit> {
    let Some(t) = vm.fw.m8.sqlx.txns.get_mut(db) else {
        return Err(Throwable::new(
            "Ljava/lang/IllegalStateException;",
            "Cannot perform this operation because there is no current transaction.".to_string(),
        )
        .into());
    };
    let Some(ok) = t.levels.pop() else {
        return Err(Throwable::new(
            "Ljava/lang/IllegalStateException;",
            "Cannot perform this operation because there is no current transaction.".to_string(),
        )
        .into());
    };
    if !ok {
        t.failed = true;
    }
    if t.levels.is_empty() {
        let commit = !t.failed;
        run(vm, db, if commit { "COMMIT" } else { "ROLLBACK" }, &[])?;
    }
    Ok(())
}

/// Builds `SELECT` for the `SQLiteDatabase.query` overloads.
#[allow(clippy::too_many_arguments)]
fn build_select(
    distinct: bool,
    table: &str,
    cols: Option<Vec<String>>,
    sel: Option<String>,
    group: Option<String>,
    having: Option<String>,
    order: Option<String>,
    limit: Option<String>,
) -> String {
    let mut q = String::from("SELECT ");
    if distinct {
        q.push_str("DISTINCT ");
    }
    match cols {
        Some(c) if !c.is_empty() => q.push_str(&c.join(", ")),
        _ => q.push('*'),
    }
    q.push_str(" FROM ");
    q.push_str(table);
    for (kw, v) in [
        (" WHERE ", sel),
        (" GROUP BY ", group),
        (" HAVING ", having),
        (" ORDER BY ", order),
        (" LIMIT ", limit),
    ] {
        if let Some(v) = v.filter(|v| !v.is_empty()) {
            q.push_str(kw);
            q.push_str(&v);
        }
    }
    q
}

fn content_values(vm: &Engine, cv: Option<&Value>) -> Vec<(String, Sv)> {
    let Some(Value::Obj(r)) = cv else {
        return Vec::new();
    };
    vm.fw
        .m8
        .content_values
        .get(r)
        .map(|v| {
            v.iter()
                .map(|(k, x)| (k.clone(), java_to_sv(vm, x)))
                .collect()
        })
        .unwrap_or_default()
}

const CONFLICT: [&str; 6] = [
    "",
    " OR ROLLBACK",
    " OR ABORT",
    " OR FAIL",
    " OR IGNORE",
    " OR REPLACE",
];

fn insert(
    vm: &mut Engine,
    db: &str,
    table: &str,
    hack: Option<String>,
    cv: Vec<(String, Sv)>,
    conflict: usize,
) -> Result<i64, VmExit> {
    let (sql, args) = if cv.is_empty() {
        let col = hack.unwrap_or_default();
        (
            format!(
                "INSERT{} INTO {table}({col}) VALUES (NULL)",
                CONFLICT[conflict.min(5)]
            ),
            Vec::new(),
        )
    } else {
        let cols: Vec<&str> = cv.iter().map(|(k, _)| k.as_str()).collect();
        let marks = vec!["?"; cv.len()].join(",");
        (
            format!(
                "INSERT{} INTO {table}({}) VALUES ({marks})",
                CONFLICT[conflict.min(5)],
                cols.join(",")
            ),
            cv.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
        )
    };
    let r = run(vm, db, &sql, &args)?;
    Ok(if r.changes > 0 { r.last } else { -1 })
}

fn db_intrinsic(
    vm: &mut Engine,
    db: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let s = |i: usize| jstr(vm, args.get(i));
    Ok(Some(match name {
        "execSQL" | "execPerConnectionSQL" => {
            let sql = s(0).unwrap_or_default();
            let binds = str_array(vm, args.get(1));
            run(vm, db, &sql, &binds)?;
            Value::Null
        }
        "rawQuery" => {
            let sql = s(0).unwrap_or_default();
            let binds = str_array(vm, args.get(1));
            let res = run(vm, db, &sql, &binds)?;
            new_cursor_obj(vm, res)?
        }
        "rawQueryWithFactory" => {
            let factory = args.first().cloned().unwrap_or(Value::Null);
            let sql = s(1).unwrap_or_default();
            let binds = str_array(vm, args.get(2));
            let edit = args.get(3).cloned().unwrap_or(Value::Null);
            match factory {
                Value::Obj(f) => {
                    let q = new_program(vm, QUERY, db, sql, binds)?;
                    let driver = vm
                        .heap
                        .alloc_instance(DRIVER.to_string(), Vec::new())
                        .map_err(crate::err::oom_throwable)?;
                    let out = crate::m8::call_dex(
                        vm,
                        f,
                        "newCursor",
                        "(Landroid/database/sqlite/SQLiteDatabase;Landroid/database/sqlite/SQLiteCursorDriver;Ljava/lang/String;Landroid/database/sqlite/SQLiteQuery;)Landroid/database/Cursor;",
                        vec![Value::Obj(recv), Value::Obj(driver), edit, Value::Obj(q)],
                    )?;
                    match out {
                        Some(c) => c,
                        None => {
                            let Some((_, p_sql, p_binds)) = vm.fw.m8.sqlx.programs.get(&q).cloned()
                            else {
                                return Err(Throwable::new(
                                    "Ljava/lang/IllegalStateException;",
                                    "attempt to re-open an already-closed object".to_string(),
                                )
                                .into());
                            };
                            let res = run(vm, db, &p_sql, &p_binds)?;
                            new_cursor_obj(vm, res)?
                        }
                    }
                }
                _ => {
                    let res = run(vm, db, &sql, &binds)?;
                    new_cursor_obj(vm, res)?
                }
            }
        }
        "query" | "queryWithFactory" => {
            // (table, cols, sel, selArgs, group, having, order[, limit])
            // (distinct, table, cols, sel, selArgs, group, having, order, limit[, signal])
            // queryWithFactory(factory, distinct, table, …)
            let base = match name {
                "queryWithFactory" => 1,
                _ => 0,
            };
            let distinct_form = sig.starts_with("(Z") || name == "queryWithFactory";
            let (distinct, o) = if distinct_form {
                (matches!(args.get(base), Some(Value::Int(1))), base + 1)
            } else {
                (false, 0)
            };
            let cols = match args.get(o + 1) {
                Some(Value::Obj(a)) => Some(
                    vm.heap
                        .as_array_elems(*a)
                        .map(|(e, _)| e.iter().filter_map(|x| jstr(vm, Some(x))).collect())
                        .unwrap_or_default(),
                ),
                _ => None,
            };
            let sql = build_select(
                distinct,
                &s(o).unwrap_or_default(),
                cols,
                s(o + 2),
                s(o + 4),
                s(o + 5),
                s(o + 6),
                s(o + 7),
            );
            let binds = str_array(vm, args.get(o + 3));
            let res = run(vm, db, &sql, &binds)?;
            new_cursor_obj(vm, res)?
        }
        "insert" | "insertOrThrow" | "insertWithOnConflict" | "replace" | "replaceOrThrow" => {
            let table = s(0).unwrap_or_default();
            let hack = s(1);
            let cv = content_values(vm, args.get(2));
            let conflict = match name {
                "insertWithOnConflict" => {
                    args.get(3).and_then(|v| v.as_int().ok()).unwrap_or(0) as usize
                }
                "replace" | "replaceOrThrow" => 5,
                _ => 0,
            };
            let r = insert(vm, db, &table, hack, cv, conflict);
            match (r, name) {
                (Ok(id), _) => Value::Long(id),
                (Err(VmExit::Exception(_)), "insert" | "replace" | "insertWithOnConflict") => {
                    Value::Long(-1)
                }
                (Err(e), _) => return Err(e),
            }
        }
        "update" | "updateWithOnConflict" => {
            let table = s(0).unwrap_or_default();
            let cv = content_values(vm, args.get(1));
            if cv.is_empty() {
                return Err(Throwable::new(
                    "Ljava/lang/IllegalArgumentException;",
                    "Empty values".to_string(),
                )
                .into());
            }
            let conflict = if name == "updateWithOnConflict" {
                args.get(4).and_then(|v| v.as_int().ok()).unwrap_or(0) as usize
            } else {
                0
            };
            let set: Vec<String> = cv.iter().map(|(k, _)| format!("{k}=?")).collect();
            let mut sql = format!(
                "UPDATE{} {table} SET {}",
                CONFLICT[conflict.min(5)],
                set.join(",")
            );
            if let Some(w) = s(2).filter(|w| !w.is_empty()) {
                sql.push_str(" WHERE ");
                sql.push_str(&w);
            }
            let mut binds: Vec<Sv> = cv.into_iter().map(|(_, v)| v).collect();
            binds.extend(str_array(vm, args.get(3)));
            Value::Int(run(vm, db, &sql, &binds)?.changes as i32)
        }
        "delete" => {
            let table = s(0).unwrap_or_default();
            let mut sql = format!("DELETE FROM {table}");
            if let Some(w) = s(1).filter(|w| !w.is_empty()) {
                sql.push_str(" WHERE ");
                sql.push_str(&w);
            }
            let binds = str_array(vm, args.get(2));
            Value::Int(run(vm, db, &sql, &binds)?.changes as i32)
        }
        "compileStatement" => {
            let sql = s(0).unwrap_or_default();
            Value::Obj(new_program(vm, STMT, db, sql, Vec::new())?)
        }
        "beginTransaction"
        | "beginTransactionNonExclusive"
        | "beginTransactionWithListener"
        | "beginTransactionWithListenerNonExclusive"
        | "beginTransactionReadOnly" => {
            begin(vm, db)?;
            if let Some(Value::Obj(l)) = args.first() {
                crate::m8::call_dex(vm, *l, "onBegin", "()V", vec![])?;
            }
            Value::Null
        }
        "setTransactionSuccessful" => {
            match vm
                .fw
                .m8
                .sqlx
                .txns
                .get_mut(db)
                .and_then(|t| t.levels.last_mut())
            {
                Some(top) => *top = true,
                None => {
                    return Err(Throwable::new(
                        "Ljava/lang/IllegalStateException;",
                        "Cannot perform this operation because there is no current transaction."
                            .to_string(),
                    )
                    .into())
                }
            }
            Value::Null
        }
        "endTransaction" => {
            end(vm, db)?;
            Value::Null
        }
        "inTransaction" | "isDbLockedByCurrentThread" => Value::Int(in_transaction(vm, db) as i32),
        "isInMemoryDatabase" => Value::Int((!persistent_db(db)) as i32),
        "yieldIfContendedSafely"
        | "yieldIfContended"
        | "isDbLockedByOtherThreads"
        | "isReadOnly" => Value::Int(0),
        "getVersion" => Value::Int(version(vm, db)? as i32),
        "setVersion" => {
            let v = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0);
            run(vm, db, &format!("PRAGMA user_version = {v}"), &[])?;
            Value::Null
        }
        "needUpgrade" => {
            let want = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0) as i64;
            Value::Int((want > version(vm, db)?) as i32)
        }
        "setForeignKeyConstraintsEnabled" => {
            let on = matches!(args.first(), Some(Value::Int(1)));
            run(
                vm,
                db,
                &format!("PRAGMA foreign_keys = {}", if on { "ON" } else { "OFF" }),
                &[],
            )?;
            Value::Null
        }
        "getPath" => jstring(vm, db.to_string())?,
        "isOpen" => Value::Int(1),
        "close" | "releaseReference" | "acquireReference" | "releaseReferenceFromContainer" => {
            // Reference-count/close calls are a safe durability point; the
            // object model stays usable (existing RustyDroid deviation), but
            // committed named-database state is flushed to the VFS image.
            persist_db(vm, db)?;
            Value::Null
        }
        "enableWriteAheadLogging" => Value::Int(1),
        "isWriteAheadLoggingEnabled" => Value::Int(0),
        "disableWriteAheadLogging"
        | "setLocale"
        | "setMaxSqlCacheSize"
        | "setPageSize"
        | "setCustomScalarFunction"
        | "setCustomAggregateFunction" => Value::Null,
        "getMaximumSize" => Value::Long(1 << 40),
        "getPageSize" => Value::Long(4096),
        "setMaximumSize" => Value::Long(
            args.first()
                .map(|v| match v {
                    Value::Long(l) => *l,
                    _ => 0,
                })
                .unwrap_or(0),
        ),
        "validateSql" => {
            let sql = s(0).unwrap_or_default();
            run(vm, db, &format!("EXPLAIN {sql}"), &[])?;
            Value::Null
        }
        "getAttachedDbs" => {
            let main = jstring(vm, "main".into())?;
            let path = jstring(vm, db.to_string())?;
            let pair = vm
                .heap
                .alloc_instance(
                    "Landroid/util/Pair;".to_string(),
                    vec![("first".to_string(), main), ("second".to_string(), path)],
                )
                .map_err(crate::err::oom_throwable)?;
            crate::collections::new_list(vm, "Ljava/util/ArrayList;", vec![Value::Obj(pair)])?
        }
        "isDatabaseIntegrityOk" => Value::Int(1),
        _ => return Ok(None),
    }))
}

/// Counts SQLite bind parameters (`?`, `?NNN`, `:name`, `@name`, `$name`)
/// outside string/identifier quotes and SQL comments, mirroring
/// `sqlite3_bind_parameter_count` (capped at SQLite's `32_766` variable limit).
fn count_sql_bind_params(sql: &str) -> usize {
    const MAX_SQLITE_VARS: usize = 32_766;
    let b = sql.as_bytes();
    let mut i = 0usize;
    let mut next_idx = 0usize;
    let mut max_idx = 0usize;
    let mut named: Vec<String> = Vec::new();
    while i < b.len() {
        match b[i] {
            b'\'' | b'"' | b'`' => {
                let q = b[i];
                i += 1;
                while i < b.len() {
                    if b[i] == q {
                        i += 1;
                        if i < b.len() && b[i] == q {
                            i += 1;
                        } else {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
            }
            b'[' => {
                i += 1;
                while i < b.len() && b[i] != b']' {
                    i += 1;
                }
                if i < b.len() {
                    i += 1;
                }
            }
            b'-' if i + 1 < b.len() && b[i + 1] == b'-' => {
                i += 2;
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < b.len() && b[i + 1] == b'*' => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(b.len());
            }
            b'?' => {
                i += 1;
                let start = i;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                if i > start {
                    let n = sql[start..i].parse::<usize>().unwrap_or(0);
                    if (1..=MAX_SQLITE_VARS).contains(&n) {
                        next_idx = n;
                        max_idx = max_idx.max(n);
                    }
                } else {
                    next_idx = next_idx.saturating_add(1).min(MAX_SQLITE_VARS);
                    max_idx = max_idx.max(next_idx);
                }
            }
            b':' | b'@' | b'$'
                if i + 1 < b.len() && (b[i + 1].is_ascii_alphabetic() || b[i + 1] == b'_') =>
            {
                let start = i;
                i += 1;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'$')
                {
                    i += 1;
                }
                let tok = &sql[start..i];
                if !named.iter().any(|x| x == tok) {
                    named.push(tok.to_string());
                    next_idx = next_idx.saturating_add(1).min(MAX_SQLITE_VARS);
                    max_idx = max_idx.max(next_idx);
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    max_idx.min(MAX_SQLITE_VARS)
}

fn program_intrinsic(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let Some((db, sql, binds)) = vm.fw.m8.sqlx.programs.get(&recv).cloned() else {
        return Err(Throwable::new(
            "Ljava/lang/IllegalStateException;",
            "attempt to re-open an already-closed object".to_string(),
        )
        .into());
    };
    let num_params = count_sql_bind_params(&sql).max(binds.len()).min(32_766);
    let raw_index = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0);
    let check_bind_idx = |raw: i32| -> Result<usize, VmExit> {
        if raw < 1 || (raw as usize) > num_params {
            return Err(Throwable::new(
                "Ljava/lang/IllegalArgumentException;",
                format!(
                    "Cannot bind argument at index {raw} because the index is out of range.  The statement has {num_params} parameters."
                ),
            )
            .into());
        }
        Ok((raw as usize) - 1)
    };
    let bind = |vm: &mut Engine, idx: usize, v: Sv| {
        if let Some(p) = vm.fw.m8.sqlx.programs.get_mut(&recv) {
            if p.2.len() <= idx {
                p.2.resize(idx + 1, Sv::Null);
            }
            p.2[idx] = v;
        }
    };
    Ok(Some(match name {
        "bindNull" => {
            let idx = check_bind_idx(raw_index)?;
            bind(vm, idx, Sv::Null);
            Value::Null
        }
        "bindLong" => {
            let idx = check_bind_idx(raw_index)?;
            let v = match args.get(1) {
                Some(Value::Long(l)) => *l,
                Some(Value::Int(i)) => *i as i64,
                _ => 0,
            };
            bind(vm, idx, Sv::Int(v));
            Value::Null
        }
        "bindDouble" => {
            let idx = check_bind_idx(raw_index)?;
            let v = match args.get(1) {
                Some(Value::Double(d)) => *d,
                Some(Value::Float(f)) => *f as f64,
                _ => 0.0,
            };
            bind(vm, idx, Sv::Real(v));
            Value::Null
        }
        "bindString" | "bindBlob" => {
            let idx = check_bind_idx(raw_index)?;
            match args.get(1) {
                None | Some(Value::Null) => {
                    return Err(Throwable::new(
                        "Ljava/lang/IllegalArgumentException;",
                        format!("the bind value at index {} is null", idx + 1),
                    )
                    .into())
                }
                Some(v) => {
                    let sv = java_to_sv(vm, v);
                    bind(vm, idx, sv);
                }
            }
            Value::Null
        }
        "bindAllArgsAsStrings" => {
            let all = str_array(vm, args.first());
            if let Some(p) = vm.fw.m8.sqlx.programs.get_mut(&recv) {
                p.2 = all;
            }
            Value::Null
        }
        "clearBindings" => {
            if let Some(p) = vm.fw.m8.sqlx.programs.get_mut(&recv) {
                p.2.clear();
            }
            Value::Null
        }
        "execute" => {
            run(vm, &db, &sql, &binds)?;
            Value::Null
        }
        "executeInsert" => {
            let r = run(vm, &db, &sql, &binds)?;
            Value::Long(if r.changes > 0 { r.last } else { -1 })
        }
        "executeUpdateDelete" => Value::Int(run(vm, &db, &sql, &binds)?.changes as i32),
        "simpleQueryForLong" | "simpleQueryForString" => {
            let r = run(vm, &db, &sql, &binds)?;
            let Some(v) = r.rows.first().and_then(|row| row.first()).cloned() else {
                return Err(Throwable::new(
                    "Landroid/database/sqlite/SQLiteDoneException;",
                    "not an error (code 101 SQLITE_DONE)".to_string(),
                )
                .into());
            };
            if name == "simpleQueryForLong" {
                Value::Long(sv_long(&v))
            } else {
                match sv_text(&v) {
                    Some(t) => jstring(vm, t)?,
                    None => Value::Null,
                }
            }
        }
        "getSql" | "toString" => jstring(
            vm,
            if name == "toString" {
                format!("SQLiteProgram: {sql}")
            } else {
                sql
            },
        )?,
        "close" | "releaseReference" | "acquireReference" | "onAllReferencesReleased" => {
            Value::Null
        }
        "<init>" => Value::Null,
        _ if sig.is_empty() => return Ok(None),
        _ => return Ok(None),
    }))
}

fn cursor_intrinsic(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let i = |k: usize| args.get(k).and_then(|v| v.as_int().ok()).unwrap_or(0) as i64;
    let (n, pos, ncols) = {
        let c = &vm.fw.m8.sqlx.cursors[&recv];
        (c.rows.len() as i64, c.pos, c.cols.len() as i64)
    };
    let set_pos = |vm: &mut Engine, p: i64| -> Value {
        let c = vm.fw.m8.sqlx.cursors.get_mut(&recv).expect("cursor");
        let p = p.clamp(-1, n);
        c.pos = p;
        Value::Int((p >= 0 && p < n) as i32)
    };
    let cell = |vm: &Engine, col: i64| -> Result<Sv, VmExit> {
        let c = &vm.fw.m8.sqlx.cursors[&recv];
        if c.pos < 0 || c.pos >= n {
            return Err(Throwable::new(
                "Landroid/database/CursorIndexOutOfBoundsException;",
                format!("Index {} requested, with a size of {n}", c.pos),
            )
            .into());
        }
        c.rows[c.pos as usize]
            .get(col as usize)
            .cloned()
            .ok_or_else(|| {
                Throwable::new(
                    "Ljava/lang/IllegalStateException;",
                    format!("Couldn't read row {}, col {col} from CursorWindow.", c.pos),
                )
                .into()
            })
    };
    Ok(Some(match name {
        "getCount" => Value::Int(n as i32),
        "getPosition" => Value::Int(pos as i32),
        "moveToFirst" => set_pos(vm, 0),
        "moveToLast" => set_pos(vm, n - 1),
        "moveToNext" => set_pos(vm, pos + 1),
        "moveToPrevious" => set_pos(vm, pos - 1),
        "moveToPosition" => set_pos(vm, i(0)),
        "move" => set_pos(vm, pos + i(0)),
        "isFirst" => Value::Int((n > 0 && pos == 0) as i32),
        "isLast" => Value::Int((n > 0 && pos == n - 1) as i32),
        "isBeforeFirst" => Value::Int((n == 0 || pos == -1) as i32),
        "isAfterLast" => Value::Int((n == 0 || pos == n) as i32),
        "getColumnCount" => Value::Int(ncols as i32),
        "getColumnIndex" | "getColumnIndexOrThrow" => {
            let want = jstr(vm, args.first()).unwrap_or_default();
            let cols = vm.fw.m8.sqlx.cursors[&recv].cols.clone();
            // AOSP also matches "table.column" by its last segment.
            let bare = want.rsplit('.').next().unwrap_or(&want).to_string();
            let found = cols
                .iter()
                .position(|c| c.eq_ignore_ascii_case(&want))
                .or_else(|| cols.iter().position(|c| c.eq_ignore_ascii_case(&bare)));
            match (found, name) {
                (Some(k), _) => Value::Int(k as i32),
                (None, "getColumnIndexOrThrow") => {
                    return Err(Throwable::new(
                        "Ljava/lang/IllegalArgumentException;",
                        format!("column '{want}' does not exist. Available columns: {cols:?}"),
                    )
                    .into())
                }
                (None, _) => Value::Int(-1),
            }
        }
        "getColumnName" => {
            let c = vm.fw.m8.sqlx.cursors[&recv]
                .cols
                .get(i(0) as usize)
                .cloned();
            match c {
                Some(c) => jstring(vm, c)?,
                None => Value::Null,
            }
        }
        "getColumnNames" => {
            let cols = vm.fw.m8.sqlx.cursors[&recv].cols.clone();
            let mut vals = Vec::new();
            for c in cols {
                vals.push(jstring(vm, c)?);
            }
            new_array(vm, "[Ljava/lang/String;", ElemKind::Obj, vals)?
        }
        "getString" => match sv_text(&cell(vm, i(0))?) {
            Some(t) => jstring(vm, t)?,
            None => Value::Null,
        },
        "getLong" => Value::Long(sv_long(&cell(vm, i(0))?)),
        "getInt" => Value::Int(sv_long(&cell(vm, i(0))?) as i32),
        "getShort" => Value::Int(sv_long(&cell(vm, i(0))?) as i16 as i32),
        "getDouble" => Value::Double(sv_double(&cell(vm, i(0))?)),
        "getFloat" => Value::Float(sv_double(&cell(vm, i(0))?) as f32),
        "isNull" => Value::Int((cell(vm, i(0))? == Sv::Null) as i32),
        "getType" => Value::Int(match cell(vm, i(0))? {
            Sv::Null => 0,
            Sv::Int(_) => 1,
            Sv::Real(_) => 2,
            Sv::Text(_) => 3,
            Sv::Blob(_) => 4,
        }),
        "getBlob" => match cell(vm, i(0))? {
            Sv::Null => Value::Null,
            other => {
                let bytes = match other {
                    Sv::Blob(b) => b,
                    o => sv_text(&o).unwrap_or_default().into_bytes(),
                };
                new_array(
                    vm,
                    "[B",
                    ElemKind::Int,
                    bytes
                        .into_iter()
                        .map(|b| Value::Int(b as i8 as i32))
                        .collect(),
                )?
            }
        },
        "close" => {
            if let Some(c) = vm.fw.m8.sqlx.cursors.get_mut(&recv) {
                c.closed = true;
                c.rows = Vec::new();
                c.pos = -1;
            }
            Value::Null
        }
        "deactivate" => {
            if let Some(c) = vm.fw.m8.sqlx.cursors.get_mut(&recv) {
                c.closed = true;
            }
            Value::Null
        }
        "isClosed" => Value::Int(vm.fw.m8.sqlx.cursors[&recv].closed as i32),
        "requery" => Value::Int(1),
        "getWantsAllOnMoveCalls" => Value::Int(0),
        "getExtras" => Value::Obj(
            vm.heap
                .alloc_instance("Landroid/os/Bundle;".to_string(), Vec::new())
                .map_err(crate::err::oom_throwable)?,
        ),
        "registerContentObserver"
        | "unregisterContentObserver"
        | "registerDataSetObserver"
        | "unregisterDataSetObserver"
        | "setNotificationUri"
        | "setNotificationUris"
        | "setExtras"
        | "setWindow"
        | "setSelectionArguments" => Value::Null,
        "getNotificationUri" => Value::Null,
        "getNotificationUris" => Value::Null,
        "respond" => args.first().cloned().unwrap_or(Value::Null),
        "<init>" => Value::Null,
        "hashCode" => Value::Int(recv as i32),
        _ if sig.is_empty() => return Ok(None),
        _ => return Ok(None),
    }))
}

fn helper_open(vm: &mut Engine, recv: ObjRef) -> Result<Value, VmExit> {
    if let Ok(Value::Obj(d)) = vm.heap.get_field(recv, "rd_sqlx_db") {
        if db_name(vm, d).is_some() {
            return Ok(Value::Obj(d));
        }
    }
    let name = vm
        .fw
        .m8
        .helpers_db
        .get(&recv)
        .cloned()
        .filter(|n| !n.is_empty());
    let path = match &name {
        Some(n) if n.starts_with('/') => n.clone(),
        Some(n) => {
            let pkg = if vm.fw.package_name.is_empty() {
                "app".to_string()
            } else {
                vm.fw.package_name.clone()
            };
            crate::jfile::mkdirs_pub(&format!("/data/user/0/{pkg}/databases"));
            format!("/data/user/0/{pkg}/databases/{n}")
        }
        None => format!(":memory:{recv}"),
    };
    let d = new_db(vm, &path)?;
    crate::err::put_field_result(vm.heap.put_field(recv, "rd_sqlx_db", Value::Obj(d)))?;
    let dbv = Value::Obj(d);
    let sig1 = "(Landroid/database/sqlite/SQLiteDatabase;)V";
    crate::m8::call_dex(vm, recv, "onConfigure", sig1, vec![dbv.clone()])?;
    let want = match vm.heap.get_field(recv, "rd_sqlx_version") {
        Ok(Value::Int(v)) => v as i64,
        _ => 1,
    };
    let have = version(vm, &path)?;
    if have != want {
        begin(vm, &path)?;
        let r = (|| -> Result<(), VmExit> {
            if have == 0 {
                crate::m8::call_dex(vm, recv, "onCreate", sig1, vec![dbv.clone()])?;
            } else if have < want {
                crate::m8::call_dex(
                    vm,
                    recv,
                    "onUpgrade",
                    "(Landroid/database/sqlite/SQLiteDatabase;II)V",
                    vec![
                        dbv.clone(),
                        Value::Int(have as i32),
                        Value::Int(want as i32),
                    ],
                )?;
            } else {
                let called = crate::m8::call_dex(
                    vm,
                    recv,
                    "onDowngrade",
                    "(Landroid/database/sqlite/SQLiteDatabase;II)V",
                    vec![
                        dbv.clone(),
                        Value::Int(have as i32),
                        Value::Int(want as i32),
                    ],
                )?;
                if called.is_none() {
                    return Err(Throwable::new(
                        "Landroid/database/sqlite/SQLiteException;",
                        format!("Can't downgrade database from version {have} to {want}"),
                    )
                    .into());
                }
            }
            run(vm, &path, &format!("PRAGMA user_version = {want}"), &[])?;
            if let Some(t) = vm
                .fw
                .m8
                .sqlx
                .txns
                .get_mut(&path)
                .and_then(|t| t.levels.last_mut())
            {
                *t = true;
            }
            Ok(())
        })();
        end(vm, &path)?;
        r?;
    }
    crate::m8::call_dex(vm, recv, "onOpen", sig1, vec![dbv.clone()])?;
    Ok(dbv)
}

/// Dispatch for `android.database.sqlite`. `None` = not handled here (the
/// m8 subset or the generic path continues).
pub fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // new SQLiteCursor(driver, editTable, query): execute the bound query
    // BEFORE checking existing `cursors[&recv]` so a recycled `SQLiteCursor`
    // slot runs `<init>` cleanly instead of acting like an old cursor (#257).
    if name == "<init>" && (mref_class == SQL_CURSOR || recv_class == SQL_CURSOR) {
        vm.fw.m8.sqlx.clear_slot(recv);
        let q = args.iter().rev().find_map(|a| match a {
            Value::Obj(o)
                if vm
                    .heap
                    .class_of(*o)
                    .ok()
                    .is_some_and(|c| is_program_class(vm, c))
                    && vm.fw.m8.sqlx.programs.contains_key(o) =>
            {
                Some(*o)
            }
            _ => None,
        });
        if let Some(q) = q {
            let (db, sql, binds) = vm.fw.m8.sqlx.programs[&q].clone();
            let res = run(vm, &db, &sql, &binds)?;
            vm.fw.m8.sqlx.cursors.insert(
                recv,
                Cursor {
                    cols: res.cols,
                    rows: res.rows,
                    pos: -1,
                    closed: false,
                },
            );
            return Ok(Some(Value::Null));
        }
        return Ok(None);
    }
    if vm.fw.m8.sqlx.cursors.contains_key(&recv) {
        if is_cursor_class(vm, recv_class) {
            return cursor_intrinsic(vm, name, sig, recv, args);
        }
        vm.fw.m8.sqlx.cursors.remove(&recv);
    }
    if vm.fw.m8.sqlx.programs.contains_key(&recv) {
        if is_program_class(vm, recv_class) {
            return program_intrinsic(vm, name, sig, recv, args);
        }
        vm.fw.m8.sqlx.programs.remove(&recv);
    }
    if vm.fw.m8.sqlx.dbs.contains_key(&recv) {
        if let Some(db) = db_name(vm, recv) {
            return db_intrinsic(vm, &db, name, sig, recv, args);
        }
        vm.fw.m8.sqlx.dbs.remove(&recv);
    }
    let helper_call = mref_class == HELPER
        || (vm.cp.is_subtype(recv_class, HELPER)
            && vm.cp.resolve_method(mref_class, name, sig).is_none());
    if helper_call {
        match name {
            "<init>" => {
                if !backend_ready(vm) {
                    return Ok(None);
                }
                let dbname = jstr(vm, args.get(1)).unwrap_or_default();
                vm.fw.m8.helpers_db.insert(recv, dbname);
                let v = args.get(3).and_then(|v| v.as_int().ok()).unwrap_or(1);
                if v < 1 {
                    return Err(Throwable::new(
                        "Ljava/lang/IllegalArgumentException;",
                        format!("Version must be >= 1, was {v}"),
                    )
                    .into());
                }
                crate::err::put_field_result(vm.heap.put_field(
                    recv,
                    "rd_sqlx_version",
                    Value::Int(v),
                ))?;
                return Ok(Some(Value::Null));
            }
            "getWritableDatabase" | "getReadableDatabase"
                if vm
                    .heap
                    .get_field(recv, "rd_sqlx_version")
                    .is_ok_and(|v| v != Value::Null) =>
            {
                return Ok(Some(helper_open(vm, recv)?));
            }
            "onConfigure" | "onOpen" => return Ok(Some(Value::Null)),
            "onDowngrade" => {
                let (a, b) = (
                    args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(0),
                    args.get(2).and_then(|v| v.as_int().ok()).unwrap_or(0),
                );
                return Err(Throwable::new(
                    "Landroid/database/sqlite/SQLiteException;",
                    format!("Can't downgrade database from version {a} to {b}"),
                )
                .into());
            }
            "close"
                if vm
                    .heap
                    .get_field(recv, "rd_sqlx_version")
                    .is_ok_and(|v| v != Value::Null) =>
            {
                let _ = vm.heap.put_field(recv, "rd_sqlx_db", Value::Null);
                return Ok(Some(Value::Null));
            }
            _ => return Ok(None),
        }
    }
    let _ = (PROGRAM, CLOSABLE);
    Ok(None)
}

/// `SQLiteDatabase.openDatabase`/`openOrCreateDatabase`/`create`.
pub fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != DB
        || !matches!(
            name,
            "openDatabase"
                | "openOrCreateDatabase"
                | "create"
                | "createInMemory"
                | "deleteDatabase"
        )
    {
        return Ok(None);
    }
    if !backend_ready(vm) {
        return Ok(None);
    }
    let path = match args.first() {
        Some(Value::Obj(o))
            if vm
                .heap
                .class_of(*o)
                .map(|c| c == "Ljava/io/File;")
                .unwrap_or(false) =>
        {
            let p = vm.heap.get_field(*o, "rd_path").unwrap_or(Value::Null);
            jstr(vm, Some(&p)).unwrap_or_default()
        }
        other => jstr(vm, other).unwrap_or_default(),
    };
    if name == "deleteDatabase" {
        let was_open = vm.fw.m8.sqlx.dbs.values().any(|d| *d == path);
        if was_open || vm.fw.m8.sqlx.loaded.contains(&path) {
            backend_close(vm, &path);
        }
        let deleted_file = persistent_db(&path) && crate::jfile::delete_file(&path);
        return Ok(Some(Value::Int((was_open || deleted_file) as i32)));
    }
    let path =
        if matches!(name, "create" | "createInMemory") || path.is_empty() || path == ":memory:" {
            let n = vm.fw.m8.sqlx.dbs.len();
            format!(":memory:{n}")
        } else {
            path
        };
    Ok(Some(Value::Obj(new_db(vm, &path)?)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn base64_roundtrip() {
        for s in [&b""[..], b"a", b"ab", b"abc", b"\x00\xff\x10hello"] {
            assert_eq!(unb64(&b64(s)), s);
        }
    }
    #[test]
    fn persistence_classification() {
        assert!(persistent_db("/data/user/0/app/databases/app.db"));
        assert!(!persistent_db(":memory:"));
        assert!(!persistent_db(":memory:3"));
        assert!(should_persist_after("CREATE TABLE t (x INTEGER)"));
        assert!(should_persist_after(" insert into t values (1)"));
        assert!(should_persist_after("PRAGMA user_version = 2"));
        assert!(!should_persist_after("PRAGMA user_version"));
        assert!(!should_persist_after("SELECT * FROM t"));
    }
    #[test]
    fn select_builder() {
        assert_eq!(
            build_select(
                true,
                "t",
                Some(vec!["a".into(), "b".into()]),
                Some("a=?".into()),
                None,
                None,
                Some("b".into()),
                Some("5".into())
            ),
            "SELECT DISTINCT a, b FROM t WHERE a=? ORDER BY b LIMIT 5"
        );
        assert_eq!(
            build_select(false, "t", None, None, None, None, None, None),
            "SELECT * FROM t"
        );
    }
}
