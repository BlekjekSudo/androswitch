//! Fail before and after each durable mutation; recovery may expose only a
//! complete old or complete new snapshot, never a torn preferences/database.
use super::*;
use as_platform::{
    fs::{Entry, Metadata},
    host_fs::HostFileSystem,
};
use core::cell::Cell;
#[derive(Clone)]
struct FaultFs {
    inner: HostFileSystem,
    fail: Rc<Cell<Option<(usize, bool)>>>,
    count: Rc<Cell<usize>>,
}
impl FaultFs {
    fn operation<T>(
        &self,
        mutate: impl FnOnce() -> Result<T, PlatformError>,
    ) -> Result<T, PlatformError> {
        let n = self.count.get();
        self.count.set(n + 1);
        if self.fail.get() == Some((n, false)) {
            return Err(error("INJECTED_IO", "before mutation"));
        }
        let result = mutate()?;
        if self.fail.get() == Some((n, true)) {
            return Err(error("INJECTED_IO", "after mutation"));
        }
        Ok(result)
    }
}
impl FileSystem for FaultFs {
    fn stat(&self, p: &str) -> Result<Option<Metadata>, PlatformError> {
        self.inner.stat(p)
    }
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, PlatformError> {
        self.inner.read_at(p, o, b)
    }
    fn list(&self, p: &str) -> Result<Vec<Entry>, PlatformError> {
        self.inner.list(p)
    }
    fn write_at(&self, p: &str, o: u64, b: &[u8], t: bool) -> Result<usize, PlatformError> {
        self.operation(|| self.inner.write_at(p, o, b, t))
    }
    fn create_dir(&self, p: &str, e: bool) -> Result<bool, PlatformError> {
        self.operation(|| self.inner.create_dir(p, e))
    }
    fn rename_new(&self, a: &str, b: &str) -> Result<(), PlatformError> {
        self.operation(|| self.inner.rename_new(a, b))
    }
    fn remove(&self, p: &str, r: bool) -> Result<(), PlatformError> {
        self.operation(|| self.inner.remove(p, r))
    }
}
#[test]
fn atomic_preferences_and_sqlite_survive_before_and_after_each_mutation() {
    for database in [false, true] {
        for after in [false, true] {
            for boundary in 0..18 {
                let fs = FaultFs {
                    inner: {
                        // Every fixture gets an independent rooted host filesystem.
                        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
                            "../../target/storage-faults/{}-{}-{database}-{after}-{boundary}",
                            std::process::id(),
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_nanos()
                        ));
                        HostFileSystem::new(p).unwrap()
                    },
                    fail: Rc::new(Cell::new(None)),
                    count: Rc::new(Cell::new(0)),
                };
                let store = Store::new(Rc::new(fs.clone()), "org.fixture.storage").unwrap();
                let mut old = Preferences::new();
                old.insert("text".into(), Preference::String("old 雪".into()));
                let mut new = old.clone();
                new.insert("text".into(), Preference::String("new & <text>".into()));
                if database {
                    let mut db = Databases::new(store.clone());
                    db.query("notes.db", "CREATE TABLE notes(text TEXT)", &[])
                        .unwrap();
                    db.query("notes.db", "INSERT INTO notes VALUES('old')", &[])
                        .unwrap();
                    fs.count.set(0);
                    fs.fail.set(Some((boundary, after)));
                    let result = db.query("notes.db", "UPDATE notes SET text='new'", &[]);
                    fs.fail.set(None);
                    drop(db);
                    let rows = Databases::new(store)
                        .query("notes.db", "SELECT text FROM notes", &[])
                        .unwrap();
                    let value = &rows.values[0][0];
                    assert!(
                        value == &SqlValue::Text("old".into())
                            || value == &SqlValue::Text("new".into())
                    );
                    if result.is_ok() {
                        assert_eq!(value, &SqlValue::Text("new".into()));
                    }
                } else {
                    store.save_preferences("settings", &old).unwrap();
                    fs.count.set(0);
                    fs.fail.set(Some((boundary, after)));
                    let result = store.save_preferences("settings", &new);
                    fs.fail.set(None);
                    let read = store.preferences("settings").unwrap();
                    assert!(read == old || read == new);
                    if result.is_ok() {
                        assert_eq!(read, new);
                    }
                }
            }
        }
    }
}
