//! Heap arena da VM com os orçamentos do piso moto-e5 (spec A2/PRF-06):
//! heap do app ≤ 256 MB. Alocação que exceder o orçamento responde erro
//! tipado `VM_OOM` — nunca abort (Lei 1). Objetos inalcançáveis são
//! coletados pelo mark-sweep de `crate::gc` em safe points (slots liberados
//! viram tombstones e são reciclados pela free list); o resto é liberado de
//! uma vez quando o `Engine` morre.

use crate::value::Value;

/// Referência de objeto (índice + 1 na arena; 0 = inválido).
pub type ObjRef = u32;

/// Teto default do heap do app = 256 MB (piso E5: dalvik.vm.heapgrowthlimit
/// de um device de 2 GB).
pub const DEFAULT_HEAP_BUDGET: usize = 256 * 1024 * 1024;
/// Teto absoluto de objetos — independe do orçamento em bytes, protege o
/// custo O(1) de índice e evita explosão de metadados (arena é Vec contíguo).
pub const MAX_OBJECTS: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElemKind {
    Int,
    Long,
    Float,
    Double,
    Obj,
}

impl ElemKind {
    /// Tamanho contabilizado por elemento (para o orçamento em bytes).
    pub fn byte_size(self) -> usize {
        match self {
            ElemKind::Int | ElemKind::Float => 4,
            ElemKind::Long | ElemKind::Double | ElemKind::Obj => 8,
        }
    }

    /// `new-array` recebe o descritor do tipo de elemento (`[I`, `[J`, `[D`,
    /// `[F`, `[Ljava/lang/String;`…).
    pub fn from_array_desc(desc: &str) -> Option<ElemKind> {
        match desc {
            "[I" | "[Z" | "[B" | "[C" | "[S" => Some(ElemKind::Int),
            "[J" => Some(ElemKind::Long),
            "[F" => Some(ElemKind::Float),
            "[D" => Some(ElemKind::Double),
            d if d.starts_with("[L") || d.starts_with("[[") => Some(ElemKind::Obj),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum HeapObj {
    Str(String),
    Array {
        elem: ElemKind,
        elems: Vec<Value>,
        /// descritor do tipo do elemento para arrays de objetos (`[LX;` —
        /// issue #37); vazio/ignorado para primitivos
        elem_class: String,
    },
    Instance {
        /// descritor da classe (`Ljava/lang/StringBuilder;`, `LCaso;`…)
        class: String,
        /// campos de instância em ordem de declaração; iget/iput resolvem por nome
        fields: Vec<(String, Value)>,
    },
}

/// Private arena storage: vacant slots need no per-slot tombstone String.
/// The public HeapObj projection (including Debug and mutable access) retains
/// the existing tombstone contract. ObjRef values and LIFO reuse do not change.
#[repr(transparent)]
struct HeapSlot(Option<HeapObj>);

fn freed_object() -> &'static HeapObj {
    static FREED: std::sync::OnceLock<HeapObj> = std::sync::OnceLock::new();
    FREED.get_or_init(|| HeapObj::Instance {
        class: crate::gc::FREED_CLASS.to_string(),
        fields: Vec::new(),
    })
}

impl HeapSlot {
    fn value(&self) -> &HeapObj {
        match &self.0 {
            Some(value) => value,
            None => freed_object(),
        }
    }
}

impl std::fmt::Debug for HeapSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.value(), f)
    }
}

#[derive(Debug)]
pub struct OomError {
    pub requested: usize,
    pub budget: usize,
}

