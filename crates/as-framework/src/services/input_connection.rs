//! API26 local full-editor text state, with genuine View/Context identity.
//! No IME, key delivery, rich spans or keyboard presentation is fabricated.
use super::*;
use core::cell::Cell;
mod editable;
const BASE: &str = "Landroid/view/inputmethod/BaseInputConnection;";
const BUILDER: &str = "Landroid/text/SpannableStringBuilder;";
const SELECTION: &str = "Landroid/text/Selection;";
const EDITABLE: &str = "Landroid/text/Editable;";
#[derive(Clone)]
struct Connection {
    view: ObjRef,
    manager: Option<ObjRef>,
    full_editor: bool,
    editable: Option<ObjRef>,
}
#[derive(Default)]
pub(super) struct InputConnections {
    connections: RefCell<BTreeMap<ObjRef, Option<Connection>>>,
    editables: editable::Editables,
    scopes: RefCell<BTreeMap<u64, Vec<ObjRef>>>,
    serial: Cell<u64>,
}
struct Scope<'a> {
    service: &'a InputConnections,
    ticket: u64,
}
impl Scope<'_> {
    fn add(&self, r: ObjRef) {
        self.service
            .scopes
            .borrow_mut()
            .get_mut(&self.ticket)
            .expect("active input scope")
            .push(r);
    }
}
impl Drop for Scope<'_> {
    fn drop(&mut self) {
        self.service.scopes.borrow_mut().remove(&self.ticket);
    }
}
fn unavailable(cause: impl Into<String>) -> VmExit {
    error("API_UNAVAILABLE", cause)
}
fn thrown(class: &str, cause: impl Into<String>) -> VmExit {
    Throwable::new(class, cause).into()
}
impl InputConnections {
    fn protect(&self, roots: Vec<ObjRef>) -> Result<Scope<'_>> {
        if self.scopes.borrow().len() >= 64 {
            return Err(error("INPUT_BUDGET", "64 active input scopes"));
        }
        let ticket = self
            .serial
            .get()
            .checked_add(1)
            .ok_or_else(|| error("INPUT_BUDGET", "scope serial exhausted"))?;
        self.serial.set(ticket);
        self.scopes.borrow_mut().insert(ticket, roots);
        Ok(Scope {
            service: self,
            ticket,
        })
    }
    fn connection(&self, r: ObjRef) -> Result<Connection> {
        self.connections
            .borrow()
            .get(&r)
            .cloned()
            .flatten()
            .ok_or_else(|| unavailable("BaseInputConnection constructor has not completed"))
    }
    pub(super) fn attach(&self, vm: &Engine, class: &str, r: ObjRef) -> Result<bool> {
        if vm.cp.is_subtype(class, BASE) {
            if self.connections.borrow().len() >= 1024 {
                return Err(error("INPUT_BUDGET", "1024 input connections"));
            }
            self.connections.borrow_mut().insert(r, None);
            return Ok(true);
        }
        self.editables.attach(vm, class, r)
    }
    pub(super) fn metadata(
        class: &str,
        name: &str,
        sig: &str,
    ) -> Option<rd_vm::framework_host::HostMethodKind> {
        if let Some(kind) = editable::Editables::metadata(class, name, sig) {
            return Some(kind);
        }
        if class == BASE && name == "<init>" && sig == "(Landroid/view/View;Z)V" {
            return Some(rd_vm::framework_host::HostMethodKind::Constructor);
        }
        ((class == BASE && name == "getEditable" && sig == "()Landroid/text/Editable;")
            || matches!(class, BASE | "Landroid/view/inputmethod/InputConnection;")
                && matches!(
                    (name, sig),
                    (
                        "beginBatchEdit" | "endBatchEdit" | "finishComposingText",
                        "()Z"
                    ) | ("getHandler", "()Landroid/os/Handler;")
                        | ("closeConnection", "()V")
                        | ("commitText", "(Ljava/lang/CharSequence;I)Z")
                        | ("setSelection" | "deleteSurroundingText", "(II)Z")
                        | (
                            "getTextBeforeCursor" | "getTextAfterCursor",
                            "(II)Ljava/lang/CharSequence;"
                        )
                        | ("getSelectedText", "(I)Ljava/lang/CharSequence;")
                ))
        .then_some(rd_vm::framework_host::HostMethodKind::Instance)
    }
    pub(super) fn call_static(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if class != SELECTION {
            return Ok(None);
        }
        let roots = args
            .iter()
            .filter_map(|v| v.as_ref().ok().flatten())
            .collect();
        let _scope = self.protect(roots)?;
        self.editables.call_static(vm, class, name, sig, args)
    }
    fn actual_editable(
        &self,
        vm: &mut Engine,
        r: ObjRef,
        scope: &Scope<'_>,
    ) -> Result<Option<ObjRef>> {
        let value = vm.invoke_instance(r, "getEditable", "()Landroid/text/Editable;", &[])?;
        let Some(editable) = value.as_ref()? else {
            return Ok(None);
        };
        scope.add(editable);
        if !vm.cp.is_subtype(vm.heap.class_of(editable)?, EDITABLE) {
            return Err(thrown(
                "Ljava/lang/ClassCastException;",
                "getEditable returned a non-Editable",
            ));
        }
        self.editables.anchors(vm, editable)?;
        Ok(Some(editable))
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn call(
        &self,
        android: &Android,
        vm: &mut Engine,
        _class: &str,
        name: &str,
        sig: &str,
        r: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        let is_editable = self.editables.contains(r);
        if !is_editable && !self.connections.borrow().contains_key(&r) {
            return Ok(None);
        }
        let mut roots = vec![r];
        roots.extend(
            args.iter()
                .filter_map(|value| value.as_ref().ok().flatten()),
        );
        let scope = self.protect(roots)?;
        if is_editable {
            return self.editables.call(vm, name, sig, r, args);
        }
        if name == "<init>" && sig == "(Landroid/view/View;Z)V" {
            let view = obj(&args[0])?;
            android.validate_input_target(vm, view)?;
            let context = obj(&vm.invoke_instance(
                view,
                "getContext",
                "()Landroid/content/Context;",
                &[],
            )?)?;
            scope.add(context);
            if !vm
                .cp
                .is_subtype(vm.heap.class_of(context)?, "Landroid/content/Context;")
            {
                return Err(thrown(
                    "Ljava/lang/ClassCastException;",
                    "View context type",
                ));
            }
            let key = string(vm, "input_method")?;
            if let Some(r) = key.as_ref()? {
                scope.add(r);
            }
            let manager = vm
                .invoke_instance(
                    context,
                    "getSystemService",
                    "(Ljava/lang/String;)Ljava/lang/Object;",
                    &[key],
                )?
                .as_ref()?;
            if let Some(manager) = manager {
                scope.add(manager);
                if !vm.cp.is_subtype(
                    vm.heap.class_of(manager)?,
                    "Landroid/view/inputmethod/InputMethodManager;",
                ) {
                    return Err(thrown(
                        "Ljava/lang/ClassCastException;",
                        "input_method service type",
                    ));
                }
            }
            self.connections.borrow_mut().insert(
                r,
                Some(Connection {
                    view,
                    manager,
                    full_editor: int(args, 1)? != 0,
                    editable: None,
                }),
            );
            return Ok(Some(Value::Null));
        }
        let connection = self.connection(r)?;
        let value = match (name, sig) {
            ("getEditable", "()Landroid/text/Editable;") => {
                let object = match connection.editable {
                    Some(r) => r,
                    None => {
                        let value = self.editables.alloc(vm, vec![])?;
                        self.editables.select(vm, value, 0, 0)?;
                        self.connections
                            .borrow_mut()
                            .get_mut(&r)
                            .expect("owned connection")
                            .as_mut()
                            .expect("initialized connection")
                            .editable = Some(value);
                        value
                    }
                };
                Value::Obj(object)
            }
            ("beginBatchEdit" | "endBatchEdit", "()Z") => Value::Int(0),
            ("getHandler", "()Landroid/os/Handler;") => Value::Null,
            ("closeConnection", "()V") => {
                vm.invoke_instance(r, "finishComposingText", "()Z", &[])?;
                Value::Null
            }
            ("finishComposingText", "()Z") => {
                if !connection.full_editor {
                    return Err(unavailable(
                        "dummy input text requires actual IME key delivery",
                    ));
                }
                if self.actual_editable(vm, r, &scope)?.is_some() {
                    vm.invoke_instance(r, "beginBatchEdit", "()Z", &[])?;
                    // Rich/composing spans cannot be installed through this
                    // subset, so there are no admitted composing spans to erase.
                    vm.invoke_instance(r, "endBatchEdit", "()Z", &[])?;
                }
                Value::Int(1)
            }
            ("commitText", "(Ljava/lang/CharSequence;I)Z") => {
                if !connection.full_editor {
                    return Err(unavailable(
                        "dummy input text requires actual IME key delivery",
                    ));
                }
                if let Some(content) = self.actual_editable(vm, r, &scope)? {
                    vm.invoke_instance(r, "beginBatchEdit", "()Z", &[])?;
                    let [mut start, mut end] = self.editables.anchors(vm, content)?;
                    start = start.max(0);
                    end = end.max(0);
                    if start > end {
                        core::mem::swap(&mut start, &mut end);
                    }
                    let length = self.editables.snapshot(content)?.len() as i32;
                    let position = int(args, 1)?;
                    let cursor = if position > 0 {
                        position.wrapping_add(end).wrapping_sub(1)
                    } else {
                        position.wrapping_add(start)
                    }
                    .clamp(0, length);
                    self.editables.select(vm, content, cursor, cursor)?;
                    vm.invoke_instance(
                        content,
                        "replace",
                        "(IILjava/lang/CharSequence;)Landroid/text/Editable;",
                        &[Value::Int(start), Value::Int(end), args[0].clone()],
                    )?;
                    vm.invoke_instance(r, "endBatchEdit", "()Z", &[])?;
                }
                Value::Int(1)
            }
            ("setSelection", "(II)Z") => {
                let Some(content) = self.actual_editable(vm, r, &scope)? else {
                    return Ok(Some(Value::Int(0)));
                };
                let length = self.editables.snapshot(content)?.len() as i32;
                let (start, end) = (int(args, 0)?, int(args, 1)?);
                if start >= 0 && end >= 0 && start <= length && end <= length {
                    self.editables.select(vm, content, start, end)?;
                }
                Value::Int(1)
            }
            ("getTextBeforeCursor" | "getTextAfterCursor", "(II)Ljava/lang/CharSequence;")
            | ("getSelectedText", "(I)Ljava/lang/CharSequence;") => {
                let Some(content) = self.actual_editable(vm, r, &scope)? else {
                    return Ok(Some(Value::Null));
                };
                let [mut start, mut end] = self.editables.anchors(vm, content)?;
                if start > end {
                    core::mem::swap(&mut start, &mut end);
                }
                let units = self.editables.snapshot(content)?;
                let flags = int(args, if name == "getSelectedText" { 0 } else { 1 })?;
                if flags != 0 {
                    return Err(unavailable(
                        "styled InputConnection text snapshots require a rich span provider",
                    ));
                }
                let (a, b) = if name == "getSelectedText" {
                    if start == end {
                        return Ok(Some(Value::Null));
                    }
                    (start, end)
                } else if name == "getTextBeforeCursor" {
                    if start <= 0 {
                        return string(vm, "").map(Some);
                    }
                    let count = int(args, 0)?.min(start);
                    (start.wrapping_sub(count), start)
                } else {
                    end = end.max(0);
                    let count = int(args, 0)?;
                    let limit = end
                        .checked_add(count)
                        .ok_or_else(|| unavailable("InputConnection query integer overflow"))?
                        .min(units.len() as i32);
                    (end, limit)
                };
                if a < 0 || b < a || b as usize > units.len() {
                    return Err(thrown(
                        "Ljava/lang/IndexOutOfBoundsException;",
                        "input text range",
                    ));
                }
                editable::utf16_string(vm, &units[a as usize..b as usize])?
            }
            ("deleteSurroundingText", "(II)Z") => {
                let Some(content) = self.actual_editable(vm, r, &scope)? else {
                    return Ok(Some(Value::Int(0)));
                };
                let [mut start, mut end] = self.editables.anchors(vm, content)?;
                if start > end {
                    core::mem::swap(&mut start, &mut end);
                }
                let before = int(args, 0)?;
                let after = int(args, 1)?;
                if after > 0 && end.checked_add(after).is_none() {
                    return Err(unavailable("InputConnection delete integer overflow"));
                }
                vm.invoke_instance(r, "beginBatchEdit", "()Z", &[])?;
                let mut removed = 0;
                if before > 0 {
                    let from = start.saturating_sub(before).max(0);
                    vm.invoke_instance(
                        content,
                        "delete",
                        "(II)Landroid/text/Editable;",
                        &[Value::Int(from), Value::Int(start)],
                    )?;
                    removed = start - from;
                }
                if after > 0 {
                    end -= removed;
                    let length = self.editables.snapshot(content)?.len() as i32;
                    let to = end.saturating_add(after).min(length);
                    vm.invoke_instance(
                        content,
                        "delete",
                        "(II)Landroid/text/Editable;",
                        &[Value::Int(end), Value::Int(to)],
                    )?;
                }
                vm.invoke_instance(r, "endBatchEdit", "()Z", &[])?;
                Value::Int(1)
            }
            ("sendKeyEvent", "(Landroid/view/KeyEvent;)Z") => {
                return Err(unavailable("InputMethodManager key dispatch backend"))
            }
            _ => return Ok(None),
        };
        Ok(Some(value))
    }
    pub(super) fn roots(&self) -> Vec<ObjRef> {
        self.scopes.borrow().values().flatten().copied().collect()
    }
    pub(super) fn references(&self, r: ObjRef) -> Vec<ObjRef> {
        self.connections
            .borrow()
            .get(&r)
            .and_then(Option::as_ref)
            .map_or_else(Vec::new, |value| {
                let mut roots = vec![value.view];
                roots.extend(value.manager);
                roots.extend(value.editable);
                roots
            })
    }
    pub(super) fn collect(&self, marked: &[bool]) {
        self.connections
            .borrow_mut()
            .retain(|r, _| marked.get(*r as usize).copied().unwrap_or(false));
        self.editables.collect(marked);
    }
}
