use super::*;
use crate::framework::{HostObj, Vis};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Editor {
    pub start: usize,
    pub end: usize,
    pub composing: Option<(usize, usize)>,
    pub batch_depth: u32,
}
fn offset(s: &str, u: usize) -> Result<usize, VmExit> {
    let mut count = 0;
    for (i, c) in s.char_indices() {
        if count == u {
            return Ok(i);
        }
        count += c.len_utf16();
        if count > u {
            return Err(illegal("UTF-16 offset splits surrogate pair"));
        }
    }
    if count == u {
        Ok(s.len())
    } else {
        Err(illegal("UTF-16 offset outside text"))
    }
}
impl Engine {
    pub fn m9_focus(&mut self, target: ObjRef) -> Result<(), VmExit> {
        let valid = matches!(
            self.fw.get(target),
            Some(HostObj::View {
                vis: Vis::Visible,
                enabled: true,
                ..
            })
        );
        if !valid {
            return Err(illegal("focus requires visible enabled view"));
        }
        if !self
            .cp
            .is_subtype(self.heap.class_of(target)?, "Landroid/widget/EditText;")
        {
            return Err(illegal("IME focus requires EditText"));
        }
        let n = self.view_text(target)?.encode_utf16().count();
        self.fw.m9.editors.entry(target).or_insert(Editor {
            start: n,
            end: n,
            ..Default::default()
        });
        let changed = self.fw.m9.focused != Some(target);
        self.fw.m9.focused = Some(target);
        self.fw.focused_view = Some(target); // window focus follows the editor
        if changed {
            self.m9_accessibility_event(target, 8)?;
        }
        Ok(())
    }
    pub fn m9_set_selection(&mut self, start: usize, end: usize) -> Result<(), VmExit> {
        let r = self
            .fw
            .m9
            .focused
            .ok_or_else(|| illegal("no focused editor"))?;
        let s = self.view_text(r)?;
        offset(&s, start)?;
        offset(&s, end)?;
        let e = self
            .fw
            .m9
            .editors
            .get_mut(&r)
            .ok_or_else(|| illegal("editor absent"))?;
        e.start = start;
        e.end = end;
        Ok(())
    }
    pub fn m9_commit_text(
        &mut self,
        text: &str,
        new_cursor: i32,
        composing: bool,
    ) -> Result<String, VmExit> {
        let r = self
            .fw
            .m9
            .focused
            .ok_or_else(|| illegal("no focused editor"))?;
        self.m9_focus(r)?;
        let s = self.view_text(r)?;
        let e = self
            .fw
            .m9
            .editors
            .get(&r)
            .cloned()
            .ok_or_else(|| illegal("editor absent"))?;
        let (start, end) = e
            .composing
            .unwrap_or((e.start.min(e.end), e.start.max(e.end)));
        let bstart = offset(&s, start)?;
        let bend = offset(&s, end)?;
        let mut result = s;
        result.replace_range(bstart..bend, text);
        let inserted = text.encode_utf16().count();
        let len = result.encode_utf16().count();
        let cursor = if new_cursor > 0 {
            start as i64 + inserted as i64 + i64::from(new_cursor) - 1
        } else {
            start as i64 + i64::from(new_cursor)
        }
        .clamp(0, len as i64) as usize;
        offset(&result, cursor)?;
        self.set_view_text(r, result.clone())?;
        let e = self.fw.m9.editors.get_mut(&r).unwrap();
        e.start = cursor;
        e.end = cursor;
        e.composing = if composing {
            Some((start, start + inserted))
        } else {
            None
        };
        if e.batch_depth == 0 {
            self.m9_accessibility_event(r, 16)?;
        }
        Ok(result)
    }
    pub fn m9_delete_surrounding(
        &mut self,
        before: usize,
        after: usize,
        codepoints: bool,
    ) -> Result<String, VmExit> {
        let r = self
            .fw
            .m9
            .focused
            .ok_or_else(|| illegal("no focused editor"))?;
        let s = self.view_text(r)?;
        let e = self.fw.m9.editors.get(&r).cloned().unwrap_or_default();
        let start = e.start.min(e.end);
        let end = e.start.max(e.end);
        let len = s.encode_utf16().count();
        let (mut left, mut right) = (
            start.saturating_sub(before),
            end.saturating_add(after).min(len),
        );
        if codepoints {
            let bstart = offset(&s, start)?;
            let bend = offset(&s, end)?;
            left = start
                - s[..bstart]
                    .chars()
                    .rev()
                    .take(before)
                    .map(char::len_utf16)
                    .sum::<usize>();
            right = end
                + s[bend..]
                    .chars()
                    .take(after)
                    .map(char::len_utf16)
                    .sum::<usize>();
        }
        let (bl, bs, be, br) = (
            offset(&s, left)?,
            offset(&s, start)?,
            offset(&s, end)?,
            offset(&s, right)?,
        );
        let result = format!("{}{}{}", &s[..bl], &s[bs..be], &s[br..]);
        self.set_view_text(r, result.clone())?;
        let e = self.fw.m9.editors.get_mut(&r).unwrap();
        e.start = left;
        e.end = left + end - start;
        e.composing = None;
        self.m9_accessibility_event(r, 16)?;
        Ok(result)
    }
    pub fn m9_register_ime(&mut self, service: ObjRef) -> Result<(), VmExit> {
        if self.fw.m9.ime_service == Some(service) {
            return Ok(());
        }
        self.fw.m9.ime_service = Some(service);
        call_dex(self, service, "onInitializeInterface", "()V", vec![])?;
        Ok(())
    }
    fn m9_start_input(&mut self) -> Result<(), VmExit> {
        if let Some(service) = self.fw.m9.ime_service {
            let info = alloc(self, "Landroid/view/inputmethod/EditorInfo;")?;
            if let Some(r) = self.fw.m9.focused {
                let e = self.fw.m9.editors[&r].clone();
                self.heap
                    .put_field(info, "initialSelStart", Value::Int(e.start as i32))
                    .map_err(|e| format!("{e:?}"))?;
                self.heap
                    .put_field(info, "initialSelEnd", Value::Int(e.end as i32))
                    .map_err(|e| format!("{e:?}"))?;
            }
            call_dex(
                self,
                service,
                "onStartInput",
                "(Landroid/view/inputmethod/EditorInfo;Z)V",
                vec![Value::Obj(info), Value::Int(0)],
            )?;
            call_dex(
                self,
                service,
                "onStartInputView",
                "(Landroid/view/inputmethod/EditorInfo;Z)V",
                vec![Value::Obj(info), Value::Int(0)],
            )?;
        }
        Ok(())
    }
}
/// The m9 editor model (selection, composing, IME service) covers host
/// EditTexts; any other view (Compose's AndroidComposeView, custom
/// editors) is focused by the window model in `view_layout` and edits
/// through its own `onCreateInputConnection`.
fn is_edit_text(vm: &mut Engine, r: ObjRef) -> bool {
    match vm.heap.class_of(r) {
        Ok(c) => {
            let c = c.to_string();
            vm.cp.is_subtype(&c, "Landroid/widget/EditText;")
        }
        Err(_) => false,
    }
}
pub(super) fn instance_call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class.starts_with("Landroid/view/") || class.starts_with("Landroid/widget/") {
        match name {
            "requestFocus" if sig.starts_with("()") && is_edit_text(vm, recv) => {
                vm.m9_focus(recv)?;
                return Ok(Some(Value::Int(1)));
            }
            "clearFocus" if is_edit_text(vm, recv) => {
                if vm.fw.focused_view == Some(recv) {
                    vm.fw.focused_view = None;
                }
                if vm.fw.m9.focused == Some(recv) {
                    vm.fw.m9.focused = None;
                    vm.fw.m9.keyboard_visible = false;
                }
                return Ok(Some(Value::Null));
            }
            "isFocused" | "hasFocus" if is_edit_text(vm, recv) => {
                return Ok(Some(Value::Int((vm.fw.m9.focused == Some(recv)) as i32)))
            }
            "setSelection" => {
                vm.m9_focus(recv)?;
                let start = int(args, 0);
                let end = if args.len() > 1 { int(args, 1) } else { start };
                if start < 0 || end < 0 {
                    return Err(illegal("negative selection"));
                }
                vm.m9_set_selection(start as usize, end as usize)?;
                return Ok(Some(Value::Null));
            }
            "getSelectionStart" | "getSelectionEnd" => {
                let n = vm
                    .fw
                    .m9
                    .editors
                    .get(&recv)
                    .map(|e| {
                        if name == "getSelectionStart" {
                            e.start
                        } else {
                            e.end
                        }
                    })
                    .unwrap_or(0);
                return Ok(Some(Value::Int(n as i32)));
            }
            _ => {}
        }
    }
    // EditorInfo built by the runtime for onCreateInputConnection (and by
    // apps): the initial-surrounding-text API stores what the editor
    // reports so IME-side reads return it
    if class == "Landroid/view/inputmethod/EditorInfo;" {
        let value = match name {
            "<init>" => Value::Null,
            "setInitialSurroundingText" | "setInitialSurroundingSubText" => {
                let v = args.first().cloned().unwrap_or(Value::Null);
                crate::err::put_field_result(vm.heap.put_field(recv, "rd_surrounding", v))?;
                Value::Null
            }
            "getInitialTextBeforeCursor"
            | "getInitialSelectedText"
            | "getInitialTextAfterCursor" => {
                if name == "getInitialTextBeforeCursor" {
                    vm.heap
                        .get_field(recv, "rd_surrounding")
                        .unwrap_or(Value::Null)
                } else {
                    Value::Null
                }
            }
            // remaining API-33+ setters (handwriting gestures, preview
            // ...) are accepted; getters answer the unset default
            _ if sig.ends_with(")V") => Value::Null,
            _ if sig.ends_with(")Z") || sig.ends_with(")I") => Value::Int(0),
            _ if sig.contains(")L") => Value::Null,
            _ => return Ok(None),
        };
        return Ok(Some(value));
    }
    // ClipboardManager: one primary clip per device, kept on the service
    // object (setPrimaryClip → getPrimaryClip/hasPrimaryClip read it back)
    if class == "Landroid/content/ClipboardManager;" {
        let clip = match vm.heap.get_field(recv, "rd_primary_clip") {
            Ok(Value::Obj(c)) if c != 0 => Some(c),
            _ => None,
        };
        let value = match name {
            "setPrimaryClip" => {
                let v = args.first().cloned().unwrap_or(Value::Null);
                crate::err::put_field_result(vm.heap.put_field(recv, "rd_primary_clip", v))?;
                Value::Null
            }
            "clearPrimaryClip" => {
                crate::err::put_field_result(vm.heap.put_field(
                    recv,
                    "rd_primary_clip",
                    Value::Null,
                ))?;
                Value::Null
            }
            "getPrimaryClip" => clip.map_or(Value::Null, Value::Obj),
            "hasPrimaryClip" | "hasText" => Value::Int(clip.is_some() as i32),
            "getPrimaryClipDescription" => match clip {
                Some(c) => vm.invoke_instance(
                    c,
                    "getDescription",
                    "()Landroid/content/ClipDescription;",
                    &[],
                )?,
                None => Value::Null,
            },
            "addPrimaryClipChangedListener" | "removePrimaryClipChangedListener" => Value::Null,
            _ => return Ok(None),
        };
        return Ok(Some(value));
    }
    if class == "Landroid/view/inputmethod/InputMethodManager;" {
        let value = match name {
            "showSoftInput" => {
                let target = obj(args, 0).ok_or_else(|| illegal("showSoftInput view null"))?;
                if !is_edit_text(vm, target) {
                    // non-EditText IME target (Compose): keyboard up, the
                    // view edits through its own InputConnection
                    vm.fw.m9.keyboard_visible = true;
                    return Ok(Some(Value::Int(1)));
                }
                vm.m9_focus(target)?;
                vm.fw.m9.keyboard_visible = true;
                vm.m9_start_input()?;
                Value::Int(1)
            }
            "hideSoftInputFromWindow" => {
                let was = vm.fw.m9.keyboard_visible;
                vm.fw.m9.keyboard_visible = false;
                if let Some(s) = vm.fw.m9.ime_service {
                    call_dex(vm, s, "onFinishInputView", "(Z)V", vec![Value::Int(0)])?;
                }
                Value::Int(was as i32)
            }
            // editor -> IME notifications while typing: no on-screen IME
            // to repaint, accepted
            "updateSelection"
            | "updateCursorAnchorInfo"
            | "updateExtractedText"
            | "invalidateInput"
            | "viewClicked"
            | "displayCompletions"
            | "updateCursor"
            | "startStylusHandwriting"
            | "prepareStylusHandwritingDelegation" => Value::Null,
            "isFullscreenMode" | "isInputMethodSuppressingSpellChecker" => Value::Int(0),
            "isActive" | "isAcceptingText" => Value::Int(
                (vm.fw.m9.focused.is_some()
                    && (args.is_empty() || obj(args, 0) == vm.fw.m9.focused))
                    as i32,
            ),
            "restartInput" => {
                if let Some(r) = obj(args, 0) {
                    if !is_edit_text(vm, r) {
                        return Ok(Some(Value::Null));
                    }
                    vm.m9_focus(r)?;
                }
                vm.m9_start_input()?;
                Value::Null
            }
            _ => return Ok(None),
        };
        return Ok(Some(value));
    }
    if class == "Landroid/inputmethodservice/InputMethodService;" {
        let value = match name {
            "<init>" => Value::Null,
            "onCreate" => {
                vm.m9_register_ime(recv)?;
                Value::Null
            }
            "onDestroy" => {
                vm.fw.m9.ime_service = None;
                vm.fw.m9.keyboard_visible = false;
                Value::Null
            }
            "getCurrentInputConnection" => {
                let target = vm
                    .fw
                    .m9
                    .focused
                    .ok_or_else(|| illegal("no current input"))?;
                let r = alloc(vm, "Landroid/view/inputmethod/BaseInputConnection;")?;
                vm.fw.m9.connections.insert(r, target);
                Value::Obj(r)
            }
            "isInputViewShown" => Value::Int(vm.fw.m9.keyboard_visible as i32),
            "requestHideSelf" => {
                vm.fw.m9.keyboard_visible = false;
                Value::Null
            }
            "onStartInput"
            | "onStartInputView"
            | "onFinishInputView"
            | "onFinishInput"
            | "onInitializeInterface" => Value::Null,
            _ => return Ok(None),
        };
        return Ok(Some(value));
    }
    if matches!(
        class,
        "Landroid/view/inputmethod/BaseInputConnection;"
            | "Landroid/view/inputmethod/InputConnection;"
    ) {
        if name == "<init>" {
            let target = obj(args, 0).ok_or_else(|| illegal("BaseInputConnection target null"))?;
            vm.m9_focus(target)?;
            vm.fw.m9.connections.insert(recv, target);
            return Ok(Some(Value::Null));
        }
        let target = *vm
            .fw
            .m9
            .connections
            .get(&recv)
            .ok_or_else(|| illegal("input connection not attached"))?;
        if vm.fw.m9.focused != Some(target) {
            return Ok(Some(Value::Int(0)));
        }
        let value = match name {
            "commitText" | "setComposingText" => {
                let s = text(vm, args, 0)?;
                vm.m9_commit_text(&s, int(args, 1), name == "setComposingText")?;
                Value::Int(1)
            }
            "finishComposingText" => {
                vm.fw.m9.editors.get_mut(&target).unwrap().composing = None;
                Value::Int(1)
            }
            "setComposingRegion" => {
                let s = vm.view_text(target)?;
                let a = int(args, 0).max(0) as usize;
                let b = int(args, 1).max(0) as usize;
                offset(&s, a)?;
                offset(&s, b)?;
                vm.fw.m9.editors.get_mut(&target).unwrap().composing = Some((a.min(b), a.max(b)));
                Value::Int(1)
            }
            "setSelection" => {
                if int(args, 0) < 0 || int(args, 1) < 0 {
                    return Err(illegal("negative selection"));
                }
                vm.m9_set_selection(int(args, 0) as usize, int(args, 1) as usize)?;
                Value::Int(1)
            }
            "deleteSurroundingText" | "deleteSurroundingTextInCodePoints" => {
                if int(args, 0) < 0 || int(args, 1) < 0 {
                    return Err(illegal("negative deletion"));
                }
                vm.m9_delete_surrounding(
                    int(args, 0) as usize,
                    int(args, 1) as usize,
                    name.ends_with("InCodePoints"),
                )?;
                Value::Int(1)
            }
            "getTextBeforeCursor" | "getTextAfterCursor" | "getSelectedText" => {
                let s = vm.view_text(target)?;
                let e = vm.fw.m9.editors[&target].clone();
                let lo = e.start.min(e.end);
                let hi = e.start.max(e.end);
                let n = int(args, 0).max(0) as usize;
                let (a, b) = match name {
                    "getTextBeforeCursor" => (lo.saturating_sub(n), lo),
                    "getTextAfterCursor" => {
                        (hi, hi.saturating_add(n).min(s.encode_utf16().count()))
                    }
                    _ => (lo, hi),
                };
                let bstart = offset(&s, a)?;
                let bend = offset(&s, b)?;
                string(vm, &s[bstart..bend])?
            }
            "beginBatchEdit" => {
                let e = vm.fw.m9.editors.get_mut(&target).unwrap();
                e.batch_depth = e.batch_depth.saturating_add(1);
                Value::Int(1)
            }
            "endBatchEdit" => {
                let e = vm.fw.m9.editors.get_mut(&target).unwrap();
                if e.batch_depth == 0 {
                    Value::Int(0)
                } else {
                    e.batch_depth -= 1;
                    if e.batch_depth == 0 {
                        vm.m9_accessibility_event(target, 16)?;
                    }
                    Value::Int(1)
                }
            }
            "performEditorAction" => {
                let action = int(args, 0);
                vm.fw.m9.editor_actions.push(action);
                if let Some(s) = vm.fw.m9.ime_service {
                    call_dex(
                        vm,
                        s,
                        "sendDefaultEditorAction",
                        "(Z)Z",
                        vec![Value::Int(1)],
                    )?;
                }
                Value::Int(1)
            }
            "closeConnection" => {
                vm.fw.m9.connections.remove(&recv);
                Value::Null
            }
            _ => return Ok(None),
        };
        return Ok(Some(value));
    }
    Ok(None)
}