#[derive(Debug, Default)]
pub struct Heap {
    budget: usize,
    used: usize,
    /// Marca d'água (M7): maior `used` já observado — o RSS do heap do app
    /// para o orçamento `ram_app_heap_mb` (com o GC, `used` cai a cada ciclo;
    /// o contrato do gate é o pico).
    peak: usize,
    objects: Vec<HeapSlot>,
    /// Slots freed by the GC, reused LIFO by `alloc` (see `gc.rs`).
    free: Vec<ObjRef>,
    young: Vec<bool>,
    remembered: std::collections::HashSet<ObjRef>,
    // Some = host scope (pin allocations); None = guest execution.
    gc_scopes: Vec<Option<Vec<ObjRef>>>,
    births: Vec<u64>,
    allocation_clock: u64,
    existing_cutoffs: Vec<(usize, u64)>,
}

impl Heap {
    pub fn new(budget: usize) -> Self {
        Heap {
            budget,
            used: 0,
            peak: 0,
            objects: Vec::new(),
            free: Vec::new(),
            young: Vec::new(),
            remembered: Default::default(),
            gc_scopes: Vec::new(),
            births: Vec::new(),
            allocation_clock: 0,
            existing_cutoffs: Vec::new(),
        }
    }

    pub(crate) fn is_young(&self, r: ObjRef) -> bool {
        self.young
            .get(r.wrapping_sub(1) as usize)
            .copied()
            .unwrap_or(true)
    }
    pub(crate) fn remembered(&self) -> impl Iterator<Item = ObjRef> + '_ {
        self.remembered.iter().copied()
    }
    pub(crate) fn promote(&mut self) {
        self.young.fill(false);
        // Free slots are never old; keep them unmarked in minor collections.
        for r in &self.free {
            self.young[(*r - 1) as usize] = true;
        }
        self.remembered.clear();
    }
    pub(crate) fn gc_scope_depth(&self) -> usize {
        self.gc_scopes.len()
    }
    pub(crate) fn gc_host_enter(&mut self, existing: bool) {
        if existing {
            self.existing_cutoffs
                .push((self.gc_scopes.len(), self.allocation_clock));
        }
        self.gc_scopes.push(Some(Vec::new()));
    }
    /// A nested Rust host call can return a temporary to its enclosing host
    /// callback. Preserve all nested pins conservatively until that callback
    /// returns, but never promote them through an intervening Java frame.
    pub(crate) fn gc_host_restore(&mut self, depth: usize) {
        let roots = self
            .gc_scopes
            .get_mut(depth)
            .and_then(Option::take)
            .unwrap_or_default();
        self.gc_scope_restore(depth);
        if let Some(Some(parent)) = self.gc_scopes.last_mut() {
            parent.extend(roots);
        }
    }

    pub(crate) fn gc_guest_enter(&mut self) {
        self.gc_scopes.push(None);
    }
    pub(crate) fn gc_scope_restore(&mut self, depth: usize) {
        self.gc_scopes.truncate(depth);
        self.existing_cutoffs.retain(|(d, _)| *d < depth);
    }
    pub(crate) fn gc_pinned_roots(&self) -> impl Iterator<Item = ObjRef> + '_ {
        let cutoff = self
            .existing_cutoffs
            .iter()
            .map(|(_, c)| *c)
            .max()
            .unwrap_or(0);
        self.gc_scopes.iter().filter_map(Option::as_ref).flatten().copied().chain(
            self.births.iter().enumerate().filter(move |(_, birth)| **birth <= cutoff)
                .map(|(i, _)| (i + 1) as ObjRef).filter(|r|
                    !matches!(self.get(*r), Ok(HeapObj::Instance { class, .. }) if class == crate::gc::FREED_CLASS)
                )
        )
    }
    pub(crate) fn gc_pin_return(&mut self, r: ObjRef) {
        self.pin_allocation(r);
    }

    fn pin_allocation(&mut self, r: ObjRef) {
        if let Some(Some(roots)) = self.gc_scopes.last_mut() {
            roots.push(r);
        }
    }

    pub fn budget(&self) -> usize {
        self.budget
    }

    pub fn used(&self) -> usize {
        self.used
    }

    /// Maior `used` observado desde a criação da arena (M7, orçamento do app).
    pub fn peak(&self) -> usize {
        self.peak
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    fn alloc(&mut self, bytes: usize, obj: HeapObj) -> Result<ObjRef, OomError> {
        if self.free.is_empty() && self.objects.len() >= MAX_OBJECTS {
            return Err(OomError {
                requested: bytes,
                budget: self.budget,
            });
        }
        let new_used = self.used.checked_add(bytes).ok_or(OomError {
            requested: bytes,
            budget: self.budget,
        })?;
        if new_used > self.budget {
            return Err(OomError {
                requested: bytes,
                budget: self.budget,
            });
        }
        self.used = new_used;
        self.peak = self.peak.max(new_used);
        if let Some(r) = self.free.pop() {
            self.objects[(r - 1) as usize].0 = Some(obj);
            self.young[(r - 1) as usize] = true;
            self.allocation_clock += 1;
            self.births[(r - 1) as usize] = self.allocation_clock;
            self.pin_allocation(r);
            return Ok(r);
        }
        self.objects.push(HeapSlot(Some(obj)));
        self.young.push(true);
        self.allocation_clock += 1;
        self.births.push(self.allocation_clock);
        let r = self.objects.len() as ObjRef;
        self.pin_allocation(r);
        Ok(r)
    }

    pub fn alloc_string(&mut self, s: String) -> Result<ObjRef, OomError> {
        let bytes = s.len().saturating_mul(2).saturating_add(32);
        self.alloc(bytes, HeapObj::Str(s))
    }

    /// String constructors initialize an already allocated DEX `new-instance`
    /// shell. Replace that shell in place so the existing ObjRef stays stable.
    pub fn initialize_string(&mut self, r: ObjRef, s: String) -> Result<(), OomError> {
        if r == 0 || r as usize > self.objects.len() {
            return Err(OomError {
                requested: 32,
                budget: self.budget,
            });
        }
        let index = (r - 1) as usize;
        let old_bytes = match self.objects[index].value() {
            HeapObj::Instance { fields, .. } => {
                32usize.saturating_add(fields.len().saturating_mul(24))
            }
            HeapObj::Str(old) => old.len().saturating_mul(2).saturating_add(32),
            _ => {
                return Err(OomError {
                    requested: 32,
                    budget: self.budget,
                })
            }
        };
        let new_bytes = s.len().saturating_mul(2).saturating_add(32);
        let used_without_old = self.used.saturating_sub(old_bytes);
        let new_used = used_without_old.checked_add(new_bytes).ok_or(OomError {
            requested: new_bytes,
            budget: self.budget,
        })?;
        if new_used > self.budget {
            return Err(OomError {
                requested: new_bytes,
                budget: self.budget,
            });
        }
        self.used = new_used;
        self.peak = self.peak.max(new_used);
        self.objects[index].0 = Some(HeapObj::Str(s));
        Ok(())
    }

    /// `alloc_array` with every element set to the JVM default for its kind
    /// (0, 0L, 0.0f, 0.0, null) — use this for buffers handed to guest code
    /// (`read(byte[], off, len)` scratch arrays, host-created arrays).
    pub fn alloc_array_zeroed(
        &mut self,
        elem: ElemKind,
        len: usize,
        elem_class: &str,
    ) -> Result<ObjRef, OomError> {
        let r = self.alloc_array(elem, len, elem_class)?;
        let fill = match elem {
            ElemKind::Int => Value::Int(0),
            ElemKind::Long => Value::Long(0),
            ElemKind::Float => Value::Float(0.0),
            ElemKind::Double => Value::Double(0.0),
            ElemKind::Obj => Value::Null,
        };
        if let Ok(e) = self.array_elems_mut(r) {
            e.resize(len, fill);
        }
        Ok(r)
    }

    pub fn alloc_array(
        &mut self,
        elem: ElemKind,
        len: usize,
        elem_class: &str,
    ) -> Result<ObjRef, OomError> {
        // issue #28: o orçamento cobra o custo REAL de armazenamento na arena
        // (Value = 32 B), não o tamanho lógico do elemento (4/8 B) — com a
        // contabilidade antiga o RSS real chegava a ~8× o orçamento (DoS)
        let bytes = len
            .saturating_mul(std::mem::size_of::<Value>())
            .saturating_add(16);
        let array_desc = if elem_class.starts_with('[') {
            elem_class.to_string()
        } else if elem == ElemKind::Obj {
            format!("[{elem_class}")
        } else {
            match elem {
                ElemKind::Int => "[I".to_string(),
                ElemKind::Long => "[J".to_string(),
                ElemKind::Float => "[F".to_string(),
                ElemKind::Double => "[D".to_string(),
                ElemKind::Obj => unreachable!(),
            }
        };
        self.alloc(
            bytes,
            HeapObj::Array {
                elem,
                elems: Vec::new(),
                // Keep the full runtime array descriptor. ElemKind alone loses
                // the distinction between char[], byte[], short[], boolean[],
                // and int[] even though all use the same storage lane.
                elem_class: array_desc,
            },
        )
    }

    pub fn alloc_instance(
        &mut self,
        class: String,
        fields: Vec<(String, Value)>,
    ) -> Result<ObjRef, OomError> {
        let bytes = 32 + fields.len().saturating_mul(24);
        self.alloc(bytes, HeapObj::Instance { class, fields })
    }

    /// GC sweep: drops the contents of slot `r`, leaving a tombstone instance
    /// of `crate::gc::FREED_CLASS`, queues the slot for reuse, and returns
    /// the bytes given back to the budget. `None` if `r` is invalid or
    /// already freed.
    /// Number of recycled slots waiting for reuse.
    pub fn free_slots(&self) -> usize {
        self.free.len()
    }

    pub fn free_slot(&mut self, r: ObjRef) -> Option<usize> {
        if r == 0 || r as usize > self.objects.len() {
            return None;
        }
        let slot = &mut self.objects[(r - 1) as usize];
        let bytes = match slot.0.as_ref()? {
            HeapObj::Instance { class, .. } if class == crate::gc::FREED_CLASS => return None,
            HeapObj::Instance { fields, .. } => {
                fields
                    .iter()
                    .map(|(n, _)| std::mem::size_of::<Value>() + n.len())
                    .sum::<usize>()
                    .max(fields.len() * 24)
                    + 32
            }
            HeapObj::Str(s) => s.len().saturating_mul(2).saturating_add(32),
            HeapObj::Array { elems, .. } => elems
                .len()
                .saturating_mul(std::mem::size_of::<Value>())
                .saturating_add(16),
        };
        slot.0 = None;
        self.free.push(r);
        self.young[(r - 1) as usize] = true;
        self.remembered.remove(&r);
        let bytes = bytes.min(self.used);
        self.used -= bytes;
        Some(bytes)
    }

    pub fn get(&self, r: ObjRef) -> Result<&HeapObj, String> {
        if r == 0 || r as usize > self.objects.len() {
            return Err(format!("referência inválida #{r}"));
        }
        Ok(self.objects[(r - 1) as usize].value())
    }

    pub fn get_mut(&mut self, r: ObjRef) -> Result<&mut HeapObj, String> {
        if r == 0 || r as usize > self.objects.len() {
            return Err(format!("referência inválida #{r}"));
        }
        if !self.young[(r - 1) as usize] {
            self.remembered.insert(r);
        }
        // Mutable access must never expose the shared immutable tombstone.
        // Materialize the same owned instance lazily only if a caller requests it.
        Ok(self.objects[(r - 1) as usize]
            .0
            .get_or_insert_with(|| freed_object().clone()))
    }

    pub fn as_str(&self, r: ObjRef) -> Result<&str, String> {
        match self.get(r)? {
            HeapObj::Str(s) => Ok(s),
            other => Err(format!("esperado String, got {:?}", other.kind_name())),
        }
    }

    pub fn as_array_elems(&self, r: ObjRef) -> Result<(&[Value], ElemKind), String> {
        match self.get(r)? {
            HeapObj::Array { elem, elems, .. } => Ok((elems, *elem)),
            other => Err(format!("esperado array, got {:?}", other.kind_name())),
        }
    }

    pub fn array_elems_mut(&mut self, r: ObjRef) -> Result<&mut Vec<Value>, String> {
        match self.get_mut(r)? {
            HeapObj::Array { elems, .. } => Ok(elems),
            other => Err(format!("esperado array, got {:?}", other.kind_name())),
        }
    }

    pub fn as_instance(&self, r: ObjRef) -> Result<InstanceView<'_>, String> {
        match self.get(r)? {
            HeapObj::Instance { class, fields } => Ok((class, fields)),
            other => Err(format!("esperado instância, got {:?}", other.kind_name())),
        }
    }

    /// class de uma instância (para dispatch virtual e instanceof/catch).
    pub fn class_of(&self, r: ObjRef) -> Result<&str, String> {
        match self.get(r)? {
            HeapObj::Instance { class, .. } => Ok(class),
            HeapObj::Str(_) => Ok("Ljava/lang/String;"),
            HeapObj::Array { elem_class, .. } => Ok(elem_class),
        }
    }

    pub fn get_field(&self, r: ObjRef, name: &str) -> Result<Value, String> {
        match self.get(r)? {
            HeapObj::Instance { fields, .. } => fields
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
                .ok_or_else(|| format!("campo {name} ausente na instância #{r}")),
            _ => Err(format!("#{r} não é instância")),
        }
    }

    /// issue #49: escrita de campo é erro TIPADO — OOM não pode vira
    /// `VM_TYPE_ERROR` mentiroso pelo `From<String>` do interp.
    pub fn put_field(&mut self, r: ObjRef, name: &str, v: Value) -> Result<(), FieldWriteErr> {
        // issue #28: campo novo é cobrado no orçamento ANTES de materializar
        // (o push antigo não cobrava nada — leak de contabilidade). O check
        // vem antes do get_mut para não conflitar o borrow
        let is_new = match self.get(r) {
            Ok(HeapObj::Instance { fields, .. }) => !fields.iter().any(|(n, _)| n == name),
            Ok(_) => return Err(FieldWriteErr::NotInstance),
            Err(_) => return Err(FieldWriteErr::NotInstance),
        };
        if is_new {
            let cost = std::mem::size_of::<Value>() + name.len();
            let new_used = self
                .used
                .checked_add(cost)
                .ok_or(FieldWriteErr::Oom(OomError {
                    requested: cost,
                    budget: self.budget,
                }))?;
            if new_used > self.budget {
                return Err(FieldWriteErr::Oom(OomError {
                    requested: cost,
                    budget: self.budget,
                }));
            }
            self.used = new_used;
            self.peak = self.peak.max(new_used);
        }
        match self.get_mut(r) {
            Ok(HeapObj::Instance { fields, .. }) => {
                if let Some(slot) = fields.iter_mut().find(|(n, _)| n == name) {
                    slot.1 = v;
                } else {
                    fields.push((name.to_string(), v));
                }
                Ok(())
            }
            _ => Err(FieldWriteErr::NotInstance),
        }
    }
}

