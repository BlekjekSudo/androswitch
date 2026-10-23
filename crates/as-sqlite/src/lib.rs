//! Native SQLite with bounded in-memory databases and explicit snapshot I/O.
//! The runtime storage adapter owns durable publication; SQLite never spawns a
//! process or forwards a guest/Linux filesystem operation to Horizon.
#![no_std]
extern crate alloc;
use alloc::{string::String, vec::Vec};
#[allow(unsafe_code)] // Documented C API ownership, pointer lengths and global lease.
mod ffi;
pub use ffi::Connection;

pub const MAX_DATABASE_BYTES: usize = 8 << 20;
pub const MAX_ROWS: usize = 10_000;
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}
#[derive(Debug, Clone)]
pub struct Rows {
    pub columns: Vec<String>,
    pub values: Vec<Vec<Value>>,
}
#[derive(Debug, Clone)]
pub struct Error {
    pub code: i32,
    pub message: String,
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "SQLITE_{}: {}", self.code, self.message)
    }
}
pub type Result<T> = core::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sqlite_executes_bindings_transactions_and_roundtrips_its_real_file_format() {
        let db = Connection::open(None).unwrap();
        assert!(db.snapshot().unwrap().starts_with(b"SQLite format 3\0"));
        assert!(db.query("SELECT zeroblob(8388609)", &[]).is_err());
        db.query(
            "CREATE TABLE notes(id INTEGER PRIMARY KEY, text TEXT UNIQUE, n REAL, data BLOB)",
            &[],
        )
        .unwrap();
        db.query(
            "INSERT INTO notes(text,n,data) VALUES(?,?,?)",
            &[
                Value::Text("雪 & <note>".into()),
                Value::Real(1.25),
                Value::Blob(alloc::vec![0, 1, 255]),
            ],
        )
        .unwrap();
        db.query("BEGIN", &[]).unwrap();
        db.query("DELETE FROM notes", &[]).unwrap();
        db.query("ROLLBACK", &[]).unwrap();
        let image = db.snapshot().unwrap();
        assert!(image.starts_with(b"SQLite format 3\0"));
        let reopened = Connection::open(Some(&image)).unwrap();
        let rows = reopened
            .query("SELECT text,n,data FROM notes", &[])
            .unwrap();
        assert_eq!(
            rows.values[0],
            alloc::vec![
                Value::Text("雪 & <note>".into()),
                Value::Real(1.25),
                Value::Blob(alloc::vec![0, 1, 255])
            ]
        );
        assert!(reopened
            .query(
                "INSERT INTO notes(text) VALUES(?)",
                &[Value::Text("雪 & <note>".into())]
            )
            .is_err());
        assert_eq!(
            reopened
                .query("PRAGMA integrity_check", &[])
                .unwrap()
                .values[0][0],
            Value::Text("ok".into())
        );
        assert!(reopened.query("SELECT ?", &[]).is_err());
        assert!(reopened.query("SELECT 1; DELETE FROM notes", &[]).is_err());
    }
    #[test]
    fn invalid_snapshots_and_unbounded_queries_have_named_errors() {
        assert!(Connection::open(Some(&alloc::vec![0; MAX_DATABASE_BYTES+1])).is_err());
        let db = Connection::open(None).unwrap();
        assert!(db.query("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n) SELECT sum(x) FROM n", &[]).is_err());
        assert!(db.autocommit());
    }
}
