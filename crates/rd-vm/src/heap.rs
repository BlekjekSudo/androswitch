//! Heap arena da VM com os orçamentos do piso moto-e5 (spec A2/PRF-06):
//! heap do app ≤ 256 MB. Alocação que exceder o orçamento responde erro
//! tipado `VM_OOM` — nunca abort (Lei 1). Semântica de arena: tudo é
//! liberado de uma vez quando o `Engine` morre (M2 não tem GC por
//! gerações; arena-por-execução é o "GC" contratual desta fase).

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
    /// para o orçamento `ram_app_heap_mb` (a arena não libera objetos, então
    /// na prática used == peak, mas o contrato do gate é o pico).
    peak: usize,
    objects: Vec<HeapObj>,
}

impl Heap {
    pub fn new(budget: usize) -> Self {
        Heap {
            budget,
            used: 0,
            peak: 0,
            objects: Vec::new(),
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
        if self.objects.len() >= MAX_OBJECTS {
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
        self.objects.push(obj);
        Ok(self.objects.len() as ObjRef)
    }

    pub fn alloc_string(&mut self, s: String) -> Result<ObjRef, OomError> {
        let bytes = s.len().saturating_mul(2).saturating_add(32);
        self.alloc(bytes, HeapObj::Str(s))
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
        self.alloc(
            bytes,
            HeapObj::Array {
                elem,
                elems: Vec::new(),
                // issue #37: arrays de objetos guardam o tipo do elemento
                // (check-cast [LX; e ArrayStoreException precisam dele)
                elem_class: elem_class.to_string(),
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
            HeapObj::Array {
                elem, elem_class, ..
            } => match elem {
                ElemKind::Int => Ok("[I"),
                ElemKind::Long => Ok("[J"),
                ElemKind::Float => Ok("[F"),
                ElemKind::Double => Ok("[D"),
                // issue #37: o descritor real do array de objetos ([LX;) —
                // nunca "[Ljava/lang/Object;" genérico
                ElemKind::Obj => Ok(elem_class),
            },
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