/// issue #49: escrita de campo é erro TIPADO — OOM não pode virar
/// `VM_TYPE_ERROR` mentiroso pelo `From<String>` do interp.
#[derive(Debug)]
pub enum FieldWriteErr {
    Oom(OomError),
    NotInstance,
}

/// (classe, campos) de uma instância — alias para o tipo complexo do clippy.
type InstanceView<'h> = (&'h str, &'h [(String, Value)]);

impl HeapObj {
    pub fn kind_name(&self) -> &'static str {
        match self {
            HeapObj::Str(_) => "String",
            HeapObj::Array { .. } => "array",
            HeapObj::Instance { .. } => "instance",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vacant_slot_layout_and_public_tombstone_contract_are_unchanged() {
        assert_eq!(
            std::mem::size_of::<HeapSlot>(),
            std::mem::size_of::<HeapObj>()
        );
        let mut heap = Heap::new(1024);
        let r = heap.alloc_instance("LOriginal;".into(), vec![]).unwrap();
        assert_eq!(heap.free_slot(r), Some(32));
        assert!(heap.objects[(r - 1) as usize].0.is_none());
        assert_eq!(heap.class_of(r), Ok(crate::gc::FREED_CLASS));
        let expected = HeapObj::Instance {
            class: crate::gc::FREED_CLASS.to_string(),
            fields: vec![],
        };
        assert_eq!(
            format!("{:?}", heap.get(r).unwrap()),
            format!("{expected:?}")
        );
        assert_eq!(
            format!("{:#?}", heap.get(r).unwrap()),
            format!("{expected:#?}")
        );
        assert_eq!(heap.free_slot(r), None);
        // Never hand a mutable reference to the shared immutable projection.
        if let HeapObj::Instance { fields, .. } = heap.get_mut(r).unwrap() {
            fields.push(("private".into(), Value::Int(17)));
        }
        assert_eq!(heap.get_field(r, "private"), Ok(Value::Int(17)));
        let mut another = Heap::new(1024);
        let other = another.alloc_string("gone".into()).unwrap();
        another.free_slot(other).unwrap();
        assert!(another.get_field(other, "private").is_err());
        assert_eq!(heap.alloc_string("replacement".into()).unwrap(), r);
        assert_eq!(heap.as_str(r), Ok("replacement"));
        assert_eq!(heap.free_slots(), 0);
    }

    #[test]
    fn compact_slots_differential_against_materialized_legacy_tombstones() {
        let mut compact = Heap::new(8192);
        let mut legacy = Heap::new(8192);
        let mut state = 0x7300_1020_5eedu64;
        for step in 0..8192 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let reference = (state as usize % (compact.len() + 3)) as ObjRef;
            match state % 7 {
                0 => {
                    let a = compact.alloc_instance(
                        "LRecord;".into(),
                        vec![("value".into(), Value::Int(step))],
                    );
                    let b = legacy.alloc_instance(
                        "LRecord;".into(),
                        vec![("value".into(), Value::Int(step))],
                    );
                    assert_eq!(format!("{a:?}"), format!("{b:?}"));
                }
                1 => {
                    let text = "x🌳".repeat((state as usize >> 8) % 21);
                    let a = compact.alloc_string(text.clone());
                    let b = legacy.alloc_string(text);
                    assert_eq!(format!("{a:?}"), format!("{b:?}"));
                }
                2 => {
                    assert_eq!(compact.free_slot(reference), legacy.free_slot(reference));
                    // This is exactly the old storage representation after a
                    // free; the comparison heap keeps real owned tombstones.
                    for slot in &mut legacy.objects {
                        if slot.0.is_none() {
                            slot.0 = Some(freed_object().clone());
                        }
                    }
                }
                3 => {
                    let a = compact.put_field(reference, "value", Value::Int(step));
                    let b = legacy.put_field(reference, "value", Value::Int(step));
                    assert_eq!(format!("{a:?}"), format!("{b:?}"));
                }
                4 => {
                    let a = compact.initialize_string(reference, "initialized🌳".into());
                    let b = legacy.initialize_string(reference, "initialized🌳".into());
                    assert_eq!(format!("{a:?}"), format!("{b:?}"));
                }
                5 => {
                    compact.promote();
                    legacy.promote();
                }
                _ => {
                    let length = ((state >> 16) % 17) as usize;
                    let a = compact.alloc_array_zeroed(ElemKind::Int, length, "[I");
                    let b = legacy.alloc_array_zeroed(ElemKind::Int, length, "[I");
                    assert_eq!(format!("{a:?}"), format!("{b:?}"));
                }
            }
            assert_eq!(
                (compact.used(), compact.peak(), compact.len()),
                (legacy.used(), legacy.peak(), legacy.len())
            );
            assert_eq!(compact.free, legacy.free);
            assert_eq!(compact.young, legacy.young);
            assert_eq!(compact.births, legacy.births);
            assert_eq!(compact.remembered, legacy.remembered);
            assert_eq!(
                format!("{:?}", compact.get(reference)),
                format!("{:?}", legacy.get(reference))
            );
            if step % 127 == 0 {
                for r in 0..=compact.len() as ObjRef + 1 {
                    assert_eq!(
                        format!("{:?}", compact.get(r)),
                        format!("{:?}", legacy.get(r))
                    );
                }
            }
        }
        // With no remembered-set entries, derived Heap Debug remains exactly
        // equal too, despite the different private vacant-slot representation.
        compact.promote();
        legacy.promote();
        assert_eq!(format!("{compact:?}"), format!("{legacy:?}"));
        assert_eq!(format!("{compact:#?}"), format!("{legacy:#?}"));
    }

