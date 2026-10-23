//! SharedPreferences API-26 behavior, backed by typed Android XML snapshots.
//! Contract reference: AOSP android-8.0.0_r1 SharedPreferencesImpl (Apache-2.0).
use super::*;
use alloc::collections::BTreeSet;
use as_storage::{Preference, Preferences, Store};
#[derive(Clone)]
struct Editor {
    owner: ObjRef,
    clear: bool,
    changes: BTreeMap<String, Option<Preference>>,
}
#[derive(Default)]
struct State {
    names: BTreeMap<ObjRef, String>,
    instances: BTreeMap<String, ObjRef>,
    values: BTreeMap<String, Preferences>,
    editors: BTreeMap<ObjRef, Editor>,
    pending: BTreeMap<String, Preferences>,
    listeners: BTreeMap<ObjRef, BTreeSet<ObjRef>>,
    notification_roots: Vec<ObjRef>,
}
fn bytes(value: &Preference) -> usize {
    64 + match value {
        Preference::String(v) => v.len(),
        Preference::StringSet(v) => v.iter().map(|s| s.len() + 64).sum(),
        _ => 0,
    }
}
fn budget(state: &State) -> Result<()> {
    let values = state
        .values
        .values()
        .chain(state.pending.values())
        .map(|p| p.iter().map(|(k, v)| k.len() + bytes(v)).sum::<usize>())
        .sum::<usize>();
    let edits = state
        .editors
        .values()
        .map(|e| {
            e.changes
                .iter()
                .map(|(k, v)| k.len() + v.as_ref().map_or(64, bytes))
                .sum::<usize>()
        })
        .sum::<usize>();
    if values.saturating_add(edits) > 8 << 20 {
        return Err(error(
            "PREFERENCES_BUDGET",
            "8 MiB retained preference values/edits",
        ));
    }
    Ok(())
}
pub(super) struct PreferenceService {
    store: Store,
    state: RefCell<State>,
}
impl PreferenceService {
    pub(super) fn new(store: Store) -> Self {
        Self {
            store,
            state: Default::default(),
        }
    }
    pub(super) fn get(&self, vm: &mut Engine, name: &str) -> Result<Value> {
        if let Some(r) = self.state.borrow().instances.get(name) {
            return Ok(Value::Obj(*r));
        }
        if self.state.borrow().instances.len() >= 128 {
            return Err(error("PREFERENCES_BUDGET", "128 preference files"));
        }
        let values = self.store.preferences(name).map_err(storage_error)?;
        let r = vm
            .heap
            .alloc_instance("Landroid/app/SharedPreferencesImpl;".into(), vec![])?;
        let mut state = self.state.borrow_mut();
        state.names.insert(r, name.into());
        state.instances.insert(name.into(), r);
        state.values.insert(name.into(), values);
        budget(&state)?;
        Ok(Value::Obj(r))
    }
    pub(super) fn flush(&self) -> Result<()> {
        loop {
            let next = self
                .state
                .borrow()
                .pending
                .iter()
                .next()
                .map(|(name, values)| (name.clone(), values.clone()));
            let Some((name, values)) = next else {
                break;
            };
            self.store
                .save_preferences(&name, &values)
                .map_err(storage_error)?;
            self.state.borrow_mut().pending.remove(&name);
        }
        Ok(())
    }
    fn value(
        services: &Services,
        vm: &mut Engine,
        value: &Preference,
        boxed: bool,
    ) -> Result<Value> {
        let (class, value) = match value {
            Preference::String(v) => return string(vm, v.clone()),
            Preference::StringSet(values) => {
                let values = values
                    .iter()
                    .map(|v| string(vm, v.clone()))
                    .collect::<Result<Vec<_>>>()?;
                return services.collections.new_list(vm, values, true);
            }
            Preference::Int(v) => ("Ljava/lang/Integer;", Value::Int(*v)),
            Preference::Long(v) => ("Ljava/lang/Long;", Value::Long(*v)),
            Preference::Float(v) => ("Ljava/lang/Float;", Value::Float(f32::from_bits(*v))),
            Preference::Boolean(v) => ("Ljava/lang/Boolean;", Value::Int(*v as i32)),
        };
        if boxed {
            services.collections.boxed(vm, class, value)
        } else {
            Ok(value)
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn call(
        &self,
        services: &Services,
        android: &Android,
        vm: &mut Engine,
        _class: &str,
        name: &str,
        sig: &str,
        r: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if name == "getSharedPreferences"
            && sig == "(Ljava/lang/String;I)Landroid/content/SharedPreferences;"
            && vm
                .cp
                .is_subtype(vm.heap.class_of(r)?, "Landroid/content/Context;")
        {
            if int(args, 1)? != 0 {
                return Err(error(
                    "API_UNAVAILABLE",
                    "SharedPreferences mode: only MODE_PRIVATE",
                ));
            }
            return self.get(vm, &text(vm, &args[0])?).map(Some);
        }
        if name == "getPreferences"
            && sig == "(I)Landroid/content/SharedPreferences;"
            && vm
                .cp
                .is_subtype(vm.heap.class_of(r)?, "Landroid/app/Activity;")
        {
            if int(args, 0)? != 0 {
                return Err(error("API_UNAVAILABLE", "Activity preferences mode"));
            }
            let class = vm.heap.class_of(r)?.to_string();
            let name = class
                .trim_end_matches(';')
                .rsplit('/')
                .next()
                .unwrap_or(&class);
            return self.get(vm, name).map(Some);
        }
        let pref_name = self.state.borrow().names.get(&r).cloned();
        if let Some(pref_name) = pref_name {
            let value = match (name, sig) {
                ("edit", "()Landroid/content/SharedPreferences$Editor;") => {
                    if self.state.borrow().editors.len() >= 64 {
                        return Err(error(
                            "PREFERENCES_BUDGET",
                            "64 editors; GC unreferenced editors before allocating more",
                        ));
                    }
                    let editor = vm.heap.alloc_instance(
                        "Landroid/app/SharedPreferencesImpl$EditorImpl;".into(),
                        vec![],
                    )?;
                    self.state.borrow_mut().editors.insert(
                        editor,
                        Editor {
                            owner: r,
                            clear: false,
                            changes: BTreeMap::new(),
                        },
                    );
                    Value::Obj(editor)
                }
                ("contains", "(Ljava/lang/String;)Z") => Value::Int(
                    self.state.borrow().values[&pref_name].contains_key(&text(vm, &args[0])?)
                        as i32,
                ),
                ("getAll", "()Ljava/util/Map;") => {
                    let values = self.state.borrow().values[&pref_name].clone();
                    let mut items = Vec::new();
                    for (key, value) in values {
                        items.push((string(vm, key)?, Self::value(services, vm, &value, true)?));
                    }
                    services.collections.new_map(vm, items)?
                }
                (
                    "registerOnSharedPreferenceChangeListener",
                    "(Landroid/content/SharedPreferences$OnSharedPreferenceChangeListener;)V",
                ) => {
                    if let Some(listener) = args[0].as_ref()? {
                        self.state
                            .borrow_mut()
                            .listeners
                            .entry(r)
                            .or_default()
                            .insert(listener);
                    }
                    Value::Null
                }
                (
                    "unregisterOnSharedPreferenceChangeListener",
                    "(Landroid/content/SharedPreferences$OnSharedPreferenceChangeListener;)V",
                ) => {
                    if let Some(listener) = args[0].as_ref()? {
                        if let Some(listeners) = self.state.borrow_mut().listeners.get_mut(&r) {
                            listeners.remove(&listener);
                        }
                    }
                    Value::Null
                }
                _ if matches!(
                    (name, sig),
                    (
                        "getString",
                        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;"
                    ) | (
                        "getStringSet",
                        "(Ljava/lang/String;Ljava/util/Set;)Ljava/util/Set;"
                    ) | ("getInt", "(Ljava/lang/String;I)I")
                        | ("getLong", "(Ljava/lang/String;J)J")
                        | ("getFloat", "(Ljava/lang/String;F)F")
                        | ("getBoolean", "(Ljava/lang/String;Z)Z")
                ) =>
                {
                    let key = text(vm, &args[0])?;
                    let stored = self.state.borrow().values[&pref_name].get(&key).cloned();
                    if let Some(value) = stored {
                        let correct = matches!(
                            (name, &value),
                            ("getString", Preference::String(_))
                                | ("getStringSet", Preference::StringSet(_))
                                | ("getInt", Preference::Int(_))
                                | ("getLong", Preference::Long(_))
                                | ("getFloat", Preference::Float(_))
                                | ("getBoolean", Preference::Boolean(_))
                        );
                        if !correct {
                            return Err(
                                Throwable::new("Ljava/lang/ClassCastException;", key).into()
                            );
                        }
                        Self::value(services, vm, &value, false)?
                    } else {
                        args[1].clone()
                    }
                }
                _ => return Ok(None),
            };
            return Ok(Some(value));
        }
        let editor = self.state.borrow().editors.get(&r).cloned();
        let Some(mut editor) = editor else {
            return Ok(None);
        };
        let mut update = false;
        let value = match (name, sig) {
            ("clear", "()Landroid/content/SharedPreferences$Editor;") => {
                editor.clear = true;
                update = true;
                Value::Obj(r)
            }
            ("remove", "(Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;") => {
                editor.changes.insert(text(vm, &args[0])?, None);
                update = true;
                Value::Obj(r)
            }
            _ if name.starts_with("put")
                && sig.ends_with(")Landroid/content/SharedPreferences$Editor;") =>
            {
                let key = text(vm, &args[0])?;
                let value = match name {
                    "putString" => {
                        if args[1].as_ref()?.is_none() {
                            None
                        } else {
                            Some(Preference::String(text(vm, &args[1])?))
                        }
                    }
                    "putStringSet" => {
                        if let Some(set) = args[1].as_ref()? {
                            let values = services
                                .collections
                                .items(set)?
                                .iter()
                                .map(|v| text(vm, v))
                                .collect::<Result<BTreeSet<_>>>()?;
                            Some(Preference::StringSet(values))
                        } else {
                            None
                        }
                    }
                    "putInt" => Some(Preference::Int(int(args, 1)?)),
                    "putLong" => Some(Preference::Long(args[1].as_long()?)),
                    "putFloat" => Some(Preference::Float(args[1].as_float()?.to_bits())),
                    "putBoolean" => Some(Preference::Boolean(int(args, 1)? != 0)),
                    _ => return Ok(None),
                };
                if editor.changes.len() >= 8192 && !editor.changes.contains_key(&key) {
                    return Err(error("PREFERENCES_BUDGET", "8192 pending edits"));
                }
                editor.changes.insert(key, value);
                update = true;
                Value::Obj(r)
            }
            ("commit", "()Z") | ("apply", "()V") => {
                let pref_name = self.state.borrow().names[&editor.owner].clone();
                let mut values = if editor.clear {
                    Preferences::new()
                } else {
                    self.state.borrow().values[&pref_name].clone()
                };
                let mut changed = Vec::new();
                // API 26 clears first; clear-only does not synthesize per-key listener events.
                for (key, value) in core::mem::take(&mut editor.changes) {
                    if values.get(&key) == value.as_ref() {
                        continue;
                    }
                    if let Some(value) = value {
                        values.insert(key.clone(), value);
                    } else {
                        values.remove(&key);
                    }
                    changed.push(key);
                }
                self.state
                    .borrow_mut()
                    .values
                    .insert(pref_name.clone(), values.clone());
                editor.clear = false;
                self.state.borrow_mut().editors.insert(r, editor.clone());
                self.state.borrow_mut().pending.insert(pref_name, values);
                budget(&self.state.borrow())?;
                let committed = if name == "commit" {
                    self.flush().is_ok()
                } else {
                    true
                };
                let listeners = self
                    .state
                    .borrow()
                    .listeners
                    .get(&editor.owner)
                    .cloned()
                    .unwrap_or_default();
                let root_count = self.state.borrow().notification_roots.len();
                self.state
                    .borrow_mut()
                    .notification_roots
                    .extend(listeners.iter().copied());
                let notified = (|| -> Result<()> {
                    for key in changed.into_iter().rev() {
                        let key = string(vm, key)?;
                        for listener in &listeners {
                            android.callback(
                                vm,
                                *listener,
                                "onSharedPreferenceChanged",
                                "(Landroid/content/SharedPreferences;Ljava/lang/String;)V",
                                &[Value::Obj(editor.owner), key.clone()],
                            )?;
                        }
                    }
                    Ok(())
                })();
                self.state
                    .borrow_mut()
                    .notification_roots
                    .truncate(root_count);
                notified?;
                if name == "commit" {
                    Value::Int(committed as i32)
                } else {
                    Value::Null
                }
            }
            _ => return Ok(None),
        };
        if update {
            self.state.borrow_mut().editors.insert(r, editor);
            budget(&self.state.borrow())?;
        }
        Ok(Some(value))
    }
    pub(super) fn roots(&self) -> Vec<ObjRef> {
        let state = self.state.borrow();
        state
            .instances
            .values()
            .chain(state.notification_roots.iter())
            .copied()
            .collect()
    }
    pub(super) fn references(&self, r: ObjRef) -> Vec<ObjRef> {
        self.state
            .borrow()
            .editors
            .get(&r)
            .map(|e| vec![e.owner])
            .unwrap_or_default()
    }
    pub(super) fn collect(&self, marked: &[bool]) {
        let live = |r: &ObjRef| marked.get(*r as usize).copied().unwrap_or(false);
        let mut state = self.state.borrow_mut();
        state.editors.retain(|r, _| live(r));
        // Listener registrations are weak, as on Android; they are not GC roots.
        for listeners in state.listeners.values_mut() {
            listeners.retain(live);
        }
    }
}
