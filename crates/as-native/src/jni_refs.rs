//! Thread-owned local references, process global references and stable member
//! IDs. JNI object tokens root the actual VM heap, not a parallel object model.
use crate::*;
use alloc::collections::BTreeSet;
use rd_jni::Handle;
struct Entry {
    value: Handle,
    owner: Option<u64>,
    id: bool,
}
#[derive(Default)]
pub struct References {
    next: u64,
    entries: BTreeMap<u64, Entry>,
    threads: BTreeMap<u64, Vec<BTreeSet<u64>>>,
}
impl References {
    pub fn attach(&mut self, thread: u64) {
        self.threads
            .entry(thread)
            .or_insert_with(|| vec![BTreeSet::new()]);
    }
    pub fn detach(&mut self, thread: u64) {
        if let Some(frames) = self.threads.remove(&thread) {
            for token in frames.into_iter().flatten() {
                self.entries.remove(&token);
            }
        }
    }
    pub fn get(&self, thread: u64, token: u64) -> Result<&Handle> {
        let entry = self.entries.get(&token).ok_or_else(|| {
            error(
                "JNI_INVALID_REFERENCE",
                "get",
                "JNI",
                format!("deleted or invalid token {token}"),
            )
        })?;
        if entry.owner.is_some_and(|owner| owner != thread) {
            return Err(error(
                "JNI_WRONG_THREAD",
                "get",
                "JNI",
                "reference owned by another thread",
            ));
        }
        Ok(&entry.value)
    }
    fn insert(&mut self, thread: u64, value: Handle, global: bool, id: bool) -> Result<u64> {
        if self.entries.len() >= 65536 {
            return Err(error(
                "JNI_REFERENCE_LIMIT",
                "new",
                "JNI",
                "<=65536 references",
            ));
        }
        self.next = self.next.checked_add(1).ok_or_else(|| {
            error(
                "JNI_REFERENCE_LIMIT",
                "new",
                "JNI",
                "nonwrapping reference IDs",
            )
        })?;
        let token = self.next;
        let owner = if global || id { None } else { Some(thread) };
        if owner.is_some() {
            self.threads
                .get_mut(&thread)
                .and_then(|f| f.last_mut())
                .ok_or_else(|| error("JNI_DETACHED_THREAD", "new", "JNI", "AttachCurrentThread"))?
                .insert(token);
        }
        self.entries.insert(token, Entry { value, owner, id });
        Ok(token)
    }
    pub fn local(&mut self, thread: u64, value: Handle) -> Result<u64> {
        self.insert(thread, value, false, false)
    }
    pub fn id(&mut self, thread: u64, value: Handle) -> Result<u64> {
        if let Some((token, _)) = self
            .entries
            .iter()
            .find(|(_, entry)| entry.id && entry.value == value)
        {
            return Ok(*token);
        }
        self.insert(thread, value, true, true)
    }
    pub fn duplicate(&mut self, thread: u64, token: u64, global: bool) -> Result<u64> {
        if token == 0 {
            return Ok(0);
        }
        let value = self.get(thread, token)?.clone();
        self.insert(thread, value, global, false)
    }
    pub fn delete(&mut self, thread: u64, token: u64, global: bool) -> Result<()> {
        if token == 0 {
            return Ok(());
        }
        self.get(thread, token)?;
        let entry = &self.entries[&token];
        if entry.id || global != entry.owner.is_none() {
            return Err(error(
                "JNI_REFERENCE_KIND",
                "delete",
                "JNI",
                "matching local/global reference",
            ));
        }
        self.entries.remove(&token);
        if let Some(frames) = self.threads.get_mut(&thread) {
            for frame in frames {
                frame.remove(&token);
            }
        }
        Ok(())
    }
    pub fn push(&mut self, thread: u64) -> Result<()> {
        let frames = self
            .threads
            .get_mut(&thread)
            .ok_or_else(|| error("JNI_DETACHED_THREAD", "push", "JNI", "attached thread"))?;
        if frames.len() >= 512 {
            return Err(error(
                "JNI_FRAME_LIMIT",
                "push",
                "JNI",
                "<=512 local frames",
            ));
        }
        frames.push(BTreeSet::new());
        Ok(())
    }
    pub fn pop(&mut self, thread: u64, result: u64) -> Result<u64> {
        let value = if result == 0 {
            None
        } else {
            Some(self.get(thread, result)?.clone())
        };
        let frames = self
            .threads
            .get_mut(&thread)
            .ok_or_else(|| error("JNI_DETACHED_THREAD", "pop", "JNI", "attached thread"))?;
        if frames.len() <= 1 {
            return Err(error("JNI_INVALID_FRAME", "pop", "JNI", "nested frame"));
        }
        for token in frames.pop().unwrap() {
            self.entries.remove(&token);
        }
        value.map(|v| self.local(thread, v)).unwrap_or(Ok(0))
    }
    pub fn roots(&self) -> Vec<u32> {
        self.entries
            .values()
            .filter_map(|e| {
                if let Handle::Object(r) = e.value {
                    Some(r)
                } else {
                    None
                }
            })
            .collect()
    }
    pub fn same(&self, thread: u64, a: u64, b: u64) -> Result<bool> {
        if a == 0 || b == 0 {
            return Ok(a == b);
        }
        Ok(self.get(thread, a)? == self.get(thread, b)?)
    }
    pub fn kind(&self, thread: u64, token: u64) -> Result<u64> {
        self.get(thread, token)?;
        Ok(if self.entries[&token].owner.is_none() {
            2
        } else {
            1
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thread_frames_globals_ids_and_gc_roots() {
        let mut r = References::default();
        r.attach(10);
        r.attach(20);
        let a = r.local(10, Handle::Object(99)).unwrap();
        let global = r.duplicate(10, a, true).unwrap();
        assert!(r.get(20, a).is_err());
        assert_eq!(r.get(20, global).unwrap(), &Handle::Object(99));
        r.push(10).unwrap();
        let ephemeral = r.local(10, Handle::Object(100)).unwrap();
        r.push(20).unwrap();
        let other = r.local(20, Handle::Object(101)).unwrap();
        let returned = r.pop(10, ephemeral).unwrap();
        assert!(r.get(10, ephemeral).is_err());
        assert!(r.roots().contains(&100));
        assert!(r.get(20, other).is_ok());
        r.delete(10, returned, false).unwrap();
        assert!(!r.roots().contains(&100));
        r.detach(10);
        assert!(r.get(20, global).is_ok());
        r.delete(20, global, true).unwrap();
        assert!(!r.roots().contains(&99));
        r.pop(20, 0).unwrap();
        assert!(r.get(20, other).is_err());
    }
}