    #[test]
    fn budget_is_enforced_with_typed_oom() {
        let mut h = Heap::new(1000);
        // string de 600 chars → contabiliza ~1232 > 1000 → OOM
        let big = "x".repeat(600);
        let err = h.alloc_string(big).unwrap_err();
        assert!(err.requested > 1000);
        assert_eq!(err.budget, 1000);
    }

    #[test]
    fn peak_is_high_water_mark_of_used() {
        let mut h = Heap::new(4096);
        assert_eq!(h.peak(), 0);
        h.alloc_string("abc".into()).unwrap();
        let after_first = h.used();
        assert_eq!(h.peak(), after_first);
        h.alloc_instance("C".into(), vec![]).unwrap();
        // pico cresce com cada alocação e nunca retrocede
        assert!(h.peak() > after_first);
        assert_eq!(h.peak(), h.used());
        // OOM negado não altera o pico
        let big = "x".repeat(3000);
        let _ = h.alloc_string(big);
        assert_eq!(h.peak(), h.used());
    }

    #[test]
    fn objects_are_indexed_from_one() {
        let mut h = Heap::new(1024);
        let r = h.alloc_string("abc".into()).unwrap();
        assert_eq!(r, 1);
        assert_eq!(h.as_str(r), Ok("abc"));
        assert!(h.as_str(2).is_err());
        assert!(h.as_str(0).is_err());
    }

