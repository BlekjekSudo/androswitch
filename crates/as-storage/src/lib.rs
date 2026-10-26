#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;
mod preferences;
use alloc::{
    collections::BTreeMap,
    format,
    rc::Rc,
    string::{String, ToString},
    vec::Vec,
};
use as_platform::{
    fs::{self, FileSystem},
    PlatformError,
};
pub use as_sqlite::{Rows, Value as SqlValue};
pub use preferences::{decode_preferences, encode_preferences, Preference, Preferences};
use sha2::{Digest, Sha256};

fn error(code: &'static str, cause: impl Into<String>) -> PlatformError {
    fs::error(code, "persistent_storage", cause)
}
fn sql(e: as_sqlite::Error) -> PlatformError {
    error("SQLITE_ERROR", e.to_string())
}
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !matches!(name, "." | "..")
        && !name.ends_with(['.', ' '])
        && !name
            .chars()
            .any(|c| c.is_control() || "/\\:<>?*|".contains(c))
}
#[derive(Clone)]
pub struct Store {
    fs: Rc<dyn FileSystem>,
    package: String,
}
impl Store {
    pub fn new(fs: Rc<dyn FileSystem>, package: &str) -> Result<Self, PlatformError> {
        if !as_platform::vfs::valid_package(package) {
            return Err(error("INVALID_PACKAGE", package));
        }
        Ok(Self {
            fs,
            package: package.into(),
        })
    }
    fn path(&self, kind: &str, name: &str) -> Result<String, PlatformError> {
        if !valid_name(name) {
            return Err(error("INVALID_STORAGE_NAME", name));
        }
        let directory = format!("userdata/{}/{kind}", self.package);
        if self.fs.stat(&directory)?.is_some() {
            for entry in self.fs.list(&directory)? {
                if entry.name != name && entry.name.eq_ignore_ascii_case(name) {
                    return Err(error("STORAGE_CASE_COLLISION", name));
                }
            }
        }
        Ok(format!("{directory}/{name}"))
    }
    fn journal(&self, path: &str) -> String {
        use core::fmt::Write;
        let mut hash = String::with_capacity(64);
        for byte in Sha256::digest(path.as_bytes()) {
            write!(&mut hash, "{byte:02x}").expect("String write");
        }
        format!("userdata/{}/.androswitch-atomic/{hash}", self.package)
    }
    fn recover(&self, path: &str) -> Result<(), PlatformError> {
        let backup = format!("{}/previous", self.journal(path));
        if self.fs.stat(&backup)?.is_some() && self.fs.stat(path)?.is_none() {
            self.fs.rename_new(&backup, path)?;
        }
        Ok(())
    }
    fn read(&self, path: &str, limit: usize) -> Result<Option<Vec<u8>>, PlatformError> {
        self.recover(path)?;
        if self.fs.stat(path)?.is_none() {
            return Ok(None);
        }
        fs::read_all(self.fs.as_ref(), path, limit).map(Some)
    }
    fn publish(&self, path: &str, bytes: &[u8]) -> Result<(), PlatformError> {
        self.recover(path)?;
        let journal = self.journal(path);
        fs::mkdirs(self.fs.as_ref(), &journal)?;
        if let Some((parent, _)) = path.rsplit_once('/') {
            fs::mkdirs(self.fs.as_ref(), parent)?;
        }
        let pending = format!("{journal}/pending");
        let backup = format!("{journal}/previous");
        if self.fs.stat(&backup)?.is_some() {
            self.fs.remove(&backup, false)?;
        }
        if let Some(mut writer) = self.fs.begin_write(&pending)? {
            for chunk in bytes.chunks(64 << 10) {
                if writer.write(chunk)? != chunk.len() {
                    return Err(error("SHORT_WRITE", &pending));
                }
            }
            writer.finish()?;
        } else {
            fs::write_all(self.fs.as_ref(), &pending, bytes)?;
        }
        let existed = self.fs.stat(path)?.is_some();
        if existed {
            self.fs.rename_new(path, &backup)?;
        }
        if let Err(cause) = self.fs.rename_new(&pending, path) {
            if self.fs.stat(path)?.is_some() {
                return Err(error("PUBLISH_INDETERMINATE", cause.to_string()));
            }
            if existed
                && self.fs.stat(path)?.is_none()
                && self.fs.rename_new(&backup, path).is_err()
            {
                return Err(error("PUBLISH_INDETERMINATE", cause.to_string()));
            }
            return Err(cause);
        }
        // The new target is durable. A leftover backup can be removed on the
        // next commit; its presence must not turn a committed save into failure.
        if existed {
            let _ = self.fs.remove(&backup, false);
        }
        Ok(())
    }
    pub fn preferences(&self, name: &str) -> Result<Preferences, PlatformError> {
        if !valid_name(name) {
            return Err(error("INVALID_STORAGE_NAME", name));
        }
        let path = self.path("shared_prefs", &format!("{name}.xml"))?;
        match self.read(&path, 1 << 20)? {
            Some(bytes) => decode_preferences(
                core::str::from_utf8(&bytes).map_err(|_| error("PREFERENCES_FORMAT", "UTF-8"))?,
            ),
            None => Ok(Preferences::new()),
        }
    }
    pub fn save_preferences(&self, name: &str, values: &Preferences) -> Result<(), PlatformError> {
        if !valid_name(name) {
            return Err(error("INVALID_STORAGE_NAME", name));
        }
        let path = self.path("shared_prefs", &format!("{name}.xml"))?;
        let xml = encode_preferences(values)?;
        self.publish(&path, xml.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeSet;
    use as_platform::host_fs::HostFileSystem;
    fn store(label: &str) -> Store {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../target/storage-tests/{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        Store::new(
            Rc::new(HostFileSystem::new(path).unwrap()),
            "org.fixture.storage",
        )
        .unwrap()
    }
    #[test]
    fn android_xml_preferences_preserve_types_unicode_sets_and_reject_ambiguous_input() {
        let mut values = Preferences::new();
        values.insert(
            "text & \"key\"".into(),
            Preference::String("雪 <value> & 'quoted'".into()),
        );
        values.insert("int".into(), Preference::Int(-7));
        values.insert("long".into(), Preference::Long(i64::MAX));
        values.insert("float".into(), Preference::Float((-0.0f32).to_bits()));
        values.insert("bool".into(), Preference::Boolean(true));
        values.insert(
            "set".into(),
            Preference::StringSet(
                ["one".into(), "雪&".into()]
                    .into_iter()
                    .collect::<BTreeSet<_>>(),
            ),
        );
        let encoded = encode_preferences(&values).unwrap();
        assert_eq!(decode_preferences(&encoded).unwrap(), values);
        let store = store("prefs");
        store.save_preferences("settings", &values).unwrap();
        assert_eq!(store.preferences("settings").unwrap(), values);
        assert!(store.preferences("../escape").is_err());
        assert!(decode_preferences("<!DOCTYPE map><map/>").is_err());
        assert!(decode_preferences("<map><int name='x' name='y' value='1'/></map>").is_err());
        assert!(decode_preferences(
            "<map><int name='x' value='1'/><int name='x' value='2'/></map>"
        )
        .is_err());
        assert!(decode_preferences("<map><string name='x'></map></string>").is_err());
    }
    #[test]
    fn sqlite_notes_commit_rollback_restart_and_real_sqlite_header() {
        let store = store("database");
        let mut db = Databases::new(store.clone());
        db.query(
            "notes.db",
            "CREATE TABLE notes(id INTEGER PRIMARY KEY, text TEXT)",
            &[],
        )
        .unwrap();
        db.query(
            "notes.db",
            "INSERT INTO notes(text) VALUES(?)",
            &[SqlValue::Text("persistent 雪".into())],
        )
        .unwrap();
        db.query("notes.db", "BEGIN", &[]).unwrap();
        assert!(db.in_transaction("notes.db"));
        db.query("notes.db", "DELETE FROM notes", &[]).unwrap();
        db.close_all(); // An uncommitted transaction must not enter the durable snapshot.
        let mut reopened = Databases::new(store.clone());
        assert_eq!(
            reopened
                .query("notes.db", "SELECT text FROM notes", &[])
                .unwrap()
                .values[0][0],
            SqlValue::Text("persistent 雪".into())
        );
        reopened.query("notes.db", "BEGIN", &[]).unwrap();
        reopened
            .query(
                "notes.db",
                "UPDATE notes SET text=?",
                &[SqlValue::Text("committed".into())],
            )
            .unwrap();
        reopened.query("notes.db", "COMMIT", &[]).unwrap();
        reopened.close_all();
        assert_eq!(
            reopened
                .query("notes.db", "SELECT text FROM notes", &[])
                .unwrap()
                .values[0][0],
            SqlValue::Text("committed".into())
        );
        let path = store.path("databases", "notes.db").unwrap();
        assert!(store
            .read(&path, 8 << 20)
            .unwrap()
            .unwrap()
            .starts_with(b"SQLite format 3\0"));
    }
}
#[cfg(test)]
extern crate std;
#[cfg(test)]
mod faults;
struct Database {
    connection: as_sqlite::Connection,
    committed: Vec<u8>,
    poisoned: bool,
}
pub struct Databases {
    store: Store,
    open: BTreeMap<String, Database>,
}
impl Databases {
    pub fn new(store: Store) -> Self {
        Self {
            store,
            open: BTreeMap::new(),
        }
    }
    pub fn open(&mut self, name: &str) -> Result<(), PlatformError> {
        if self.open.contains_key(name) {
            return Ok(());
        }
        if self.open.len() >= 4 {
            return Err(error(
                "SQLITE_HANDLE_BUDGET",
                "four open database files per app",
            ));
        }
        let path = self.store.path("databases", name)?;
        let image = self.store.read(&path, as_sqlite::MAX_DATABASE_BYTES)?;
        let connection = as_sqlite::Connection::open(image.as_deref()).map_err(sql)?;
        if image.is_none() {
            connection
                .query("PRAGMA user_version=0", &[])
                .map_err(sql)?;
        }
        let committed = connection.snapshot().map_err(sql)?;
        if image.is_none() {
            self.store.publish(&path, &committed)?;
        }
        self.open.insert(
            name.into(),
            Database {
                connection,
                committed,
                poisoned: false,
            },
        );
        Ok(())
    }
    pub fn query(
        &mut self,
        name: &str,
        statement: &str,
        args: &[SqlValue],
    ) -> Result<Rows, PlatformError> {
        self.open(name)?;
        let db = self.open.get_mut(name).expect("opened database");
        if db.poisoned {
            return Err(error(
                "SQLITE_IO_ERROR",
                "database requires reopening after indeterminate publication",
            ));
        }
        let rows = db.connection.query(statement, args).map_err(sql)?;
        if db.connection.autocommit() {
            let snapshot = db.connection.snapshot().map_err(sql)?;
            if snapshot != db.committed {
                let path = self.store.path("databases", name)?;
                if let Err(cause) = self.store.publish(&path, &snapshot) {
                    db.poisoned = cause.code == "PUBLISH_INDETERMINATE";
                    db.connection =
                        as_sqlite::Connection::open(Some(&db.committed)).map_err(sql)?;
                    return Err(cause);
                }
                db.committed = snapshot;
            }
        }
        Ok(rows)
    }
    pub fn statement_info(
        &mut self,
        name: &str,
        statement: &str,
    ) -> Result<(usize, usize), PlatformError> {
        self.open(name)?;
        let db = self.open.get(name).expect("opened database");
        if db.poisoned {
            return Err(error("SQLITE_IO_ERROR", "database requires reopening"));
        }
        db.connection.statement_info(statement).map_err(sql)
    }
    pub fn in_transaction(&self, name: &str) -> bool {
        self.open
            .get(name)
            .is_some_and(|db| !db.connection.autocommit())
    }
    pub fn close(&mut self, name: &str) {
        self.open.remove(name);
    }
    pub fn close_all(&mut self) {
        self.open.clear();
    }
}
