//! Android database contracts over the real, bounded native SQLite engine.
use super::*;
use as_storage::{Databases, Rows, SqlValue, Store};
#[derive(Clone)]
struct Handle {
    name: String,
    closed: bool,
    successful: bool,
}
#[derive(Clone)]
struct Cursor {
    rows: Rows,
    position: i32,
    closed: bool,
}
#[derive(Clone)]
struct Helper {
    name: String,
    version: i32,
    database: Option<ObjRef>,
    opening: bool,
}
pub(super) struct DatabaseService {
    databases: RefCell<Databases>,
    handles: RefCell<BTreeMap<ObjRef, Handle>>,
    cursors: RefCell<BTreeMap<ObjRef, Cursor>>,
    helpers: RefCell<BTreeMap<ObjRef, Helper>>,
}
fn exception(class: &str, cause: impl Into<String>) -> VmExit {
    Throwable::new(class, cause).into()
}
fn sql_error(e: as_platform::PlatformError) -> VmExit {
    exception("Landroid/database/sqlite/SQLiteException;", e.to_string())
}
fn array(vm: &Engine, value: &Value) -> Result<Vec<Value>> {
    if matches!(value, Value::Null) {
        return Ok(vec![]);
    }
    Ok(vm.heap.as_array_elems(obj(value)?)?.0.to_vec())
}
fn bindings(vm: &Engine, values: &Value) -> Result<Vec<SqlValue>> {
    array(vm, values)?
        .iter()
        .map(|v| {
            Ok(match v {
                Value::Null => SqlValue::Null,
                Value::Obj(r) if vm.heap.class_of(*r)? == "[B" => SqlValue::Blob(
                    vm.heap
                        .as_array_elems(*r)?
                        .0
                        .iter()
                        .map(|v| v.as_int().map(|n| n as u8))
                        .collect::<core::result::Result<Vec<_>, _>>()?,
                ),
                Value::Obj(r) if vm.heap.as_str(*r).is_ok() => {
                    SqlValue::Text(vm.heap.as_str(*r)?.into())
                }
                Value::Obj(r) => match vm.heap.get_field(*r, "value")? {
                    Value::Int(v) => SqlValue::Integer(v as i64),
                    Value::Long(v) => SqlValue::Integer(v),
                    Value::Float(v) => SqlValue::Real(v as f64),
                    Value::Double(v) => SqlValue::Real(v),
                    _ => {
                        return Err(exception(
                            "Ljava/lang/IllegalArgumentException;",
                            "SQL binding type",
                        ))
                    }
                },
                _ => {
                    return Err(exception(
                        "Ljava/lang/IllegalArgumentException;",
                        "SQL binding must be boxed",
                    ))
                }
            })
        })
        .collect()
}
impl DatabaseService {
    pub(super) fn new(store: Store) -> Self {
        Self {
            databases: RefCell::new(Databases::new(store)),
            handles: Default::default(),
            cursors: Default::default(),
            helpers: Default::default(),
        }
    }
    pub(super) fn attach(&self, vm: &Engine, class: &str, _r: ObjRef) -> bool {
        vm.cp
            .is_subtype(class, "Landroid/database/sqlite/SQLiteOpenHelper;")
    }
    fn open(&self, vm: &mut Engine, name: &str) -> Result<Value> {
        // One connection per filename. Sharing multiple Java handles requires
        // Android connection-pool ownership, which this subset does not offer.
        if self
            .handles
            .borrow()
            .values()
            .any(|h| !h.closed && h.name == name)
        {
            return Err(error(
                "API_UNAVAILABLE",
                "concurrent SQLiteDatabase handles for one file",
            ));
        }
        if self.handles.borrow().len() >= 64 {
            return Err(error("SQLITE_HANDLE_BUDGET", "64 handles"));
        }
        self.databases.borrow_mut().open(name).map_err(sql_error)?;
        let r = vm
            .heap
            .alloc_instance("Landroid/database/sqlite/SQLiteDatabase;".into(), vec![])?;
        self.handles.borrow_mut().insert(
            r,
            Handle {
                name: name.into(),
                closed: false,
                successful: false,
            },
        );
        Ok(Value::Obj(r))
    }
    fn query(&self, name: &str, sql: &str, args: &[SqlValue]) -> Result<Rows> {
        self.databases
            .borrow_mut()
            .query(name, sql, args)
            .map_err(sql_error)
    }
    fn helper_open(&self, android: &Android, vm: &mut Engine, r: ObjRef) -> Result<Value> {
        let helper = self
            .helpers
            .borrow()
            .get(&r)
            .cloned()
            .ok_or_else(|| error("SQLITE_HELPER_STATE", "constructor not called"))?;
        if helper.opening {
            return Err(exception(
                "Ljava/lang/IllegalStateException;",
                "recursive getDatabase",
            ));
        }
        if let Some(db) = helper.database {
            if self.handles.borrow().get(&db).is_some_and(|h| !h.closed) {
                return Ok(Value::Obj(db));
            }
        }
        self.helpers
            .borrow_mut()
            .get_mut(&r)
            .expect("helper")
            .opening = true;
        let result = (|| {
            let db = obj(&self.open(vm, &helper.name)?)?;
            self.helpers
                .borrow_mut()
                .get_mut(&r)
                .expect("helper")
                .database = Some(db);
            // Root through helper before callbacks, including callbacks that GC.
            self.optional_callback(
                android,
                vm,
                r,
                "onConfigure",
                "(Landroid/database/sqlite/SQLiteDatabase;)V",
                &[Value::Obj(db)],
            )?;
            let version =
                self.query(&helper.name, "PRAGMA user_version", &[])?.values[0][0].clone();
            let SqlValue::Integer(old) = version else {
                return Err(error("SQLITE_ERROR", "user_version type"));
            };
            if old != helper.version as i64 {
                self.query(&helper.name, "BEGIN EXCLUSIVE", &[])?;
                let change = (|| {
                    if old == 0 {
                        android.callback(
                            vm,
                            r,
                            "onCreate",
                            "(Landroid/database/sqlite/SQLiteDatabase;)V",
                            &[Value::Obj(db)],
                        )?;
                    } else {
                        let method = if old < helper.version as i64 {
                            "onUpgrade"
                        } else {
                            "onDowngrade"
                        };
                        if vm
                            .cp
                            .resolve_method(
                                vm.heap.class_of(r)?.to_string().as_str(),
                                method,
                                "(Landroid/database/sqlite/SQLiteDatabase;II)V",
                            )
                            .is_none()
                        {
                            return Err(exception(
                                "Landroid/database/sqlite/SQLiteException;",
                                "database version callback unavailable",
                            ));
                        }
                        android.callback(
                            vm,
                            r,
                            method,
                            "(Landroid/database/sqlite/SQLiteDatabase;II)V",
                            &[
                                Value::Obj(db),
                                Value::Int(old as i32),
                                Value::Int(helper.version),
                            ],
                        )?;
                    }
                    self.query(
                        &helper.name,
                        &format!("PRAGMA user_version={}", helper.version),
                        &[],
                    )?;
                    self.query(&helper.name, "COMMIT", &[])?;
                    Ok(())
                })();
                if change.is_err() && self.databases.borrow().in_transaction(&helper.name) {
                    self.query(&helper.name, "ROLLBACK", &[])?;
                }
                change?;
            }
            self.optional_callback(
                android,
                vm,
                r,
                "onOpen",
                "(Landroid/database/sqlite/SQLiteDatabase;)V",
                &[Value::Obj(db)],
            )?;
            Ok(Value::Obj(db))
        })();
        self.helpers
            .borrow_mut()
            .get_mut(&r)
            .expect("helper")
            .opening = false;
        if result.is_err() {
            if let Some(db) = self
                .helpers
                .borrow_mut()
                .get_mut(&r)
                .expect("helper")
                .database
                .take()
            {
                self.close(db);
            }
        }
        result
    }
    fn optional_callback(
        &self,
        android: &Android,
        vm: &mut Engine,
        r: ObjRef,
        name: &str,
        sig: &str,
        args: &[Value],
    ) -> Result<()> {
        let class = vm.heap.class_of(r)?.to_string();
        if vm.cp.resolve_method(&class, name, sig).is_some() {
            android.callback(vm, r, name, sig, args)?;
        }
        Ok(()) // API26's base onConfigure/onOpen really are empty.
    }
    fn close(&self, r: ObjRef) {
        if let Some(h) = self.handles.borrow_mut().get_mut(&r) {
            self.databases.borrow_mut().close(&h.name);
            h.closed = true;
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn call(
        &self,
        android: &Android,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        r: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if vm
            .cp
            .is_subtype(vm.heap.class_of(r)?, "Landroid/content/Context;")
        {
            if name == "openOrCreateDatabase" && sig == "(Ljava/lang/String;ILandroid/database/sqlite/SQLiteDatabase$CursorFactory;)Landroid/database/sqlite/SQLiteDatabase;" {
                if int(args, 1)? != 0 || args[2] != Value::Null { return Err(error("API_UNAVAILABLE", "database mode/cursor factory")); }
                return self.open(vm, &text(vm, &args[0])?).map(Some);
            }
            if name == "getDatabasePath" && sig == "(Ljava/lang/String;)Ljava/io/File;" {
                let filename = text(vm, &args[0])?;
                if !as_storage::valid_name(&filename) {
                    return Err(exception(
                        "Ljava/lang/IllegalArgumentException;",
                        "database filename",
                    ));
                }
                let file = shell(vm, "Ljava/io/File;")?;
                let path = string(
                    vm,
                    format!(
                        "/data/user/0/{}/databases/{filename}",
                        android.apk.manifest.package
                    ),
                )?;
                vm.invoke_instance(file, "<init>", "(Ljava/lang/String;)V", &[path])?;
                return Ok(Some(Value::Obj(file)));
            }
        }
        if class == "Landroid/database/sqlite/SQLiteOpenHelper;"
            && matches!(name, "onOpen" | "onConfigure")
            && sig == "(Landroid/database/sqlite/SQLiteDatabase;)V"
        {
            return Ok(Some(Value::Null));
        }
        if class == "Landroid/database/sqlite/SQLiteOpenHelper;" && name == "<init>" && sig == "(Landroid/content/Context;Ljava/lang/String;Landroid/database/sqlite/SQLiteDatabase$CursorFactory;I)V" {
            obj(&args[0])?;
            if args[1] == Value::Null || args[2] != Value::Null { return Err(error("API_UNAVAILABLE", "memory helper/custom cursor factory")); }
            let version = int(args, 3)?;
            if version < 1 { return Err(exception("Ljava/lang/IllegalArgumentException;", "version must be positive")); }
            self.helpers.borrow_mut().insert(r, Helper { name: text(vm, &args[1])?, version, database: None, opening: false });
            return Ok(Some(Value::Null));
        }
        if self.helpers.borrow().contains_key(&r) {
            match (name, sig) {
                (
                    "getWritableDatabase" | "getReadableDatabase",
                    "()Landroid/database/sqlite/SQLiteDatabase;",
                ) => return self.helper_open(android, vm, r).map(Some),
                ("close", "()V") => {
                    let helper = self.helpers.borrow().get(&r).cloned().expect("helper");
                    if helper.opening {
                        return Err(exception(
                            "Ljava/lang/IllegalStateException;",
                            "close during initialization",
                        ));
                    }
                    if let Some(db) = helper.database {
                        self.close(db);
                    }
                    return Ok(Some(Value::Null));
                }
                _ => return Ok(None),
            }
        }
        let handle = self.handles.borrow().get(&r).cloned();
        if let Some(handle) = handle {
            if name == "isOpen" && sig == "()Z" {
                return Ok(Some(Value::Int((!handle.closed) as i32)));
            }
            if name == "close" && sig == "()V" {
                self.close(r);
                return Ok(Some(Value::Null));
            }
            if handle.closed {
                return Err(exception(
                    "Ljava/lang/IllegalStateException;",
                    "database is closed",
                ));
            }
            let file = &handle.name;
            let value = match (name, sig) {
                (
                    "execSQL",
                    "(Ljava/lang/String;)V" | "(Ljava/lang/String;[Ljava/lang/Object;)V",
                ) => {
                    let sql = text(vm, &args[0])?;
                    let bind = if args.len() == 2 {
                        bindings(vm, &args[1])?
                    } else {
                        vec![]
                    };
                    self.query(file, &sql, &bind)?;
                    Value::Null
                }
                (
                    "rawQuery",
                    "(Ljava/lang/String;[Ljava/lang/String;)Landroid/database/Cursor;",
                ) => {
                    if self.cursors.borrow().len() >= 64 {
                        return Err(error("SQLITE_CURSOR_BUDGET", "64 cursors"));
                    }
                    let sql = text(vm, &args[0])?;
                    let bind = bindings(vm, &args[1])?;
                    let rows = self.query(file, &sql, &bind)?;
                    let bytes = |rows: &Rows| {
                        rows.columns
                            .iter()
                            .map(|c| c.len() + core::mem::size_of::<String>())
                            .sum::<usize>()
                            + rows
                                .values
                                .iter()
                                .flatten()
                                .map(|v| {
                                    core::mem::size_of::<SqlValue>()
                                        + match v {
                                            SqlValue::Text(v) => v.len(),
                                            SqlValue::Blob(v) => v.len(),
                                            _ => 0,
                                        }
                                })
                                .sum::<usize>()
                    };
                    let active = self
                        .cursors
                        .borrow()
                        .values()
                        .map(|c| bytes(&c.rows))
                        .sum::<usize>();
                    if active.saturating_add(bytes(&rows)) > 8 << 20 {
                        return Err(error(
                            "SQLITE_CURSOR_BUDGET",
                            "8 MiB total materialized cursors",
                        ));
                    }
                    let cursor = vm
                        .heap
                        .alloc_instance("Landroid/database/MatrixCursor;".into(), vec![])?;
                    self.cursors.borrow_mut().insert(
                        cursor,
                        Cursor {
                            rows,
                            position: -1,
                            closed: false,
                        },
                    );
                    Value::Obj(cursor)
                }
                ("getVersion", "()I") => {
                    let rows = self.query(file, "PRAGMA user_version", &[])?;
                    let SqlValue::Integer(v) = rows.values[0][0] else {
                        return Err(error("SQLITE_ERROR", "user_version"));
                    };
                    Value::Int(v as i32)
                }
                ("setVersion", "(I)V") => {
                    self.query(file, &format!("PRAGMA user_version={}", int(args, 0)?), &[])?;
                    Value::Null
                }
                ("inTransaction", "()Z") => {
                    Value::Int(self.databases.borrow().in_transaction(file) as i32)
                }
                ("beginTransaction" | "beginTransactionNonExclusive", "()V") => {
                    if self.databases.borrow().in_transaction(file) {
                        return Err(error("API_UNAVAILABLE", "nested Android transactions"));
                    }
                    self.query(file, "BEGIN EXCLUSIVE", &[])?;
                    self.handles
                        .borrow_mut()
                        .get_mut(&r)
                        .expect("handle")
                        .successful = false;
                    Value::Null
                }
                ("setTransactionSuccessful", "()V") => {
                    if !self.databases.borrow().in_transaction(file) || handle.successful {
                        return Err(exception(
                            "Ljava/lang/IllegalStateException;",
                            "transaction state",
                        ));
                    }
                    self.handles
                        .borrow_mut()
                        .get_mut(&r)
                        .expect("handle")
                        .successful = true;
                    Value::Null
                }
                ("endTransaction", "()V") => {
                    if !self.databases.borrow().in_transaction(file) {
                        return Err(exception(
                            "Ljava/lang/IllegalStateException;",
                            "no transaction",
                        ));
                    }
                    self.query(
                        file,
                        if handle.successful {
                            "COMMIT"
                        } else {
                            "ROLLBACK"
                        },
                        &[],
                    )?;
                    Value::Null
                }
                _ => return Ok(None),
            };
            return Ok(Some(value));
        }
        let cursor = self.cursors.borrow().get(&r).cloned();
        if let Some(mut cursor) = cursor {
            if name == "isClosed" && sig == "()Z" {
                return Ok(Some(Value::Int(cursor.closed as i32)));
            }
            if name == "close" && sig == "()V" {
                cursor.closed = true;
                cursor.rows.values.clear();
                self.cursors.borrow_mut().insert(r, cursor);
                return Ok(Some(Value::Null));
            }
            if cursor.closed {
                return Err(exception(
                    "Ljava/lang/IllegalStateException;",
                    "cursor closed",
                ));
            }
            let value = match (name, sig) {
                ("getCount", "()I") => Value::Int(cursor.rows.values.len() as i32),
                ("getPosition", "()I") => Value::Int(cursor.position),
                ("getColumnCount", "()I") => Value::Int(cursor.rows.columns.len() as i32),
                ("getColumnIndex" | "getColumnIndexOrThrow", "(Ljava/lang/String;)I") => {
                    let column = text(vm, &args[0])?;
                    let index = cursor
                        .rows
                        .columns
                        .iter()
                        .position(|v| v.eq_ignore_ascii_case(&column));
                    if name == "getColumnIndexOrThrow" && index.is_none() {
                        return Err(exception("Ljava/lang/IllegalArgumentException;", column));
                    }
                    Value::Int(index.map(|v| v as i32).unwrap_or(-1))
                }
                ("moveToFirst" | "moveToNext", "()Z") | ("moveToPosition", "(I)Z") => {
                    let desired = match name {
                        "moveToFirst" => 0,
                        "moveToNext" => cursor.position.saturating_add(1),
                        _ => int(args, 0)?,
                    };
                    let ok = desired >= 0 && (desired as usize) < cursor.rows.values.len();
                    cursor.position = desired.clamp(-1, cursor.rows.values.len() as i32);
                    self.cursors.borrow_mut().insert(r, cursor);
                    Value::Int(ok as i32)
                }
                (
                    "getString" | "getInt" | "getLong" | "getFloat" | "getDouble" | "getBlob"
                    | "isNull" | "getType",
                    _,
                ) => {
                    let row = cursor
                        .rows
                        .values
                        .get(cursor.position as usize)
                        .ok_or_else(|| {
                            exception(
                                "Landroid/database/CursorIndexOutOfBoundsException;",
                                "cursor position",
                            )
                        })?;
                    let cell = row.get(int(args, 0)? as usize).ok_or_else(|| {
                        exception(
                            "Landroid/database/CursorIndexOutOfBoundsException;",
                            "column index",
                        )
                    })?;
                    match name {
                        "isNull" => Value::Int(matches!(cell, SqlValue::Null) as i32),
                        "getType" => Value::Int(match cell {
                            SqlValue::Null => 0,
                            SqlValue::Integer(_) => 1,
                            SqlValue::Real(_) => 2,
                            SqlValue::Text(_) => 3,
                            SqlValue::Blob(_) => 4,
                        }),
                        "getString" => match cell {
                            SqlValue::Null => Value::Null,
                            SqlValue::Text(v) => string(vm, v.clone())?,
                            SqlValue::Integer(v) => string(vm, v.to_string())?,
                            SqlValue::Real(v) => string(vm, v.to_string())?,
                            SqlValue::Blob(_) => {
                                return Err(exception(
                                    "Landroid/database/sqlite/SQLiteException;",
                                    "cannot convert BLOB to String",
                                ))
                            }
                        },
                        "getBlob" => match cell {
                            SqlValue::Null => Value::Null,
                            SqlValue::Blob(bytes) => {
                                let a = vm.heap.alloc_array_zeroed(
                                    rd_vm::heap::ElemKind::Int,
                                    bytes.len(),
                                    "[B",
                                )?;
                                for (slot, byte) in
                                    vm.heap.array_elems_mut(a)?.iter_mut().zip(bytes)
                                {
                                    *slot = Value::Int(*byte as i8 as i32);
                                }
                                Value::Obj(a)
                            }
                            _ => {
                                return Err(exception(
                                    "Landroid/database/sqlite/SQLiteException;",
                                    "not a blob",
                                ))
                            }
                        },
                        _ => {
                            let number = match cell {
                                SqlValue::Null => 0.0,
                                SqlValue::Integer(v) => {
                                    if name == "getLong" {
                                        return Ok(Some(Value::Long(*v)));
                                    }
                                    if name == "getInt" {
                                        return Ok(Some(Value::Int(*v as i32)));
                                    }
                                    *v as f64
                                }
                                SqlValue::Real(v) => *v,
                                SqlValue::Text(v) => v.parse::<f64>().unwrap_or(0.0),
                                SqlValue::Blob(_) => {
                                    return Err(exception(
                                        "Landroid/database/sqlite/SQLiteException;",
                                        "numeric BLOB",
                                    ))
                                }
                            };
                            match name {
                                "getInt" => Value::Int(number as i32),
                                "getLong" => Value::Long(number as i64),
                                "getFloat" => Value::Float(number as f32),
                                _ => Value::Double(number),
                            }
                        }
                    }
                }
                _ => return Ok(None),
            };
            return Ok(Some(value));
        }
        Ok(None)
    }
    pub(super) fn references(&self, r: ObjRef) -> Vec<ObjRef> {
        self.helpers
            .borrow()
            .get(&r)
            .and_then(|h| h.database)
            .into_iter()
            .collect()
    }
    pub(super) fn collect(&self, marked: &[bool]) {
        let dead: Vec<_> = self
            .handles
            .borrow()
            .keys()
            .copied()
            .filter(|r| !marked.get(*r as usize).copied().unwrap_or(false))
            .collect();
        for r in dead {
            self.close(r);
            self.handles.borrow_mut().remove(&r);
        }
        self.helpers
            .borrow_mut()
            .retain(|r, _| marked.get(*r as usize).copied().unwrap_or(false));
        self.cursors
            .borrow_mut()
            .retain(|r, _| marked.get(*r as usize).copied().unwrap_or(false));
    }
}
