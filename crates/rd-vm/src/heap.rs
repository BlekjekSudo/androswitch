//! Heap arena da VM com os orçamentos do piso moto-e5 (spec A2/PRF-06):
//! heap do app ≤ 256 MB. Alocação que exceder o orçamento responde erro
//! tipado `VM_OOM` — nunca abort (Lei 1). Objetos inalcançáveis são
//! coletados pelo mark-sweep de `crate::gc` em safe points (slots liberados
//! viram tombstones e são reciclados pela free list); o resto é liberado de
//! uma vez quando o `Engine` morre.

use crate::maps::HashSet;
use crate::value::Value;
use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};

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
    objects: Vec<HeapObj>,
    /// Slots freed by the GC, reused LIFO by `alloc` (see `gc.rs`).
    free: Vec<ObjRef>,
    young: Vec<bool>,
    remembered: HashSet<ObjRef>,
    // Some = host scope (pin allocations); None = guest execution.
    #[cfg(feature = "std")]
    gc_scopes: Vec<Option<Vec<ObjRef>>>,
    births: Vec<u64>,
    allocation_clock: u64,
    #[cfg(feature = "std")]
    existing_cutoffs: Vec<(usize, u64)>,
    #[cfg(not(feature = "std"))]
    execution_identity: crate::portable::execution::Identity,
    #[cfg(not(feature = "std"))]
    owned_scopes: crate::portable::execution::OwnerScopes,
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
            #[cfg(feature = "std")]
            gc_scopes: Vec::new(),
            births: Vec::new(),
            allocation_clock: 0,
            #[cfg(feature = "std")]
            existing_cutoffs: Vec::new(),
            #[cfg(not(feature = "std"))]
            execution_identity: Default::default(),
            #[cfg(not(feature = "std"))]
            owned_scopes: Default::default(),
        }
    }

    #[cfg(feature = "std")]
    pub(crate) fn is_young(&self, r: ObjRef) -> bool {
        self.young
            .get(r.wrapping_sub(1) as usize)
            .copied()
            .unwrap_or(true)
    }
    #[cfg(feature = "std")]
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
    #[cfg(not(feature = "std"))]
    pub(crate) fn bind_execution_identity(
        &mut self,
        provider: alloc::rc::Rc<dyn crate::native_host::NativeHost>,
    ) {
        self.execution_identity.bind(provider);
    }
    #[cfg(not(feature = "std"))]
    pub(crate) fn execution_owner(&self) -> u64 {
        self.execution_identity.owner()
    }
    #[cfg(not(feature = "std"))]
    pub(crate) fn checked_execution_owner(&self) -> Result<u64, crate::VmExit> {
        self.execution_identity.checked_owner()
    }
    #[cfg(feature = "std")]
    pub(crate) fn gc_scope_depth(&self) -> usize {
        self.gc_scopes.len()
    }
    #[cfg(not(feature = "std"))]
    pub(crate) fn gc_scope_depth(&self) -> crate::portable::execution::ScopeMark {
        self.owned_scopes.mark(self.execution_owner())
    }
    pub(crate) fn gc_host_enter(&mut self, existing: bool) {
        #[cfg(feature = "std")]
        {
            if existing {
                self.existing_cutoffs
                    .push((self.gc_scopes.len(), self.allocation_clock));
            }
            self.gc_scopes.push(Some(Vec::new()));
        }
        #[cfg(not(feature = "std"))]
        self.owned_scopes.host_enter(
            self.execution_owner(),
            existing.then_some(self.allocation_clock),
        );
    }
    /// A nested Rust host call can return a temporary to its enclosing host
    /// callback. Preserve all nested pins conservatively until that callback
    /// returns, but never promote them through an intervening Java frame.
    #[cfg(feature = "std")]
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
        #[cfg(feature = "std")]
        self.gc_scopes.push(None);
        #[cfg(not(feature = "std"))]
        self.owned_scopes.guest_enter(self.execution_owner());
    }
    #[cfg(feature = "std")]
    pub(crate) fn gc_scope_restore(&mut self, depth: usize) {
        self.gc_scopes.truncate(depth);
        self.existing_cutoffs.retain(|(d, _)| *d < depth);
    }
    #[cfg(all(not(feature = "std"), any(test, feature = "portable-test-support")))]
    pub(crate) fn gc_scope_restore(&mut self, mark: crate::portable::execution::ScopeMark) {
        self.owned_scopes.restore(mark);
    }
    #[cfg(not(feature = "std"))]
    pub(crate) fn gc_scope_restore_return(
        &mut self,
        mark: crate::portable::execution::ScopeMark,
        object: Option<ObjRef>,
    ) {
        self.owned_scopes.restore_return(mark, object);
    }
    pub(crate) fn gc_pinned_roots(&self) -> impl Iterator<Item = ObjRef> + '_ {
        #[cfg(feature = "std")]
        let cutoff = self
            .existing_cutoffs
            .iter()
            .map(|(_, c)| *c)
            .max()
            .unwrap_or(0);
        #[cfg(not(feature = "std"))]
        let cutoff = self.owned_scopes.cutoff();
        #[cfg(feature = "std")]
        let pins = self
            .gc_scopes
            .iter()
            .filter_map(Option::as_ref)
            .flatten()
            .copied();
        #[cfg(not(feature = "std"))]
        let pins = self.owned_scopes.roots();
        pins.chain(
            self.births.iter().enumerate().filter(move |(_, birth)| **birth <= cutoff)
                .map(|(i, _)| (i + 1) as ObjRef).filter(|r|
                    !matches!(self.get(*r), Ok(HeapObj::Instance { class, .. }) if class == crate::gc::FREED_CLASS)
                )
        )
    }
    #[cfg(feature = "std")]
    pub(crate) fn gc_pin_return(&mut self, r: ObjRef) {
        self.pin_allocation(r);
    }

    pub(crate) fn pin_allocation(&mut self, r: ObjRef) {
        #[cfg(feature = "std")]
        if let Some(Some(roots)) = self.gc_scopes.last_mut() {
            roots.push(r);
        }
        #[cfg(not(feature = "std"))]
        self.owned_scopes.pin(self.execution_owner(), r);
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
            self.objects[(r - 1) as usize] = obj;
            self.young[(r - 1) as usize] = true;
            self.allocation_clock += 1;
            self.births[(r - 1) as usize] = self.allocation_clock;
            self.pin_allocation(r);
            return Ok(r);
        }
        self.objects.push(obj);
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
        let old_bytes = match &self.objects[index] {
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
        self.objects[index] = HeapObj::Str(s);
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
            .saturating_mul(core::mem::size_of::<Value>())
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
        let bytes = match slot {
            HeapObj::Instance { class, .. } if class == crate::gc::FREED_CLASS => return None,
            HeapObj::Instance { fields, .. } => {
                fields
                    .iter()
                    .map(|(n, _)| core::mem::size_of::<Value>() + n.len())
                    .sum::<usize>()
                    .max(fields.len() * 24)
                    + 32
            }
            HeapObj::Str(s) => s.len().saturating_mul(2).saturating_add(32),
            HeapObj::Array { elems, .. } => elems
                .len()
                .saturating_mul(core::mem::size_of::<Value>())
                .saturating_add(16),
        };
        *slot = HeapObj::Instance {
            class: crate::gc::FREED_CLASS.to_string(),
            fields: Vec::new(),
        };
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
        Ok(&self.objects[(r - 1) as usize])
    }

    pub fn get_mut(&mut self, r: ObjRef) -> Result<&mut HeapObj, String> {
        if r == 0 || r as usize > self.objects.len() {
            return Err(format!("referência inválida #{r}"));
        }
        if !self.young[(r - 1) as usize] {
            self.remembered.insert(r);
        }
        Ok(&mut self.objects[(r - 1) as usize])
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
            let cost = core::mem::size_of::<Value>() + name.len();
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
    use alloc::vec;

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
