//! Owned UTF-16 editable buffers. Selection anchors are supported; arbitrary
//! spans, filters and watcher registration are explicitly outside this layer.
use super::*;
const LIMIT: usize = 1 << 20;
const TOTAL: usize = 16 << 20;
#[derive(Default)]
struct Buffer {
    text: Option<Vec<u16>>,
    selection: [i32; 2],
    version: u64,
}
impl Buffer {
    fn initialized(text: Vec<u16>) -> Self {
        Self {
            text: Some(text),
            selection: [-1, -1],
            version: 1,
        }
    }
}
#[derive(Default)]
pub(super) struct Editables {
    objects: RefCell<BTreeMap<ObjRef, Buffer>>,
}
fn bounds(start: i32, end: i32, length: usize) -> Result<(usize, usize)> {
    if start < 0 || end < start || end as usize > length {
        return Err(thrown(
            "Ljava/lang/IndexOutOfBoundsException;",
            "editable range",
        ));
    }
    Ok((start as usize, end as usize))
}
pub(super) fn utf16_string(vm: &mut Engine, units: &[u16]) -> Result<Value> {
    let value = String::from_utf16(units).map_err(|_| {
        unavailable("unpaired UTF-16 cannot yet be represented by the VM String heap")
    })?;
    string(vm, value)
}
pub(super) fn sequence(vm: &mut Engine, value: &Value) -> Result<Vec<u16>> {
    let object = obj(value)?;
    if vm.heap.class_of(object)? == "Ljava/lang/String;" {
        let text = vm.heap.as_str(object)?;
        if text.len() > LIMIT * 4 {
            return Err(error("EDITABLE_BUDGET", "source text limit"));
        }
        let units: Vec<_> = text.encode_utf16().collect();
        if units.len() > LIMIT {
            return Err(error("EDITABLE_BUDGET", "1 Mi UTF-16 units per buffer"));
        }
        return Ok(units);
    }
    if !vm
        .cp
        .is_subtype(vm.heap.class_of(object)?, "Ljava/lang/CharSequence;")
    {
        return Err(thrown(
            "Ljava/lang/ClassCastException;",
            "expected CharSequence",
        ));
    }
    let length = vm.invoke_instance(object, "length", "()I", &[])?.as_int()?;
    if length < 0 || length as usize > LIMIT {
        return Err(error("EDITABLE_BUDGET", "CharSequence length"));
    }
    let mut units = Vec::with_capacity(length as usize);
    for index in 0..length {
        units.push(
            vm.invoke_instance(object, "charAt", "(I)C", &[Value::Int(index)])?
                .as_int()? as u16,
        );
    }
    Ok(units)
}
impl Editables {
    pub(super) fn attach(&self, vm: &Engine, class: &str, r: ObjRef) -> Result<bool> {
        if !vm.cp.is_subtype(class, BUILDER) {
            return Ok(false);
        }
        if self.objects.borrow().len() >= 1024 {
            return Err(error("EDITABLE_BUDGET", "1024 editable objects"));
        }
        self.objects.borrow_mut().insert(r, Buffer::default());
        Ok(true)
    }
    fn budget(&self, r: ObjRef, length: usize) -> Result<()> {
        let total: usize = self
            .objects
            .borrow()
            .iter()
            .filter(|(key, _)| **key != r)
            .map(|(_, value)| value.text.as_ref().map_or(0, |text| text.len() * 2))
            .sum();
        if length > LIMIT || total.saturating_add(length * 2) > TOTAL {
            return Err(error(
                "EDITABLE_BUDGET",
                "1 Mi UTF-16 units per buffer / 16 MiB total",
            ));
        }
        Ok(())
    }
    pub(super) fn alloc(&self, vm: &mut Engine, text: Vec<u16>) -> Result<ObjRef> {
        if self.objects.borrow().len() >= 1024 {
            return Err(error("EDITABLE_BUDGET", "1024 editable objects"));
        }
        self.budget(0, text.len())?;
        let r = vm.heap.alloc_instance(BUILDER.into(), vec![])?;
        self.objects
            .borrow_mut()
            .insert(r, Buffer::initialized(text));
        Ok(r)
    }
    pub(super) fn snapshot(&self, r: ObjRef) -> Result<Vec<u16>> {
        self.objects
            .borrow()
            .get(&r)
            .and_then(|buffer| buffer.text.clone())
            .ok_or_else(|| {
                unavailable("editable is not an initialized owned SpannableStringBuilder")
            })
    }
    pub(super) fn anchors(&self, vm: &mut Engine, r: ObjRef) -> Result<[i32; 2]> {
        let class = vm.heap.class_of(r)?.to_string();
        // We do not bypass application span/filter semantics we cannot model.
        for (name, sig) in [
            ("getSpanStart", "(Ljava/lang/Object;)I"),
            ("setSpan", "(Ljava/lang/Object;III)V"),
            ("length", "()I"),
        ] {
            if vm.cp.resolve_method(&class, name, sig).is_some() {
                return Err(unavailable(
                    "custom editable span/length overrides require a span provider",
                ));
            }
        }
        let objects = self.objects.borrow();
        let buffer = objects
            .get(&r)
            .ok_or_else(|| unavailable("external Editable provider"))?;
        if buffer.text.is_none() {
            return Err(unavailable("editable constructor has not completed"));
        }
        Ok(buffer.selection)
    }
    pub(super) fn select(&self, vm: &mut Engine, r: ObjRef, start: i32, end: i32) -> Result<()> {
        self.anchors(vm, r)?;
        let mut objects = self.objects.borrow_mut();
        let buffer = objects.get_mut(&r).expect("validated editable");
        let length = buffer.text.as_ref().expect("initialized editable").len();
        if start < 0 || end < 0 || start as usize > length || end as usize > length {
            return Err(thrown(
                "Ljava/lang/IndexOutOfBoundsException;",
                "selection range",
            ));
        }
        buffer.selection = [start, end];
        Ok(())
    }
    pub(super) fn contains(&self, r: ObjRef) -> bool {
        self.objects.borrow().contains_key(&r)
    }
    pub(super) fn metadata(
        class: &str,
        name: &str,
        sig: &str,
    ) -> Option<rd_vm::framework_host::HostMethodKind> {
        if class == SELECTION
            && matches!(
                (name, sig),
                (
                    "getSelectionStart" | "getSelectionEnd",
                    "(Ljava/lang/CharSequence;)I"
                ) | (
                    "setSelection",
                    "(Landroid/text/Spannable;I)V" | "(Landroid/text/Spannable;II)V"
                ) | (
                    "selectAll" | "removeSelection",
                    "(Landroid/text/Spannable;)V"
                )
            )
        {
            return Some(rd_vm::framework_host::HostMethodKind::Static);
        }
        let result = matches!(
            (name, sig),
            ("length", "()I")
                | ("charAt", "(I)C")
                | ("toString", "()Ljava/lang/String;")
                | ("subSequence", "(II)Ljava/lang/CharSequence;")
                | ("getChars", "(II[CI)V")
                | ("clear" | "clearSpans", "()V")
                | (
                    "replace",
                    "(IILjava/lang/CharSequence;)Landroid/text/Editable;"
                        | "(IILjava/lang/CharSequence;)Landroid/text/SpannableStringBuilder;"
                        | "(IILjava/lang/CharSequence;II)Landroid/text/Editable;"
                        | "(IILjava/lang/CharSequence;II)Landroid/text/SpannableStringBuilder;"
                )
                | (
                    "delete",
                    "(II)Landroid/text/Editable;" | "(II)Landroid/text/SpannableStringBuilder;"
                )
                | (
                    "append",
                    "(Ljava/lang/CharSequence;)Landroid/text/Editable;"
                        | "(Ljava/lang/CharSequence;)Landroid/text/SpannableStringBuilder;"
                        | "(Ljava/lang/CharSequence;)Ljava/lang/Appendable;"
                )
                | (
                    "append",
                    "(C)Landroid/text/Editable;"
                        | "(C)Landroid/text/SpannableStringBuilder;"
                        | "(C)Ljava/lang/Appendable;"
                )
        );
        if class == BUILDER
            && name == "<init>"
            && matches!(
                sig,
                "()V" | "(Ljava/lang/CharSequence;)V" | "(Ljava/lang/CharSequence;II)V"
            )
        {
            Some(rd_vm::framework_host::HostMethodKind::Constructor)
        } else if result
            && (class == BUILDER
                || class == "Ljava/lang/CharSequence;"
                    && matches!(name, "length" | "charAt" | "subSequence" | "toString")
                || class == "Landroid/text/GetChars;" && name == "getChars"
                || class == "Landroid/text/Editable;"
                    && (matches!(name, "clear" | "clearSpans")
                        || matches!(name, "replace" | "delete" | "append")
                            && sig.ends_with("Landroid/text/Editable;"))
                || class == "Ljava/lang/Appendable;"
                    && name == "append"
                    && sig.ends_with("Ljava/lang/Appendable;"))
        {
            Some(rd_vm::framework_host::HostMethodKind::Instance)
        } else {
            None
        }
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
        if matches!(
            (name, sig),
            (
                "getSelectionStart" | "getSelectionEnd",
                "(Ljava/lang/CharSequence;)I"
            )
        ) {
            let Some(r) = args[0].as_ref()? else {
                return Ok(Some(Value::Int(-1)));
            };
            if !vm
                .cp
                .is_subtype(vm.heap.class_of(r)?, "Landroid/text/Spanned;")
            {
                return Ok(Some(Value::Int(-1)));
            }
            return Ok(Some(Value::Int(
                self.anchors(vm, r)?[usize::from(name == "getSelectionEnd")],
            )));
        }
        let expected = matches!(
            (name, sig),
            (
                "setSelection",
                "(Landroid/text/Spannable;I)V" | "(Landroid/text/Spannable;II)V"
            ) | (
                "selectAll" | "removeSelection",
                "(Landroid/text/Spannable;)V"
            )
        );
        if !expected {
            return Ok(None);
        }
        let r = obj(&args[0])?;
        if name == "removeSelection" {
            self.anchors(vm, r)?;
            self.objects
                .borrow_mut()
                .get_mut(&r)
                .expect("checked editable")
                .selection = [-1, -1];
        } else {
            let (start, end) = if name == "selectAll" {
                (0, self.snapshot(r)?.len() as i32)
            } else {
                (
                    int(args, 1)?,
                    if args.len() == 3 {
                        int(args, 2)?
                    } else {
                        int(args, 1)?
                    },
                )
            };
            self.select(vm, r, start, end)?;
        }
        Ok(Some(Value::Null))
    }
    pub(super) fn call(
        &self,
        vm: &mut Engine,
        name: &str,
        sig: &str,
        r: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if !self.contains(r) {
            return Ok(None);
        }
        if name == "<init>" {
            let units = match sig {
                "()V" => vec![],
                "(Ljava/lang/CharSequence;)V" => sequence(vm, &args[0])?,
                "(Ljava/lang/CharSequence;II)V" => {
                    let text = sequence(vm, &args[0])?;
                    let (a, b) = bounds(int(args, 1)?, int(args, 2)?, text.len())?;
                    text[a..b].to_vec()
                }
                _ => return Ok(None),
            };
            self.budget(r, units.len())?;
            self.objects
                .borrow_mut()
                .insert(r, Buffer::initialized(units));
            return Ok(Some(Value::Null));
        }
        let text = self.snapshot(r)?;
        let result = match (name, sig) {
            ("length", "()I") => Value::Int(text.len() as i32),
            ("charAt", "(I)C") => {
                let index = int(args, 0)?;
                if index < 0 || index as usize >= text.len() {
                    return Err(thrown("Ljava/lang/IndexOutOfBoundsException;", "charAt"));
                }
                Value::Int(text[index as usize] as i32)
            }
            ("toString", "()Ljava/lang/String;") => utf16_string(vm, &text)?,
            ("subSequence", "(II)Ljava/lang/CharSequence;") => {
                let (a, b) = bounds(int(args, 0)?, int(args, 1)?, text.len())?;
                Value::Obj(self.alloc(vm, text[a..b].to_vec())?)
            }
            ("getChars", "(II[CI)V") => {
                let (a, b) = bounds(int(args, 0)?, int(args, 1)?, text.len())?;
                let target = obj(&args[2])?;
                let offset = int(args, 3)?;
                if vm.heap.class_of(target)? != "[C" {
                    return Err(thrown(
                        "Ljava/lang/IllegalArgumentException;",
                        "getChars needs char[]",
                    ));
                }
                let length = vm.heap.as_array_elems(target)?.0.len();
                if offset < 0
                    || (offset as usize)
                        .checked_add(b - a)
                        .is_none_or(|end| end > length)
                {
                    return Err(thrown(
                        "Ljava/lang/IndexOutOfBoundsException;",
                        "getChars target",
                    ));
                }
                for (index, unit) in text[a..b].iter().enumerate() {
                    vm.heap.array_elems_mut(target)?[offset as usize + index] =
                        Value::Int(*unit as i32);
                }
                Value::Null
            }
            ("clearSpans", "()V") => {
                self.anchors(vm, r)?;
                self.objects
                    .borrow_mut()
                    .get_mut(&r)
                    .expect("checked editable")
                    .selection = [-1, -1];
                Value::Null
            }
            ("clear", "()V") => {
                let empty = string(vm, "")?;
                self.replace_virtual(vm, r, 0, text.len() as i32, empty, 0, 0)?;
                Value::Null
            }
            (
                "delete",
                "(II)Landroid/text/Editable;" | "(II)Landroid/text/SpannableStringBuilder;",
            ) => {
                let empty = string(vm, "")?;
                self.replace_virtual(vm, r, int(args, 0)?, int(args, 1)?, empty, 0, 0)?;
                Value::Obj(r)
            }
            (
                "append",
                "(C)Landroid/text/Editable;"
                | "(C)Landroid/text/SpannableStringBuilder;"
                | "(C)Ljava/lang/Appendable;",
            ) => {
                let source = utf16_string(vm, &[int(args, 0)? as u16])?;
                self.replace_virtual(vm, r, text.len() as i32, text.len() as i32, source, 0, 1)?;
                Value::Obj(r)
            }
            (
                "append",
                "(Ljava/lang/CharSequence;)Landroid/text/Editable;"
                | "(Ljava/lang/CharSequence;)Landroid/text/SpannableStringBuilder;"
                | "(Ljava/lang/CharSequence;)Ljava/lang/Appendable;",
            ) => {
                let length = vm
                    .invoke_instance(obj(&args[0])?, "length", "()I", &[])?
                    .as_int()?;
                self.replace_virtual(
                    vm,
                    r,
                    text.len() as i32,
                    text.len() as i32,
                    args[0].clone(),
                    0,
                    length,
                )?;
                Value::Obj(r)
            }
            (
                "replace",
                "(IILjava/lang/CharSequence;)Landroid/text/Editable;"
                | "(IILjava/lang/CharSequence;)Landroid/text/SpannableStringBuilder;",
            ) => {
                let length = vm
                    .invoke_instance(obj(&args[2])?, "length", "()I", &[])?
                    .as_int()?;
                self.replace_virtual(
                    vm,
                    r,
                    int(args, 0)?,
                    int(args, 1)?,
                    args[2].clone(),
                    0,
                    length,
                )?;
                Value::Obj(r)
            }
            (
                "replace",
                "(IILjava/lang/CharSequence;II)Landroid/text/Editable;"
                | "(IILjava/lang/CharSequence;II)Landroid/text/SpannableStringBuilder;",
            ) => {
                self.anchors(vm, r)?;
                let (start, end) = bounds(int(args, 0)?, int(args, 1)?, text.len())?;
                let version = self.objects.borrow()[&r].version;
                let source = sequence(vm, &args[2])?;
                let (a, b) = bounds(int(args, 3)?, int(args, 4)?, source.len())?;
                let length = text.len() - (end - start) + (b - a);
                self.budget(r, length)?;
                let mut objects = self.objects.borrow_mut();
                let buffer = objects.get_mut(&r).expect("active editable");
                if buffer.version != version {
                    return Err(unavailable(
                        "editable mutated while reading a CharSequence argument",
                    ));
                }
                for anchor in &mut buffer.selection {
                    if *anchor < 0 {
                        continue;
                    }
                    let old = *anchor as usize;
                    let removed = end - start;
                    let inserted = b - a;
                    *anchor = if old < start {
                        old
                    } else if removed == 0 || old >= end {
                        old - removed + inserted
                    } else if old == start || inserted == 0 {
                        start
                    } else {
                        start + (old - start) * inserted / removed
                    } as i32;
                }
                let target = buffer.text.as_mut().expect("initialized editable");
                target.splice(start..end, source[a..b].iter().copied());
                buffer.version = buffer
                    .version
                    .checked_add(1)
                    .ok_or_else(|| error("EDITABLE_BUDGET", "version exhausted"))?;
                Value::Obj(r)
            }
            _ => return Ok(None),
        };
        Ok(Some(result))
    }
    #[allow(clippy::too_many_arguments)]
    fn replace_virtual(
        &self,
        vm: &mut Engine,
        r: ObjRef,
        start: i32,
        end: i32,
        source: Value,
        a: i32,
        b: i32,
    ) -> Result<()> {
        vm.invoke_instance(
            r,
            "replace",
            "(IILjava/lang/CharSequence;II)Landroid/text/SpannableStringBuilder;",
            &[
                Value::Int(start),
                Value::Int(end),
                source,
                Value::Int(a),
                Value::Int(b),
            ],
        )?;
        Ok(())
    }
    pub(super) fn collect(&self, marked: &[bool]) {
        self.objects
            .borrow_mut()
            .retain(|r, _| marked.get(*r as usize).copied().unwrap_or(false));
    }
}