    #[test]
    fn primitive_array_runtime_descriptors_are_preserved() {
        let mut heap = Heap::new(4096);
        for (descriptor, elem) in [
            ("[I", ElemKind::Int),
            ("[C", ElemKind::Int),
            ("[Z", ElemKind::Int),
            ("[B", ElemKind::Int),
            ("[S", ElemKind::Int),
        ] {
            let array = heap.alloc_array(elem, 1, descriptor).unwrap();
            assert_eq!(heap.class_of(array), Ok(descriptor));
        }
    }

    #[test]
    fn fields_roundtrip() {
        let mut h = Heap::new(1024);
        let r = h
            .alloc_instance("LCaso;".into(), vec![("x".into(), Value::Int(7))])
            .unwrap();
        assert_eq!(h.get_field(r, "x"), Ok(Value::Int(7)));
        h.put_field(r, "y", Value::Null).unwrap();
        assert_eq!(h.get_field(r, "y"), Ok(Value::Null));
        assert_eq!(h.class_of(r), Ok("LCaso;"));
    }

    #[test]
    fn array_elem_kinds() {
        assert_eq!(ElemKind::from_array_desc("[I"), Some(ElemKind::Int));
        assert_eq!(ElemKind::from_array_desc("[J"), Some(ElemKind::Long));
        assert_eq!(
            ElemKind::from_array_desc("[Ljava/lang/String;"),
            Some(ElemKind::Obj)
        );
        assert_eq!(ElemKind::from_array_desc("I"), None);
    }
}
