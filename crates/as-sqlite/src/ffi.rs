use super::*;
use alloc::ffi::CString;
use core::{
    ffi::{c_char, c_int, c_void, CStr},
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};

extern "C" {
    fn sqlite3_hard_heap_limit64(limit: i64) -> i64;
    fn sqlite3_open_v2(
        name: *const c_char,
        out: *mut *mut c_void,
        flags: c_int,
        vfs: *const c_char,
    ) -> c_int;
    fn sqlite3_close(db: *mut c_void) -> c_int;
    fn sqlite3_errmsg(db: *mut c_void) -> *const c_char;
    fn sqlite3_errcode(db: *mut c_void) -> c_int;
    fn sqlite3_prepare_v2(
        db: *mut c_void,
        sql: *const c_char,
        n: c_int,
        stmt: *mut *mut c_void,
        tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_finalize(stmt: *mut c_void) -> c_int;
    fn sqlite3_step(stmt: *mut c_void) -> c_int;
    fn sqlite3_bind_parameter_count(stmt: *mut c_void) -> c_int;
    fn sqlite3_bind_null(stmt: *mut c_void, index: c_int) -> c_int;
    fn sqlite3_bind_int64(stmt: *mut c_void, index: c_int, value: i64) -> c_int;
    fn sqlite3_bind_double(stmt: *mut c_void, index: c_int, value: f64) -> c_int;
    fn sqlite3_bind_text(
        stmt: *mut c_void,
        index: c_int,
        value: *const c_char,
        n: c_int,
        destructor: Option<unsafe extern "C" fn(*mut c_void)>,
    ) -> c_int;
    fn sqlite3_bind_blob(
        stmt: *mut c_void,
        index: c_int,
        value: *const c_void,
        n: c_int,
        destructor: Option<unsafe extern "C" fn(*mut c_void)>,
    ) -> c_int;
    fn sqlite3_column_count(stmt: *mut c_void) -> c_int;
    fn sqlite3_column_name(stmt: *mut c_void, index: c_int) -> *const c_char;
    fn sqlite3_column_type(stmt: *mut c_void, index: c_int) -> c_int;
    fn sqlite3_column_int64(stmt: *mut c_void, index: c_int) -> i64;
    fn sqlite3_column_double(stmt: *mut c_void, index: c_int) -> f64;
    fn sqlite3_column_text(stmt: *mut c_void, index: c_int) -> *const u8;
    fn sqlite3_column_blob(stmt: *mut c_void, index: c_int) -> *const c_void;
    fn sqlite3_column_bytes(stmt: *mut c_void, index: c_int) -> c_int;
    fn sqlite3_get_autocommit(db: *mut c_void) -> c_int;
    fn sqlite3_serialize(db: *mut c_void, name: *const c_char, n: *mut i64, flags: u32) -> *mut u8;
    fn sqlite3_deserialize(
        db: *mut c_void,
        name: *const c_char,
        data: *mut u8,
        n: i64,
        capacity: i64,
        flags: u32,
    ) -> c_int;
    fn sqlite3_malloc64(n: u64) -> *mut c_void;
    fn sqlite3_free(p: *mut c_void);
    fn sqlite3_limit(db: *mut c_void, id: c_int, value: c_int) -> c_int;
    fn sqlite3_progress_handler(
        db: *mut c_void,
        interval: c_int,
        callback: Option<unsafe extern "C" fn(*mut c_void) -> c_int>,
        context: *mut c_void,
    );
}

// THREADSAFE=0 is intentional: all SQLite calls, including initialization,
// prepare, snapshots and close, share this lease on both host and Horizon.
// Connections are !Send/!Sync because they contain an opaque mutable pointer.
static BUSY: AtomicBool = AtomicBool::new(false);
struct Lease;
impl Lease {
    fn acquire() -> Self {
        while BUSY
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        Self
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}
fn failure(code: i32, message: impl Into<String>) -> Error {
    Error {
        code,
        message: message.into(),
    }
}
unsafe fn error(db: *mut c_void, code: i32) -> Error {
    // SAFETY: caller holds the global lease and a live database handle.
    failure(
        code,
        unsafe { CStr::from_ptr(sqlite3_errmsg(db)) }
            .to_string_lossy()
            .into_owned(),
    )
}
struct Statement(*mut c_void);
impl Drop for Statement {
    fn drop(&mut self) {
        unsafe {
            sqlite3_finalize(self.0);
        }
    }
}
struct Progress(*mut c_void);
impl Drop for Progress {
    fn drop(&mut self) {
        unsafe {
            sqlite3_progress_handler(self.0, 0, None, ptr::null_mut());
        }
    }
}
unsafe extern "C" fn progress(context: *mut c_void) -> c_int {
    // SAFETY: budget lives on the query stack until Progress unregisters this callback.
    let remaining = unsafe { &mut *context.cast::<u32>() };
    *remaining = remaining.saturating_sub(1);
    i32::from(*remaining == 0)
}

pub struct Connection(*mut c_void);
impl Connection {
    pub fn open(image: Option<&[u8]>) -> Result<Self> {
        if image.is_some_and(|b| b.len() > MAX_DATABASE_BYTES) {
            return Err(failure(18, "database snapshot exceeds 8 MiB"));
        }
        let handle = {
            let _lease = Lease::acquire();
            // SQLite's page-count pragmas are guest-controlled. Bound C heap
            // globally as well, including serialization and hostile queries.
            unsafe {
                sqlite3_hard_heap_limit64(32 << 20);
            }
            let mut db = ptr::null_mut();
            // SAFETY: output pointer is writable; SQLite retains only its own allocation.
            let rc = unsafe { sqlite3_open_v2(c":memory:".as_ptr(), &mut db, 2 | 4, ptr::null()) };
            if rc != 0 {
                let result = if db.is_null() {
                    failure(rc, "SQLite memory database allocation failed")
                } else {
                    unsafe { error(db, rc) }
                };
                if !db.is_null() {
                    unsafe {
                        sqlite3_close(db);
                    }
                }
                return Err(result);
            }
            if let Some(bytes) = image.filter(|b| !b.is_empty()) {
                // SQLite owns/frees its copied buffer, including deserialize failure,
                // with FREEONCLOSE|RESIZEABLE. No borrowed Rust slice is retained.
                let buffer = unsafe { sqlite3_malloc64(bytes.len() as u64) }.cast::<u8>();
                if buffer.is_null() {
                    unsafe {
                        sqlite3_close(db);
                    }
                    return Err(failure(7, "SQLite snapshot allocation failed"));
                }
                unsafe {
                    ptr::copy_nonoverlapping(bytes.as_ptr(), buffer, bytes.len());
                }
                let rc = unsafe {
                    sqlite3_deserialize(
                        db,
                        c"main".as_ptr(),
                        buffer,
                        bytes.len() as i64,
                        bytes.len() as i64,
                        1 | 2,
                    )
                };
                if rc != 0 {
                    let result = unsafe { error(db, rc) };
                    unsafe {
                        sqlite3_close(db);
                    }
                    return Err(result);
                }
            }
            unsafe {
                sqlite3_limit(db, 0, MAX_DATABASE_BYTES as i32);
                sqlite3_limit(db, 1, 1 << 20);
                sqlite3_limit(db, 2, 128); // columns
                sqlite3_limit(db, 3, 100); // expression depth
                sqlite3_limit(db, 4, 50); // compound SELECT terms
                sqlite3_limit(db, 5, 100_000); // prepared VM operations
                sqlite3_limit(db, 7, 0); // ATTACH cannot bypass snapshot ownership
                sqlite3_limit(db, 9, 999); // bound parameters
                sqlite3_limit(db, 10, 16); // trigger depth
            }
            db
        };
        let db = Self(handle);
        db.query("PRAGMA max_page_count=2048", &[])?;
        Ok(db)
    }
    pub fn autocommit(&self) -> bool {
        let _lease = Lease::acquire();
        unsafe { sqlite3_get_autocommit(self.0) != 0 }
    }
    /// Parse and validate one statement without executing it. SQLite owns the
    /// placeholder grammar, including quoted question marks and named indexes.
    pub fn statement_info(&self, sql: &str) -> Result<(usize, usize)> {
        if sql.len() > 1 << 20 {
            return Err(failure(18, "SQL text exceeds 1 MiB"));
        }
        let sql = CString::new(sql).map_err(|_| failure(21, "SQL contains a NUL"))?;
        let _lease = Lease::acquire();
        let statement = self.prepare(&sql)?;
        Ok(if statement.0.is_null() {
            (0, 0)
        } else {
            // SAFETY: prepare returned a live statement, protected by the lease.
            unsafe {
                (
                    sqlite3_bind_parameter_count(statement.0) as usize,
                    sqlite3_column_count(statement.0) as usize,
                )
            }
        })
    }
    // Caller holds the global lease throughout statement ownership.
    fn prepare(&self, sql: &CStr) -> Result<Statement> {
        let mut stmt = ptr::null_mut();
        let mut tail = ptr::null();
        let rc = unsafe { sqlite3_prepare_v2(self.0, sql.as_ptr(), -1, &mut stmt, &mut tail) };
        let statement = Statement(stmt);
        if rc != 0 {
            return Err(unsafe { error(self.0, rc) });
        }
        if stmt.is_null() {
            return Ok(statement);
        }
        // Reject a second executable statement before stepping the first. SQLite
        // parses trailing whitespace/comments, so valid comments remain accepted.
        if !tail.is_null() {
            let mut extra = ptr::null_mut();
            let rc = unsafe { sqlite3_prepare_v2(self.0, tail, -1, &mut extra, ptr::null_mut()) };
            let _extra = Statement(extra);
            if rc != 0 || !extra.is_null() {
                return Err(failure(21, "one SQL statement per call"));
            }
        }
        Ok(statement)
    }
    pub fn query(&self, sql: &str, bindings: &[Value]) -> Result<Rows> {
        if sql.len() > 1 << 20 {
            return Err(failure(18, "SQL text exceeds 1 MiB"));
        }
        let binding_bytes = bindings.iter().try_fold(0usize, |total, value| {
            total
                .checked_add(core::mem::size_of::<Value>())?
                .checked_add(match value {
                    Value::Text(value) => value.len(),
                    Value::Blob(value) => value.len(),
                    _ => 0,
                })
        });
        if bindings.len() > 32766 || binding_bytes.is_none_or(|n| n > MAX_DATABASE_BYTES) {
            return Err(failure(18, "SQL bindings exceed configured budget"));
        }
        let sql = CString::new(sql).map_err(|_| failure(21, "SQL contains a NUL"))?;
        let _lease = Lease::acquire();
        let mut budget = 500u32;
        unsafe {
            sqlite3_progress_handler(
                self.0,
                1000,
                Some(progress),
                (&mut budget as *mut u32).cast(),
            );
        }
        let _progress = Progress(self.0);
        let statement = self.prepare(&sql)?;
        let stmt = statement.0;
        if stmt.is_null() {
            return Ok(Rows {
                columns: Vec::new(),
                values: Vec::new(),
            });
        }
        if unsafe { sqlite3_bind_parameter_count(stmt) } as usize != bindings.len() {
            return Err(failure(25, "SQL binding count mismatch"));
        }
        for (i, value) in bindings.iter().enumerate() {
            let i = (i + 1) as i32;
            // SQLITE_STATIC: bindings are borrowed for this entire call and the
            // Statement guard finalizes before returning to the caller.
            let rc = unsafe {
                match value {
                    Value::Null => sqlite3_bind_null(stmt, i),
                    Value::Integer(v) => sqlite3_bind_int64(stmt, i, *v),
                    Value::Real(v) => sqlite3_bind_double(stmt, i, *v),
                    Value::Text(v) => {
                        sqlite3_bind_text(stmt, i, v.as_ptr().cast(), v.len() as i32, None)
                    }
                    Value::Blob(v) => {
                        sqlite3_bind_blob(stmt, i, v.as_ptr().cast(), v.len() as i32, None)
                    }
                }
            };
            if rc != 0 {
                return Err(unsafe { error(self.0, rc) });
            }
        }
        let count = unsafe { sqlite3_column_count(stmt) };
        let columns = (0..count)
            .map(|i| {
                let name = unsafe { sqlite3_column_name(stmt, i) };
                if name.is_null() {
                    return Err(failure(7, "SQLite column name allocation failed"));
                }
                Ok(unsafe { CStr::from_ptr(name) }
                    .to_string_lossy()
                    .into_owned())
            })
            .collect::<Result<Vec<_>>>()?;
        let mut values = Vec::new();
        let mut bytes = 0usize;
        loop {
            let rc = unsafe { sqlite3_step(stmt) };
            if rc == 101 {
                break;
            }
            if rc != 100 {
                return Err(unsafe { error(self.0, rc) });
            }
            if values.len() >= MAX_ROWS {
                return Err(failure(18, "cursor exceeds 10000 materialized rows"));
            }
            let mut row = Vec::new();
            for i in 0..count {
                bytes = bytes.saturating_add(core::mem::size_of::<Value>());
                if bytes > MAX_DATABASE_BYTES {
                    return Err(failure(18, "cursor cells exceed 8 MiB"));
                }
                let value = unsafe {
                    match sqlite3_column_type(stmt, i) {
                        1 => Value::Integer(sqlite3_column_int64(stmt, i)),
                        2 => Value::Real(sqlite3_column_double(stmt, i)),
                        3 | 4 => {
                            let text = sqlite3_column_type(stmt, i) == 3;
                            let pointer = if text {
                                sqlite3_column_text(stmt, i)
                            } else {
                                sqlite3_column_blob(stmt, i).cast()
                            };
                            let length = sqlite3_column_bytes(stmt, i) as usize;
                            if sqlite3_errcode(self.0) == 7 {
                                return Err(failure(
                                    7,
                                    "SQLite column conversion allocation failed",
                                ));
                            }
                            bytes = bytes.saturating_add(length);
                            if bytes > MAX_DATABASE_BYTES || (pointer.is_null() && length != 0) {
                                return Err(failure(18, "cursor data exceeds 8 MiB"));
                            }
                            let slice = if length == 0 {
                                &[]
                            } else {
                                core::slice::from_raw_parts(pointer, length)
                            };
                            if text {
                                Value::Text(String::from_utf8_lossy(slice).into_owned())
                            } else {
                                Value::Blob(slice.to_vec())
                            }
                        }
                        _ => Value::Null,
                    }
                };
                row.push(value);
            }
            values.push(row);
        }
        drop(statement);
        Ok(Rows { columns, values })
    }
    pub fn snapshot(&self) -> Result<Vec<u8>> {
        let _lease = Lease::acquire();
        let mut length = 0i64;
        let buffer = unsafe { sqlite3_serialize(self.0, c"main".as_ptr(), &mut length, 0) };
        if buffer.is_null() {
            return Err(failure(7, "SQLite snapshot unavailable/empty or allocation failed; schema initialization required"));
        }
        let result = if !(0..=MAX_DATABASE_BYTES as i64).contains(&length) {
            Err(failure(18, "snapshot exceeds 8 MiB"))
        } else {
            let mut bytes = Vec::new();
            match bytes.try_reserve_exact(length as usize) {
                Ok(()) => {
                    bytes.extend_from_slice(unsafe {
                        core::slice::from_raw_parts(buffer, length as usize)
                    });
                    Ok(bytes)
                }
                Err(_) => Err(failure(7, "snapshot copy allocation failed")),
            }
        };
        unsafe {
            sqlite3_free(buffer.cast());
        }
        result
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        let _lease = Lease::acquire();
        unsafe {
            sqlite3_close(self.0);
        }
    }
}
