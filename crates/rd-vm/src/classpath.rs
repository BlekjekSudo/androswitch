//! Classpath da VM: resolução de classes/métodos/campos/protos sobre um ou
//! mais DEX (APK real = classes.dex/classes2.dex/…). Cache de `CodeItem`
//! parseado por offset (loops invocam o mesmo método repetidamente).

use std::collections::HashMap;
use std::sync::Arc;

use rd_dex::classes::{ClassData, ClassDef};
use rd_dex::code::CodeItem;
use rd_dex::{Dex, RdError, RdResult};

/// Descritor de método em formato Java (`(II)I`, `(Ljava/lang/String;)V`).
pub fn proto_descriptor(dex: &Dex, proto_idx: u16) -> RdResult<String> {
    let proto = dex.proto(proto_idx)?;
    let params = dex.proto_params(proto_idx)?;
    let mut out = String::from("(");
    for p in params {
        out.push_str(dex.type_str(p));
    }
    out.push(')');
    out.push_str(dex.type_str(proto.return_type_idx));
    Ok(out)
}

/// Uma entrada de method_id resolvida.
#[derive(Debug, Clone)]
pub struct MethodRef {
    /// descritor da classe declarante (`Lpkg/Cls;`)
    pub class: String,
    pub name: String,
    pub proto: String,
    /// descritores dos parâmetros em ordem (para largura de registradores)
    pub param_types: Vec<String>,
    pub return_type: String,
}

/// method_id (dex, idx) -> resolved method reference (ADR 008 Stage 1b).
type MethodRefCache = std::cell::RefCell<HashMap<(usize, u32), Arc<MethodRef>>>;
/// field_id (dex, idx) -> (class, name, type) reference.
type FieldRefCache = std::cell::RefCell<HashMap<(usize, u32), Arc<(String, String, String)>>>;
/// (class, name, proto) -> virtual resolution result.
type ResolveCache =
    HashMap<(String, String, String), Option<(usize, ClassDef, rd_dex::methods::EncodedMethod)>>;

#[derive(Debug, Default)]
pub struct Classpath {
    pub dexes: Vec<Dex>,
    /// (índice do dex, offset do code_item) → parse em cache
    code_cache: HashMap<(usize, usize), Arc<CodeItem>>,
    /// (índice do dex, class_idx) → class_data em cache
    class_data_cache: HashMap<(usize, u32), Option<Arc<ClassData>>>,
    /// ADR 008 Stage 4 (hot-path subset): method_id → resolved ref cache.
    /// `method_ref` used to rebuild the proto descriptor + parameter type
    /// strings on *every* invoke; the cache makes repeat invokes O(1).
    method_ref_cache: MethodRefCache,
    field_ref_cache: FieldRefCache,
    /// (class, name, proto) → virtual resolution result (dexes are immutable
    /// for the lifetime of an Engine, so this can never go stale).
    resolve_cache: ResolveCache,
    /// descriptor -> (dex, class_def position) memo for `find_class`
    /// (ADR 008 Stage 1b). NOTE: unlike subtype results (which a test
    /// deliberately mutates DEX metadata to re-check), class *identity*
    /// lookup is only memoized as an index pair and re-borrowed from the
    /// live DEX each call, so in-place def edits stay visible.
    find_class_cache: std::cell::RefCell<HashMap<String, Option<(usize, usize)>>>,
}

impl Classpath {
    pub fn new(dexes: Vec<Dex>) -> Self {
        Classpath {
            dexes,
            code_cache: HashMap::new(),
            class_data_cache: HashMap::new(),
            method_ref_cache: std::cell::RefCell::new(HashMap::new()),
            field_ref_cache: std::cell::RefCell::new(HashMap::new()),
            resolve_cache: HashMap::new(),
            find_class_cache: std::cell::RefCell::new(HashMap::new()),
        }
    }

    /// method_id cru (class/name/idx) — cached (ADR 008 Stage 4 subset).
    pub fn method_ref(&self, dex_idx: usize, idx: u32) -> RdResult<Arc<MethodRef>> {
        if let Some(cached) = self.method_ref_cache.borrow().get(&(dex_idx, idx)) {
            return Ok(cached.clone());
        }
        let dex = &self.dexes[dex_idx];
        let m = dex.method(idx)?;
        let proto = proto_descriptor(dex, m.proto_idx)?;
        let params = dex.proto_params(m.proto_idx)?;
        let param_types = params
            .iter()
            .map(|p| dex.type_str(*p).to_string())
            .collect();
        let ret = dex
            .type_str(dex.proto(m.proto_idx)?.return_type_idx)
            .to_string();
        let out = Arc::new(MethodRef {
            class: dex.type_str(m.class_idx as u32).to_string(),
            name: dex.string(m.name_idx).to_string(),
            proto,
            param_types,
            return_type: ret,
        });
        self.method_ref_cache
            .borrow_mut()
            .insert((dex_idx, idx), out.clone());
        Ok(out)
    }

    /// field_id cru → (classe, nome, tipo).
    pub fn field_ref(&self, dex_idx: usize, idx: u32) -> RdResult<(String, String, String)> {
        if let Some(cached) = self.field_ref_cache.borrow().get(&(dex_idx, idx)) {
            return Ok((**cached).clone());
        }
        let dex = &self.dexes[dex_idx];
        let f = dex.field(idx)?;
        let out = (
            dex.type_str(f.class_idx as u32).to_string(),
            dex.string(f.name_idx).to_string(),
            dex.type_str(f.type_idx as u32).to_string(),
        );
        self.field_ref_cache
            .borrow_mut()
            .insert((dex_idx, idx), Arc::new(out.clone()));
        Ok(out)
    }

    /// string id (com fallback vazio tolerante — Lei 1).
    pub fn string(&self, dex_idx: usize, idx: u32) -> String {
        self.dexes[dex_idx].string(idx).to_string()
    }

    pub fn type_str(&self, dex_idx: usize, idx: u32) -> String {
        self.dexes[dex_idx].type_str(idx).to_string()
    }

    /// Procura a classe por descritor em todos os dex → (dex_idx, def).
    pub fn find_class(&self, descriptor: &str) -> Option<(usize, &ClassDef)> {
        if let Some(hit) = self.find_class_cache.borrow().get(descriptor) {
            return hit.map(|(di, ci)| (di, &self.dexes[di].class_defs[ci]));
        }
        let mut found = None;
        for (i, dex) in self.dexes.iter().enumerate() {
            if let Some(def) = dex.find_class(descriptor) {
                if is_shadowed_platform_stub(descriptor, def.access_flags) {
                    found = None;
                    break;
                }
                let ci = dex
                    .class_defs
                    .iter()
                    .position(|d| std::ptr::eq(d, def))
                    .unwrap_or(usize::MAX);
                found = if ci == usize::MAX {
                    return Some((i, def));
                } else {
                    Some((i, ci))
                };
                break;
            }
        }
        self.find_class_cache
            .borrow_mut()
            .insert(descriptor.to_string(), found);
        found.map(|(di, ci)| (di, &self.dexes[di].class_defs[ci]))
    }

    /// class_data em cache.
    pub fn class_data(
        &mut self,
        dex_idx: usize,
        def: &ClassDef,
    ) -> RdResult<Option<Arc<ClassData>>> {
        let key = (dex_idx, def.class_idx);
        if let Some(cached) = self.class_data_cache.get(&key) {
            return Ok(cached.clone());
        }
        let parsed = self.dexes[dex_idx].class_data(def)?;
        let arc = parsed.map(Arc::new);
        self.class_data_cache.insert(key, arc.clone());
        Ok(arc)
    }

    /// code_item em cache.
    pub fn code(&mut self, dex_idx: usize, code_off: u32) -> RdResult<Option<Arc<CodeItem>>> {
        if code_off == 0 {
            return Ok(None);
        }
        let key = (dex_idx, code_off as usize);
        if let Some(cached) = self.code_cache.get(&key) {
            return Ok(Some(cached.clone()));
        }
        let parsed = self.dexes[dex_idx].code(code_off)?;
        match parsed {
            Some(ci) => {
                let arc = Arc::new(ci);
                self.code_cache.insert(key, arc.clone());
                Ok(Some(arc))
            }
            None => Ok(None),
        }
    }

    /// issue #39: resolve um método APENAS na própria classe (sem caminhar a
    /// cadeia de superclasses, sem interfaces). Uso correto: `<clinit>`
    /// (JLS 12.4.2 — o init da super já acontece via ensure_initialized(super);
    /// usar resolve_method aqui fazia o `<clinit>` do ancestral rodar DUAS vezes).
    pub fn find_own_method(
        &mut self,
        class_desc: &str,
        name: &str,
        proto: &str,
    ) -> Option<(usize, ClassDef, rd_dex::methods::EncodedMethod)> {
        let (dex_idx, def) = self.find_class(class_desc)?;
        let def = *def;
        let data = self.class_data(dex_idx, &def).ok().flatten()?;
        data.direct_methods
            .iter()
            .chain(&data.virtual_methods)
            .find(|m| {
                m.method_idx != rd_dex::strings::NO_INDEX
                    && self.method_name(dex_idx, m.method_idx) == Some(name)
                    && self.method_proto(dex_idx, m.method_idx).as_deref() == Some(proto)
            })
            .cloned()
            .map(|m| (dex_idx, def, m))
    }

    /// Methods declared by a DEX class itself, as `(name, proto, access_flags)`.
    /// Used by reflection enumeration (`getDeclaredMethods`, `getConstructors`, …).
    pub fn declared_methods(&mut self, class_desc: &str) -> Vec<(String, String, u32)> {
        let Some((dex_idx, def)) = self.find_class(class_desc) else {
            return Vec::new();
        };
        let def = *def;
        let Some(data) = self.class_data(dex_idx, &def).ok().flatten() else {
            return Vec::new();
        };
        data.direct_methods
            .iter()
            .chain(&data.virtual_methods)
            .filter(|m| m.method_idx != rd_dex::strings::NO_INDEX)
            .filter_map(|m| {
                Some((
                    self.method_name(dex_idx, m.method_idx)?.to_string(),
                    self.method_proto(dex_idx, m.method_idx)?,
                    m.access_flags,
                ))
            })
            .collect()
    }

    /// Resolve um método por (classe, nome, proto) caminhando a superclasse.
    /// issue #26: depois da cadeia de supers, procura na closure de interfaces
    /// (métodos default — Java 8+/Kotlin massivo); retorna
    /// (dex_idx, class_def_idx, método, classe_que_declara).
    pub fn resolve_method(
        &mut self,
        class_desc: &str,
        name: &str,
        proto: &str,
    ) -> Option<(usize, ClassDef, rd_dex::methods::EncodedMethod)> {
        let key = (class_desc.to_string(), name.to_string(), proto.to_string());
        if let Some(cached) = self.resolve_cache.get(&key) {
            return cached.clone();
        }
        let out = self.resolve_method_uncached(class_desc, name, proto);
        self.resolve_cache.insert(key, out.clone());
        out
    }

    fn resolve_method_uncached(
        &mut self,
        class_desc: &str,
        name: &str,
        proto: &str,
    ) -> Option<(usize, ClassDef, rd_dex::methods::EncodedMethod)> {
        // issue #42 (JVMS 5.4.5): a cadeia de CLASSES vence sobre default
        // methods de interface — subir a hierarquia inteira ANTES de olhar
        // interfaces (antes: interfaces do nível atual sombreavam o método
        // herdado da superclasse)
        let mut cur = class_desc.to_string();
        let mut chain: Vec<String> = Vec::new();
        for _ in 0..16 {
            let Some((dex_idx, def)) = self.find_class(&cur) else {
                break; // fim da cadeia (ex.: Object é builtin, fora do DEX)
            };
            let def = *def;
            if let Some(data) = self.class_data(dex_idx, &def).ok().flatten() {
                let found = data
                    .direct_methods
                    .iter()
                    .chain(&data.virtual_methods)
                    .find(|m| {
                        m.method_idx != rd_dex::strings::NO_INDEX
                            && self.method_name(dex_idx, m.method_idx) == Some(name)
                            && self.method_proto(dex_idx, m.method_idx).as_deref() == Some(proto)
                    })
                    .cloned();
                if let Some(m) = found {
                    return Some((dex_idx, def, m));
                }
            }
            chain.push(cur.clone());
            // sobe para a superclasse (dentro do mesmo conjunto de dex)
            let super_desc = self.dexes[dex_idx].type_str(def.superclass_idx).to_string();
            if super_desc.is_empty() || super_desc == cur {
                break;
            }
            cur = super_desc;
        }
        // fase 2: default methods de interface — da classe mais derivada
        // para a raiz (mesma ordem de visita da fase 1; issue #26)
        for c in &chain {
            let ifaces = self.interfaces_of(c);
            for iface in ifaces {
                if let Some(hit) = self.resolve_in_interface(&iface, name, proto, 0) {
                    return Some(hit);
                }
            }
        }
        None
    }

    /// (name, type) de um field_id (helper de `resolve_static_field`).
    fn field_info(&self, dex_idx: usize, idx: u32) -> Option<(String, String, String)> {
        let d = self.dexes.get(dex_idx)?;
        let f = d.field(idx).ok()?;
        Some((
            d.type_str(f.class_idx as u32).to_string(),
            d.string(f.name_idx).to_string(),
            d.type_str(f.type_idx as u32).to_string(),
        ))
    }

    /// Declared field metadata for reflection: owner, name, type descriptor,
    /// access flags, and whether storage is static.
    pub fn declared_fields_info(
        &mut self,
        class_desc: &str,
    ) -> Vec<(String, String, String, u32, bool)> {
        let Some((dex_idx, class_def)) = self.find_class(class_desc) else {
            return Vec::new();
        };
        let class_def = *class_def;
        let Ok(Some(data)) = self.class_data(dex_idx, &class_def) else {
            return Vec::new();
        };
        let mut fields = Vec::with_capacity(data.total_fields());
        for (is_static, group) in [
            (true, data.static_fields.as_slice()),
            (false, data.instance_fields.as_slice()),
        ] {
            for field in group {
                if let Some((owner, name, ty)) = self.field_info(dex_idx, field.field_idx) {
                    fields.push((owner, name, ty, field.access_flags, is_static));
                }
            }
        }
        fields
    }

    /// issue #57 (JVMS 5.4.3.2): classe DECLARANTE de um campo — caminha a
    /// cadeia de superclasses e, depois, a closure de interfaces (campos de
    /// interface são implicitamente public static final). `None` = não existe
    /// em lugar nenhum (o caller materializa NoSuchFieldError — antes o miss
    /// caía em default silencioso e sput criava slot divergente por ref).
    pub fn resolve_static_field(&mut self, class_desc: &str, name: &str) -> Option<String> {
        let mut cur = class_desc.to_string();
        for _ in 0..16 {
            let Some((dex_idx, def)) = self.find_class(&cur) else {
                break; // builtin (fora do DEX) declara nela mesma — caller decide
            };
            let def = *def;
            if let Some(data) = self.class_data(dex_idx, &def).ok().flatten() {
                for f in &data.static_fields {
                    if let Some((_, fname, _)) = self.field_info(dex_idx, f.field_idx) {
                        if fname == name {
                            return Some(cur.clone());
                        }
                    }
                }
            }
            let super_desc = self.dexes[dex_idx].type_str(def.superclass_idx).to_string();
            if super_desc.is_empty() || super_desc == cur {
                break;
            }
            cur = super_desc;
        }
        // interfaces: mesma ordem de visita do resolve_method (da classe mais
        // derivada para a raiz — fase 2 do JVMS 5.4.3.2)
        let mut queue: Vec<String> = self.interfaces_of(class_desc);
        let mut visited: std::collections::HashSet<String> = queue.iter().cloned().collect();
        let mut budget = 64;
        while let Some(iface) = queue.pop() {
            budget -= 1;
            if budget == 0 {
                return None;
            }
            if let Some((dex_idx, def)) = self.find_class(&iface) {
                let def = *def;
                if let Some(data) = self.class_data(dex_idx, &def).ok().flatten() {
                    for f in &data.static_fields {
                        if let Some((_, fname, _)) = self.field_info(dex_idx, f.field_idx) {
                            if fname == name {
                                return Some(iface.clone());
                            }
                        }
                    }
                }
                for next in self.interfaces_of(&iface) {
                    if visited.insert(next.clone()) {
                        queue.push(next);
                    }
                }
            }
        }
        None
    }

    /// Busca um método em uma interface e nas suas superinterfaces (recursão
    /// com guard de ciclo/cap — issue #26, métodos default incluídos).
    fn resolve_in_interface(
        &mut self,
        iface: &str,
        name: &str,
        proto: &str,
        depth: usize,
    ) -> Option<(usize, ClassDef, rd_dex::methods::EncodedMethod)> {
        if depth > 16 {
            return None;
        }
        let (dex_idx, def) = self.find_class(iface)?;
        let def = *def;
        if let Some(data) = self.class_data(dex_idx, &def).ok().flatten() {
            let found = data
                .direct_methods
                .iter()
                .chain(&data.virtual_methods)
                .find(|m| {
                    m.method_idx != rd_dex::strings::NO_INDEX
                        && self.method_name(dex_idx, m.method_idx) == Some(name)
                        && self.method_proto(dex_idx, m.method_idx).as_deref() == Some(proto)
                })
                .cloned();
            if let Some(m) = found {
                return Some((dex_idx, def, m));
            }
        }
        for super_iface in self.interfaces_of(iface) {
            if let Some(hit) = self.resolve_in_interface(&super_iface, name, proto, depth + 1) {
                return Some(hit);
            }
        }
        None
    }

    fn method_name(&self, dex_idx: usize, idx: u32) -> Option<&str> {
        let d = self.dexes.get(dex_idx)?;
        let m = d.method(idx).ok()?;
        Some(d.string(m.name_idx))
    }

    fn method_proto(&self, dex_idx: usize, idx: u32) -> Option<String> {
        let d = self.dexes.get(dex_idx)?;
        let m = d.method(idx).ok()?;
        proto_descriptor(d, m.proto_idx).ok()
    }

    /// Superclasse de um descritor de classe ("" se nenhuma/raiz).
    pub fn superclass_of(&self, class_desc: &str) -> Option<String> {
        for dex in &self.dexes {
            if let Some(def) = dex.find_class(class_desc) {
                let s = dex.type_str(def.superclass_idx);
                if s.is_empty() || s == "?" {
                    return None;
                }
                return Some(s.to_string());
            }
        }
        None
    }

    /// Interfaces declaradas por uma classe/interface (class_def.interfaces_off).
    /// Vazio para classes de plataforma fora do DEX (String, etc. — cobertas
    /// por `builtin_interfaces`).
    pub fn interfaces_of(&self, class_desc: &str) -> Vec<String> {
        for dex in &self.dexes {
            if let Some(def) = dex.find_class(class_desc) {
                return dex
                    .interfaces(def)
                    .unwrap_or_default()
                    .iter()
                    .map(|t| dex.type_str(*t).to_string())
                    .collect();
            }
        }
        Vec::new()
    }

    /// Closure de interfaces: `sup` é interface de `start` (direta ou via
    /// superinterfaces, com guard de ciclo e cap de profundidade) — issue #26.
    fn interface_closure_contains(&self, mut queue: Vec<String>, sup: &str) -> bool {
        let mut visited: std::collections::HashSet<String> = queue.iter().cloned().collect();
        let mut budget = 64;
        while let Some(cur) = queue.pop() {
            if cur == sup {
                return true;
            }
            // host (JDK/platform) interfaces: builtin super-interface closure
            if builtin_interfaces(&cur).is_some_and(|v| v.contains(&sup)) {
                return true;
            }
            budget -= 1;
            if budget == 0 {
                return false;
            }
            for next in self.interfaces_of(&cur) {
                if visited.insert(next.clone()) {
                    queue.push(next);
                }
            }
        }
        false
    }

    /// `class A <: B` caminhando supers (usado por instanceof/catch/check-cast).
    /// String é subtipo de CharSequence/Object/String hierarquia mínima embutida.
    /// issue #26: além da cadeia de superclasses, fecha transitivamente sobre
    /// as interfaces declaradas de cada classe da cadeia (com memo? não —
    /// closure BFS por chamada, cap de orçamento). Interfaces de plataforma
    /// (CharSequence/Comparable/…) vêm da tabela builtin.
    pub fn is_subtype(&self, sub: &str, sup: &str) -> bool {
        if sub == sup {
            return true;
        }
        // JLS 4.10: every reference type (incl. unmodelled host classes such
        // as android.content.res.Configuration) is a subtype of Object
        if sup == "Ljava/lang/Object;" && (sub.starts_with('L') || sub.starts_with('[')) {
            return true;
        }
        // issue #42: subtipagem de ARRAYS (JLS 4.10.2/4.10.3) — antes arrays
        // só casavam textualmente (ArrayStoreException espúria em código
        // covariante idiomático, check-cast/instanceof falhos):
        //   · todo array <: Object, Cloneable, Serializable
        //   · [LSub; <: [LSup; ⇔ Sub <: Sup (recursão no elemento)
        //   · [X <: [LObject; ⇔ X é tipo de referência (primitivos invariantes)
        if let Some(sub_elem) = sub.strip_prefix('[') {
            if matches!(
                sup,
                "Ljava/lang/Object;" | "Ljava/lang/Cloneable;" | "Ljava/io/Serializable;"
            ) {
                return true;
            }
            if let Some(sup_elem) = sup.strip_prefix('[') {
                if sup_elem == "Ljava/lang/Object;" {
                    return !matches!(sub_elem, "B" | "C" | "D" | "F" | "I" | "J" | "S" | "Z");
                }
                return self.is_subtype(sub_elem, sup_elem);
            }
            return false; // array vs classe não-array (além de Object/…)
        }
        // hierarquia mínima da plataforma conhecida pela VM (classes que não
        // estão no DEX mas fazem parte do contrato do interpretador M2)
        if let Some(supers) = builtin_hierarchy(sub) {
            if supers.contains(&sup) {
                return true;
            }
        }
        if let Some(ifaces) = builtin_interfaces(sub) {
            if ifaces.contains(&sup) {
                return true;
            }
        }
        // Class metadata stays immutably borrowed for this call. Resolve each
        // level once for both its interfaces and superclass; do not allocate
        // a fresh descriptor String or cache across mutable classpath calls.
        let mut cur = sub;
        for _ in 0..16 {
            if cur == sup {
                return true;
            }
            let Some((dex, def)) = self
                .dexes
                .iter()
                .find_map(|dex| dex.find_class(cur).map(|def| (dex, def)))
            else {
                return false;
            };
            let interfaces = dex
                .interfaces(def)
                .unwrap_or_default()
                .iter()
                .map(|t| dex.type_str(*t).to_string())
                .collect();
            if self.interface_closure_contains(interfaces, sup) {
                return true;
            }
            let next = dex.type_str(def.superclass_idx);
            if next.is_empty() || next == "?" {
                return false;
            }
            if next == sup {
                return true;
            }
            if let Some(supers) = builtin_hierarchy(next) {
                if supers.contains(&sup) {
                    return true;
                }
            }
            if let Some(ifaces) = builtin_interfaces(next) {
                if ifaces.contains(&sup) {
                    return true;
                }
            }
            cur = next;
        }
        false
    }

    /// Offset do call_site_item N (para invokedynamic/string concat).
    pub fn call_site_offset(&self, dex_idx: usize, n: usize) -> Option<u32> {
        self.dexes[dex_idx].call_site_offsets.get(n).copied()
    }
}

/// Hierarquia embutida mínima para classes de plataforma que a VM conhece sem
/// DEX (catch/instanceof/ hierarquia de exceções; M3.2: hierarquia Android do
/// framework host — APKs REAIS não definem android.* no DEX, e o LayoutInflater
/// precisa de is_subtype(View/Activity/…) para attach/dispatch corretos).
pub(crate) fn builtin_hierarchy(class: &str) -> Option<&'static [&'static str]> {
    const EXC: &[&str] = &[
        "Ljava/lang/Exception;",
        "Ljava/lang/Throwable;",
        "Ljava/lang/Object;",
    ];
    const ERR: &[&str] = &[
        "Ljava/lang/Error;",
        "Ljava/lang/Throwable;",
        "Ljava/lang/Object;",
    ];
    const RUNTIME: &[&str] = &[
        "Ljava/lang/RuntimeException;",
        "Ljava/lang/Exception;",
        "Ljava/lang/Throwable;",
        "Ljava/lang/Object;",
    ];
    // M3.2 — cadeia do framework host (ancestrais completos em cada entrada;
    // simplificação documentada: Activity estende ContextThemeWrapper→…→Context)
    const OBJECT: &[&str] = &["Ljava/lang/Object;"];
    // (bug M8: era "Ljava/content/Context;" — typo sem cobertura de teste até o
    // app real exigir check-cast Activity→android.content.Context)
    const CONTEXT: &[&str] = &["Landroid/content/Context;", "Ljava/lang/Object;"];
    const VIEW: &[&str] = &["Landroid/view/View;", "Ljava/lang/Object;"];
    const VIEWGROUP: &[&str] = &[
        "Landroid/view/ViewGroup;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const VIEWGROUP_LAYOUT_PARAMS: &[&str] = &[
        "Landroid/view/ViewGroup$LayoutParams;",
        "Ljava/lang/Object;",
    ];
    const MARGIN_LAYOUT_PARAMS: &[&str] = &[
        "Landroid/view/ViewGroup$MarginLayoutParams;",
        "Landroid/view/ViewGroup$LayoutParams;",
        "Ljava/lang/Object;",
    ];
    const LINEAR_LAYOUT_PARAMS: &[&str] = &[
        "Landroid/widget/LinearLayout$LayoutParams;",
        "Landroid/view/ViewGroup$MarginLayoutParams;",
        "Landroid/view/ViewGroup$LayoutParams;",
        "Ljava/lang/Object;",
    ];
    const FRAME_LAYOUT_PARAMS: &[&str] = &[
        "Landroid/widget/FrameLayout$LayoutParams;",
        "Landroid/view/ViewGroup$MarginLayoutParams;",
        "Landroid/view/ViewGroup$LayoutParams;",
        "Ljava/lang/Object;",
    ];
    const RELATIVE_LAYOUT_PARAMS: &[&str] = &[
        "Landroid/widget/RelativeLayout$LayoutParams;",
        "Landroid/view/ViewGroup$MarginLayoutParams;",
        "Landroid/view/ViewGroup$LayoutParams;",
        "Ljava/lang/Object;",
    ];
    const TEXTVIEW: &[&str] = &[
        "Landroid/widget/TextView;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const IMAGEVIEW: &[&str] = &[
        "Landroid/widget/ImageView;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const IMAGEBUTTON: &[&str] = &[
        "Landroid/widget/ImageButton;",
        "Landroid/widget/ImageView;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const PROGRESSBAR: &[&str] = &[
        "Landroid/widget/ProgressBar;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const BUTTON: &[&str] = &[
        "Landroid/widget/Button;",
        "Landroid/widget/TextView;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const COMPOUND_BUTTON: &[&str] = &[
        "Landroid/widget/CompoundButton;",
        "Landroid/widget/TextView;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const RADIO_BUTTON: &[&str] = &[
        "Landroid/widget/RadioButton;",
        "Landroid/widget/CompoundButton;",
        "Landroid/widget/TextView;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const LINEARLAYOUT: &[&str] = &[
        "Landroid/widget/LinearLayout;",
        "Landroid/view/ViewGroup;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const RADIO_GROUP: &[&str] = &[
        "Landroid/widget/RadioGroup;",
        "Landroid/widget/LinearLayout;",
        "Landroid/view/ViewGroup;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const FRAMELAYOUT: &[&str] = &[
        "Landroid/widget/FrameLayout;",
        "Landroid/view/ViewGroup;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const SCROLL_VIEW: &[&str] = &[
        "Landroid/widget/ScrollView;",
        "Landroid/widget/FrameLayout;",
        "Landroid/view/ViewGroup;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const HORIZONTAL_SCROLL_VIEW: &[&str] = &[
        "Landroid/widget/HorizontalScrollView;",
        "Landroid/widget/FrameLayout;",
        "Landroid/view/ViewGroup;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const RELATIVE_LAYOUT: &[&str] = &[
        "Landroid/widget/RelativeLayout;",
        "Landroid/view/ViewGroup;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const TABLE_LAYOUT: &[&str] = &[
        "Landroid/widget/TableLayout;",
        "Landroid/widget/LinearLayout;",
        "Landroid/view/ViewGroup;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const TABLE_ROW: &[&str] = &[
        "Landroid/widget/TableRow;",
        "Landroid/widget/LinearLayout;",
        "Landroid/view/ViewGroup;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const ADAPTER_VIEW: &[&str] = &[
        "Landroid/widget/AdapterView;",
        "Landroid/view/ViewGroup;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const ABS_LIST_VIEW: &[&str] = &[
        "Landroid/widget/AbsListView;",
        "Landroid/widget/AdapterView;",
        "Landroid/view/ViewGroup;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const LIST_VIEW: &[&str] = &[
        "Landroid/widget/ListView;",
        "Landroid/widget/AbsListView;",
        "Landroid/widget/AdapterView;",
        "Landroid/view/ViewGroup;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const GRID_VIEW: &[&str] = &[
        "Landroid/widget/GridView;",
        "Landroid/widget/AbsListView;",
        "Landroid/widget/AdapterView;",
        "Landroid/view/ViewGroup;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const SPINNER: &[&str] = &[
        "Landroid/widget/Spinner;",
        "Landroid/widget/AbsSpinner;",
        "Landroid/widget/AdapterView;",
        "Landroid/view/ViewGroup;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const ABS_SPINNER: &[&str] = &[
        "Landroid/widget/AbsSpinner;",
        "Landroid/widget/AdapterView;",
        "Landroid/view/ViewGroup;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const WEB_VIEW: &[&str] = &[
        "Landroid/webkit/WebView;",
        "Landroid/widget/AbsoluteLayout;",
        "Landroid/view/ViewGroup;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const ABSOLUTE_LAYOUT: &[&str] = &[
        "Landroid/widget/AbsoluteLayout;",
        "Landroid/view/ViewGroup;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    match class {
        "Ljava/lang/String;" => Some(&["Ljava/lang/Object;"]),
        "Ljava/lang/StringBuilder;" | "Ljava/lang/StringBuffer;" => Some(&["Ljava/lang/Object;"]),
        "Ljava/lang/ArithmeticException;" => Some(RUNTIME),
        "Ljava/lang/NullPointerException;" => Some(RUNTIME),
        "Ljava/lang/ClassCastException;" => Some(RUNTIME),
        "Ljava/lang/ArrayIndexOutOfBoundsException;"
        | "Ljava/lang/StringIndexOutOfBoundsException;" => Some(&[
            "Ljava/lang/IndexOutOfBoundsException;",
            "Ljava/lang/RuntimeException;",
            "Ljava/lang/Exception;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/lang/IndexOutOfBoundsException;"
        | "Ljava/util/NoSuchElementException;"
        | "Ljava/util/ConcurrentModificationException;"
        | "Ljava/util/EmptyStackException;"
        | "Ljava/lang/TypeNotPresentException;"
        | "Ljava/time/DateTimeException;"
        | "Ljava/io/UncheckedIOException;" => Some(RUNTIME),
        "Ljava/util/concurrent/CancellationException;" => Some(&[
            "Ljava/lang/IllegalStateException;",
            "Ljava/lang/RuntimeException;",
            "Ljava/lang/Exception;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/util/regex/PatternSyntaxException;"
        | "Ljava/util/IllegalFormatException;"
        | "Ljava/nio/charset/IllegalCharsetNameException;"
        | "Ljava/nio/charset/UnsupportedCharsetException;" => Some(&[
            "Ljava/lang/IllegalArgumentException;",
            "Ljava/lang/RuntimeException;",
            "Ljava/lang/Exception;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/util/UnknownFormatConversionException;"
        | "Ljava/util/IllegalFormatConversionException;"
        | "Ljava/util/MissingFormatArgumentException;" => Some(&[
            "Ljava/util/IllegalFormatException;",
            "Ljava/lang/IllegalArgumentException;",
            "Ljava/lang/RuntimeException;",
            "Ljava/lang/Exception;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/lang/ReflectiveOperationException;"
        | "Ljava/lang/InterruptedException;"
        | "Ljava/lang/CloneNotSupportedException;"
        | "Ljava/io/IOException;"
        | "Ljava/util/concurrent/TimeoutException;"
        | "Ljava/util/concurrent/ExecutionException;"
        | "Landroid/util/AndroidException;" => Some(EXC),
        "Ljava/lang/ClassNotFoundException;"
        | "Ljava/lang/InstantiationException;"
        | "Ljava/lang/NoSuchMethodException;"
        | "Ljava/lang/NoSuchFieldException;"
        | "Ljava/lang/IllegalAccessException;"
        | "Ljava/lang/reflect/InvocationTargetException;" => Some(&[
            "Ljava/lang/ReflectiveOperationException;",
            "Ljava/lang/Exception;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/io/FileNotFoundException;"
        | "Ljava/io/EOFException;"
        | "Landroid/util/MalformedJsonException;" => Some(&[
            "Ljava/io/IOException;",
            "Ljava/lang/Exception;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/provider/Settings$SettingNotFoundException;" => Some(&[
            "Landroid/util/AndroidException;",
            "Ljava/lang/Exception;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/lang/AbstractMethodError;" | "Ljava/lang/IncompatibleClassChangeError;" => Some(&[
            "Ljava/lang/IncompatibleClassChangeError;",
            "Ljava/lang/LinkageError;",
            "Ljava/lang/Error;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/lang/NegativeArraySizeException;" => Some(RUNTIME),
        "Ljava/lang/IllegalStateException;" => Some(RUNTIME),
        "Ljava/lang/IllegalArgumentException;" => Some(RUNTIME),
        "Ljava/lang/NumberFormatException;" => Some(RUNTIME),
        "Ljava/lang/ArrayStoreException;" => Some(RUNTIME),
        "Ljava/lang/RuntimeException;" => Some(EXC),
        // android.database exceptions (thrown by the SQLite backend; Room's
        // @Upsert catches SQLiteConstraintException to fall back to UPDATE)
        "Landroid/database/SQLException;" => Some(RUNTIME),
        "Landroid/database/sqlite/SQLiteException;" => Some(&[
            "Landroid/database/SQLException;",
            "Ljava/lang/RuntimeException;",
            "Ljava/lang/Exception;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/database/sqlite/SQLiteConstraintException;"
        | "Landroid/database/sqlite/SQLiteCantOpenDatabaseException;"
        | "Landroid/database/sqlite/SQLiteDiskIOException;"
        | "Landroid/database/sqlite/SQLiteFullException;"
        | "Landroid/database/sqlite/SQLiteDatabaseCorruptException;"
        | "Landroid/database/sqlite/SQLiteDatabaseLockedException;"
        | "Landroid/database/sqlite/SQLiteReadOnlyDatabaseException;"
        | "Landroid/database/sqlite/SQLiteDoneException;"
        | "Landroid/database/sqlite/SQLiteBlobTooBigException;" => Some(&[
            "Landroid/database/sqlite/SQLiteException;",
            "Landroid/database/SQLException;",
            "Ljava/lang/RuntimeException;",
            "Ljava/lang/Exception;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/lang/UnsupportedOperationException;" => Some(RUNTIME),
        "Ljava/lang/StackOverflowError;" => Some(ERR),
        "Ljava/lang/OutOfMemoryError;" => Some(ERR),
        // issue #41: hierarquia de ERROS de link/init — sem elas, EIIE/NCDFE
        // não casam nem com catch (Throwable) (não são subtipo de Throwable)
        "Ljava/lang/LinkageError;" => Some(ERR),
        "Ljava/lang/UnsatisfiedLinkError;" | "Ljava/lang/NoSuchMethodError;" => Some(&[
            "Ljava/lang/LinkageError;",
            "Ljava/lang/Error;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/lang/SecurityException;" => Some(RUNTIME),
        "Landroid/app/PendingIntent$CanceledException;" => Some(EXC),
        "Ljava/lang/ExceptionInInitializerError;" => Some(&[
            "Ljava/lang/LinkageError;",
            "Ljava/lang/Error;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/lang/NoClassDefFoundError;" => Some(&[
            "Ljava/lang/LinkageError;",
            "Ljava/lang/Error;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        // issue #57: campo estático inexistente na cadeia (JVMS 5.4.3.2) —
        // capturável como LinkageError/Error/Throwable, mesma família
        "Ljava/lang/NoSuchFieldError;" => Some(&[
            "Ljava/lang/LinkageError;",
            "Ljava/lang/Error;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/lang/Throwable;" => Some(&["Ljava/lang/Object;"]),
        "Ljava/lang/Exception;" => Some(EXC),
        "Ljava/lang/Error;" => Some(ERR),
        // ── M3.2: framework host Android ───────────────────────────────────
        "Landroid/content/Context;" => Some(OBJECT),
        "Ljava/text/BreakIterator;" => Some(OBJECT),
        // android.text layouts (graphics.rs): StaticLayout/BoringLayout/
        // DynamicLayout are Layouts (Compose casts them to Layout)
        "Landroid/text/Layout;" => Some(OBJECT),
        "Landroid/text/StaticLayout;"
        | "Landroid/text/BoringLayout;"
        | "Landroid/text/DynamicLayout;" => Some(&["Landroid/text/Layout;", "Ljava/lang/Object;"]),
        "Landroid/text/BoringLayout$Metrics;" => Some(&[
            "Landroid/graphics/Paint$FontMetricsInt;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/content/ContextWrapper;" => Some(CONTEXT),
        "Landroid/view/ContextThemeWrapper;" => Some(&[
            "Landroid/content/ContextWrapper;",
            "Landroid/content/Context;",
            "Ljava/lang/Object;",
        ]),
        // AOSP: Activity → ContextThemeWrapper → ContextWrapper → Context;
        // Application/Service → ContextWrapper (apps unwrap contexts with
        // `while (c instanceof ContextWrapper)` looking for owners)
        "Landroid/app/Activity;" => Some(&[
            "Landroid/view/ContextThemeWrapper;",
            "Landroid/content/ContextWrapper;",
            "Landroid/content/Context;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/app/Application;" | "Landroid/app/Service;" => Some(&[
            "Landroid/content/ContextWrapper;",
            "Landroid/content/Context;",
            "Ljava/lang/Object;",
        ]),
        // the base context ending every ContextWrapper chain
        "Landroid/app/ContextImpl;" => Some(CONTEXT),
        "Landroid/inputmethodservice/InputMethodService;"
        | "Landroid/accessibilityservice/AccessibilityService;"
        | "Landroid/service/notification/NotificationListenerService;" => Some(&[
            "Landroid/app/Service;",
            "Landroid/content/Context;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/widget/EditText;" => Some(TEXTVIEW),
        "Landroid/appwidget/AppWidgetProvider;" => {
            Some(&["Landroid/content/BroadcastReceiver;", "Ljava/lang/Object;"])
        }
        "Landroid/view/inputmethod/BaseInputConnection;" => Some(&[
            "Landroid/view/inputmethod/InputConnection;",
            "Ljava/lang/Object;",
        ]),
        // M8: FragmentManager legacy (29º gap real) — objeto simples do host.
        "Landroid/app/FragmentManager;" => Some(OBJECT),
        // M8: FragmentTransaction legacy (31º gap real) — ops até o commit.
        "Landroid/app/FragmentTransaction;" => Some(OBJECT),
        // M8: Fragment legacy (32º gap real) — super das fragments do app.
        "Landroid/app/Fragment;" => Some(OBJECT),
        "Landroid/content/Intent;" => Some(OBJECT),
        // java.util time types used by Logback's rolling-file initialization.
        "Ljava/util/Calendar;" => Some(OBJECT),
        "Ljava/util/GregorianCalendar;" => Some(&["Ljava/util/Calendar;", "Ljava/lang/Object;"]),
        "Ljava/util/TimeZone;" => Some(OBJECT),
        // ── M8 full scope ─────────────────────────────────────────────
        "Landroid/app/Dialog;" => Some(OBJECT),
        "Landroid/app/AlertDialog;" => Some(&["Landroid/app/Dialog;", "Ljava/lang/Object;"]),
        "Landroid/app/AlertDialog$Builder;" => Some(OBJECT),
        "Landroid/database/sqlite/SQLiteDatabase;" => Some(OBJECT),
        "Landroid/database/sqlite/SQLiteOpenHelper;" => Some(OBJECT),
        // jtext_format
        "Ljava/text/Format;"
        | "Ljava/text/DecimalFormatSymbols;"
        | "Ljava/text/DateFormatSymbols;" => Some(OBJECT),
        "Ljava/text/NumberFormat;" => Some(&["Ljava/text/Format;", "Ljava/lang/Object;"]),
        "Ljava/text/DecimalFormat;" => Some(&[
            "Ljava/text/NumberFormat;",
            "Ljava/text/Format;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/content/ComponentName;" => Some(OBJECT),
        // AssetManager implements AutoCloseable in AOSP; the host manager is
        // final and never subclassed by app DEX.
        "Landroid/content/res/AssetManager;" => {
            Some(&["Ljava/lang/AutoCloseable;", "Ljava/lang/Object;"])
        }
        "Landroid/graphics/drawable/Drawable;" | "Landroid/content/res/ColorStateList;" => {
            Some(OBJECT)
        }
        "Landroid/graphics/drawable/ColorDrawable;"
        | "Landroid/graphics/drawable/LayerDrawable;" => {
            Some(&["Landroid/graphics/drawable/Drawable;", "Ljava/lang/Object;"])
        }
        "Landroid/graphics/drawable/RippleDrawable;" => Some(&[
            "Landroid/graphics/drawable/LayerDrawable;",
            "Landroid/graphics/drawable/Drawable;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/view/MotionEvent;" => Some(&["Landroid/view/InputEvent;", "Ljava/lang/Object;"]),
        "Landroid/view/MotionEvent$PointerCoords;"
        | "Landroid/view/MotionEvent$PointerProperties;" => Some(OBJECT),
        "Landroid/graphics/ColorSpace;"
        | "Landroid/graphics/ColorSpace$Named;"
        | "Landroid/graphics/ColorSpace$Model;"
        | "Landroid/graphics/ColorSpace$Rgb$TransferParameters;" => Some(OBJECT),
        "Landroid/graphics/ColorSpace$Rgb;" => {
            Some(&["Landroid/graphics/ColorSpace;", "Ljava/lang/Object;"])
        }
        "Landroid/text/SpannableString;" | "Landroid/text/SpannableStringBuilder;" => Some(OBJECT),
        "Landroid/text/style/CharacterStyle;" => Some(OBJECT),
        "Landroid/text/style/MetricAffectingSpan;" => {
            Some(&["Landroid/text/style/CharacterStyle;", "Ljava/lang/Object;"])
        }
        "Landroid/text/style/ReplacementSpan;" => Some(&[
            "Landroid/text/style/MetricAffectingSpan;",
            "Landroid/text/style/CharacterStyle;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/text/style/ClickableSpan;" | "Landroid/text/style/UnderlineSpan;" => {
            Some(&["Landroid/text/style/CharacterStyle;", "Ljava/lang/Object;"])
        }
        "Landroid/content/pm/PackageManager;"
        | "Landroid/view/Display;"
        | "Landroid/content/pm/PackageInfo;"
        | "Landroid/content/pm/PackageItemInfo;" => Some(OBJECT),
        "Landroid/app/ApplicationPackageManager;" => {
            Some(&["Landroid/content/pm/PackageManager;", "Ljava/lang/Object;"])
        }
        "Landroid/content/pm/ApplicationInfo;" | "Landroid/content/pm/ComponentInfo;" => {
            Some(&["Landroid/content/pm/PackageItemInfo;", "Ljava/lang/Object;"])
        }
        "Landroid/content/pm/ActivityInfo;"
        | "Landroid/content/pm/ServiceInfo;"
        | "Landroid/content/pm/ProviderInfo;" => Some(&[
            "Landroid/content/pm/ComponentInfo;",
            "Landroid/content/pm/PackageItemInfo;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/nio/charset/Charset;" | "Ljava/util/UUID;" => Some(OBJECT),
        // jio_chars
        "Ljava/io/Writer;"
        | "Ljava/io/Reader;"
        | "Ljava/io/OutputStream;"
        | "Ljava/io/InputStream;" => Some(OBJECT),
        "Ljava/io/FilterOutputStream;" => Some(&["Ljava/io/OutputStream;", "Ljava/lang/Object;"]),
        "Ljava/io/BufferedOutputStream;" => Some(&[
            "Ljava/io/FilterOutputStream;",
            "Ljava/io/OutputStream;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/io/BufferedInputStream;" => Some(&[
            "Ljava/io/FilterInputStream;",
            "Ljava/io/InputStream;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/io/FilterInputStream;" => Some(&["Ljava/io/InputStream;", "Ljava/lang/Object;"]),
        "Landroid/util/Base64OutputStream;" => Some(&[
            "Ljava/io/FilterOutputStream;",
            "Ljava/io/OutputStream;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/util/Base64InputStream;" => Some(&[
            "Ljava/io/FilterInputStream;",
            "Ljava/io/InputStream;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/io/PrintStream;" => Some(&[
            "Ljava/io/FilterOutputStream;",
            "Ljava/io/OutputStream;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/io/StringWriter;"
        | "Ljava/io/OutputStreamWriter;"
        | "Ljava/io/BufferedWriter;"
        | "Ljava/io/PrintWriter;" => Some(&["Ljava/io/Writer;", "Ljava/lang/Object;"]),
        "Ljava/io/FileWriter;" => Some(&[
            "Ljava/io/OutputStreamWriter;",
            "Ljava/io/Writer;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/io/StringReader;" | "Ljava/io/InputStreamReader;" | "Ljava/io/BufferedReader;" => {
            Some(&["Ljava/io/Reader;", "Ljava/lang/Object;"])
        }
        "Ljava/io/FileReader;" => Some(&[
            "Ljava/io/InputStreamReader;",
            "Ljava/io/Reader;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/io/ByteArrayOutputStream;" => {
            Some(&["Ljava/io/OutputStream;", "Ljava/lang/Object;"])
        }
        "Ljava/io/ByteArrayInputStream;" => Some(&["Ljava/io/InputStream;", "Ljava/lang/Object;"]),
        // jsqlite: programs, cursors and the cursor driver.
        "Landroid/database/sqlite/SQLiteClosable;"
        | "Landroid/database/sqlite/SQLiteDirectCursorDriver;"
        | "Landroid/database/AbstractCursor;" => Some(OBJECT),
        "Landroid/database/sqlite/SQLiteProgram;" => Some(&[
            "Landroid/database/sqlite/SQLiteClosable;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/database/sqlite/SQLiteQuery;" | "Landroid/database/sqlite/SQLiteStatement;" => {
            Some(&[
                "Landroid/database/sqlite/SQLiteProgram;",
                "Landroid/database/sqlite/SQLiteClosable;",
                "Ljava/lang/Object;",
            ])
        }
        "Landroid/database/AbstractWindowedCursor;" => {
            Some(&["Landroid/database/AbstractCursor;", "Ljava/lang/Object;"])
        }
        "Landroid/database/sqlite/SQLiteCursor;" => Some(&[
            "Landroid/database/AbstractWindowedCursor;",
            "Landroid/database/AbstractCursor;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/content/ContentValues;" => Some(OBJECT),
        "Landroid/net/Uri;" => Some(OBJECT),
        "Landroid/net/Uri$Builder;" => Some(OBJECT),
        "Landroid/app/job/JobScheduler;" => Some(OBJECT),
        "Landroidx/recyclerview/widget/RecyclerView;" => Some(VIEWGROUP),
        "Landroidx/recyclerview/widget/RecyclerView$Adapter;" => Some(OBJECT),
        "Landroidx/recyclerview/widget/RecyclerView$ViewHolder;" => Some(OBJECT),
        "Landroidx/recyclerview/widget/RecyclerView$LayoutManager;" => Some(OBJECT),
        "Landroidx/recyclerview/widget/LinearLayoutManager;" => Some(&[
            "Landroidx/recyclerview/widget/RecyclerView$LayoutManager;",
            "Ljava/lang/Object;",
        ]),
        "Landroidx/recyclerview/widget/GridLayoutManager;" => Some(&[
            "Landroidx/recyclerview/widget/LinearLayoutManager;",
            "Landroidx/recyclerview/widget/RecyclerView$LayoutManager;",
            "Ljava/lang/Object;",
        ]),
        "Landroidx/fragment/app/Fragment;" => Some(OBJECT),
        "Landroidx/fragment/app/FragmentManager;" => Some(OBJECT),
        "Landroidx/fragment/app/FragmentTransaction;" => Some(OBJECT),
        "Lokhttp3/OkHttpClient;" => Some(OBJECT),
        "Lokhttp3/Request;" => Some(OBJECT),
        "Lokhttp3/Request$Builder;" => Some(OBJECT),
        "Lokhttp3/Call;" => Some(OBJECT),
        "Lokhttp3/Response;" => Some(OBJECT),
        "Lokhttp3/ResponseBody;" => Some(OBJECT),
        "Landroidx/work/WorkManager;" => Some(OBJECT),
        "Landroidx/work/Operation;" => Some(OBJECT),
        "Landroidx/work/WorkRequest;" => Some(OBJECT),
        "Landroidx/room/RoomDatabase;" => Some(OBJECT),
        "Landroidx/room/RoomDatabase$Builder;" => Some(OBJECT),
        // ── S7 (Apêndice D): hierarquias AndroidX/Material conhecidas ────
        // Cadeias FIXAS upstream (não dependem do app): permitem que o
        // LayoutInflater e os casts/check-cast resolvam widgets AndroidX
        // mesmo quando a lib não está (ainda) no DEX do APK. Só ACRESCENTAM
        // respostas positivas; o DEX real, quando presente, continua a
        // mandar na resolução de métodos/campos. Intermediários puramente
        // internos das libs (LinearLayoutCompat, VisibilityAwareImageButton,
        // NavigationBarView…) são colapsados — simplificação documentada.
        // Activities: AppCompatActivity → FragmentActivity →
        // ComponentActivity → Activity.
        "Landroidx/activity/ComponentActivity;" => Some(&[
            "Landroid/app/Activity;",
            "Landroid/view/ContextThemeWrapper;",
            "Landroid/content/ContextWrapper;",
            "Landroid/content/Context;",
            "Ljava/lang/Object;",
        ]),
        "Landroidx/fragment/app/FragmentActivity;" => Some(&[
            "Landroidx/activity/ComponentActivity;",
            "Landroid/app/Activity;",
            "Landroid/view/ContextThemeWrapper;",
            "Landroid/content/ContextWrapper;",
            "Landroid/content/Context;",
            "Ljava/lang/Object;",
        ]),
        "Landroidx/appcompat/app/AppCompatActivity;" => Some(&[
            "Landroidx/fragment/app/FragmentActivity;",
            "Landroidx/activity/ComponentActivity;",
            "Landroid/app/Activity;",
            "Landroid/view/ContextThemeWrapper;",
            "Landroid/content/ContextWrapper;",
            "Landroid/content/Context;",
            "Ljava/lang/Object;",
        ]),
        // Widgets appcompat (infláveis como as contrapartes do framework).
        "Landroidx/appcompat/widget/AppCompatTextView;" => Some(TEXTVIEW),
        "Landroidx/appcompat/widget/AppCompatEditText;" => Some(&[
            "Landroid/widget/EditText;",
            "Landroid/widget/TextView;",
            "Landroid/view/View;",
            "Ljava/lang/Object;",
        ]),
        "Landroidx/appcompat/widget/AppCompatAutoCompleteTextView;" => Some(&[
            "Landroid/widget/AutoCompleteTextView;",
            "Landroid/widget/EditText;",
            "Landroid/widget/TextView;",
            "Landroid/view/View;",
            "Ljava/lang/Object;",
        ]),
        "Landroidx/appcompat/widget/AppCompatMultiAutoCompleteTextView;" => Some(&[
            "Landroid/widget/MultiAutoCompleteTextView;",
            "Landroid/widget/AutoCompleteTextView;",
            "Landroid/widget/EditText;",
            "Landroid/widget/TextView;",
            "Landroid/view/View;",
            "Ljava/lang/Object;",
        ]),
        "Landroidx/appcompat/widget/AppCompatButton;" => Some(BUTTON),
        "Landroidx/appcompat/widget/AppCompatImageView;" => Some(IMAGEVIEW),
        "Landroidx/appcompat/widget/AppCompatImageButton;" => Some(IMAGEBUTTON),
        "Landroidx/appcompat/widget/AppCompatCheckBox;" => Some(&[
            "Landroid/widget/CheckBox;",
            "Landroid/widget/CompoundButton;",
            "Landroid/widget/Button;",
            "Landroid/widget/TextView;",
            "Landroid/view/View;",
            "Ljava/lang/Object;",
        ]),
        "Landroidx/appcompat/widget/AppCompatRadioButton;" => Some(RADIO_BUTTON),
        "Landroidx/appcompat/widget/AppCompatToggleButton;" => Some(&[
            "Landroid/widget/ToggleButton;",
            "Landroid/widget/CompoundButton;",
            "Landroid/widget/Button;",
            "Landroid/widget/TextView;",
            "Landroid/view/View;",
            "Ljava/lang/Object;",
        ]),
        "Landroidx/appcompat/widget/AppCompatSpinner;" => Some(SPINNER),
        "Landroidx/appcompat/widget/AppCompatSeekBar;" => Some(&[
            "Landroid/widget/SeekBar;",
            "Landroid/widget/AbsSeekBar;",
            "Landroid/widget/ProgressBar;",
            "Landroid/view/View;",
            "Ljava/lang/Object;",
        ]),
        "Landroidx/appcompat/widget/AppCompatRatingBar;" => Some(&[
            "Landroid/widget/RatingBar;",
            "Landroid/widget/AbsSeekBar;",
            "Landroid/widget/ProgressBar;",
            "Landroid/view/View;",
            "Ljava/lang/Object;",
        ]),
        "Landroidx/appcompat/widget/SwitchCompat;" => Some(COMPOUND_BUTTON),
        "Landroidx/appcompat/widget/Toolbar;" => Some(VIEWGROUP),
        "Landroidx/appcompat/widget/ActionMenuView;" => Some(VIEWGROUP),
        "Landroidx/appcompat/widget/LinearLayoutCompat;" => Some(VIEWGROUP),
        "Landroidx/appcompat/widget/SearchView;" => Some(VIEWGROUP),
        "Landroidx/appcompat/widget/AppCompatCheckedTextView;" => Some(&[
            "Landroid/widget/CheckedTextView;",
            "Landroid/widget/TextView;",
            "Landroid/view/View;",
            "Ljava/lang/Object;",
        ]),
        // Containers de layout (inflam como ViewGroup/FrameLayout host;
        // solvers/behaviors das libs NÃO são modelados — ver registo D).
        "Landroidx/constraintlayout/widget/ConstraintLayout;" => Some(VIEWGROUP),
        "Landroidx/coordinatorlayout/widget/CoordinatorLayout;" => Some(VIEWGROUP),
        "Landroidx/drawerlayout/widget/DrawerLayout;" => Some(VIEWGROUP),
        "Landroidx/cardview/widget/CardView;" => Some(FRAMELAYOUT),
        "Landroidx/swiperefreshlayout/widget/SwipeRefreshLayout;" => Some(VIEWGROUP),
        "Landroidx/viewpager/widget/ViewPager;" => Some(VIEWGROUP),
        "Landroidx/viewpager2/widget/ViewPager2;" => Some(VIEWGROUP),
        "Landroidx/core/widget/NestedScrollView;" => Some(FRAMELAYOUT),
        "Landroidx/fragment/app/FragmentContainerView;" => Some(FRAMELAYOUT),
        "Landroidx/appcompat/widget/ContentFrameLayout;" => Some(FRAMELAYOUT),
        // LayoutParams dos containers acima (casts do código DEX das libs).
        "Landroidx/constraintlayout/widget/ConstraintLayout$LayoutParams;" => {
            Some(MARGIN_LAYOUT_PARAMS)
        }
        "Landroidx/coordinatorlayout/widget/CoordinatorLayout$LayoutParams;" => {
            Some(MARGIN_LAYOUT_PARAMS)
        }
        "Landroidx/drawerlayout/widget/DrawerLayout$LayoutParams;" => Some(MARGIN_LAYOUT_PARAMS),
        "Landroidx/recyclerview/widget/RecyclerView$LayoutParams;" => Some(MARGIN_LAYOUT_PARAMS),
        "Landroidx/viewpager/widget/ViewPager$LayoutParams;" => Some(VIEWGROUP_LAYOUT_PARAMS),
        "Landroidx/appcompat/widget/LinearLayoutCompat$LayoutParams;" => Some(MARGIN_LAYOUT_PARAMS),
        // Google Material Components (cadeias colapsadas nos intermediários).
        "Lcom/google/android/material/button/MaterialButton;" => Some(&[
            "Landroidx/appcompat/widget/AppCompatButton;",
            "Landroid/widget/Button;",
            "Landroid/widget/TextView;",
            "Landroid/view/View;",
            "Ljava/lang/Object;",
        ]),
        "Lcom/google/android/material/textview/MaterialTextView;" => Some(&[
            "Landroidx/appcompat/widget/AppCompatTextView;",
            "Landroid/widget/TextView;",
            "Landroid/view/View;",
            "Ljava/lang/Object;",
        ]),
        "Lcom/google/android/material/textfield/TextInputLayout;" => Some(LINEARLAYOUT),
        "Lcom/google/android/material/textfield/TextInputEditText;" => Some(&[
            "Landroidx/appcompat/widget/AppCompatEditText;",
            "Landroid/widget/EditText;",
            "Landroid/widget/TextView;",
            "Landroid/view/View;",
            "Ljava/lang/Object;",
        ]),
        "Lcom/google/android/material/card/MaterialCardView;" => Some(&[
            "Landroidx/cardview/widget/CardView;",
            "Landroid/widget/FrameLayout;",
            "Landroid/view/ViewGroup;",
            "Landroid/view/View;",
            "Ljava/lang/Object;",
        ]),
        "Lcom/google/android/material/chip/Chip;" => Some(&[
            "Landroidx/appcompat/widget/AppCompatCheckBox;",
            "Landroid/widget/CheckBox;",
            "Landroid/widget/CompoundButton;",
            "Landroid/widget/Button;",
            "Landroid/widget/TextView;",
            "Landroid/view/View;",
            "Ljava/lang/Object;",
        ]),
        "Lcom/google/android/material/chip/ChipGroup;" => Some(VIEWGROUP),
        "Lcom/google/android/material/floatingactionbutton/FloatingActionButton;" => {
            Some(IMAGEBUTTON)
        }
        "Lcom/google/android/material/floatingactionbutton/ExtendedFloatingActionButton;" => {
            Some(&[
                "Lcom/google/android/material/button/MaterialButton;",
                "Landroidx/appcompat/widget/AppCompatButton;",
                "Landroid/widget/Button;",
                "Landroid/widget/TextView;",
                "Landroid/view/View;",
                "Ljava/lang/Object;",
            ])
        }
        "Lcom/google/android/material/bottomnavigation/BottomNavigationView;" => Some(FRAMELAYOUT),
        "Lcom/google/android/material/navigation/NavigationView;" => Some(FRAMELAYOUT),
        "Lcom/google/android/material/navigationrail/NavigationRailView;" => Some(FRAMELAYOUT),
        "Lcom/google/android/material/tabs/TabLayout;" => Some(&[
            "Landroid/widget/HorizontalScrollView;",
            "Landroid/widget/FrameLayout;",
            "Landroid/view/ViewGroup;",
            "Landroid/view/View;",
            "Ljava/lang/Object;",
        ]),
        "Lcom/google/android/material/appbar/AppBarLayout;" => Some(LINEARLAYOUT),
        "Lcom/google/android/material/appbar/CollapsingToolbarLayout;" => Some(FRAMELAYOUT),
        "Lcom/google/android/material/appbar/MaterialToolbar;" => Some(&[
            "Landroidx/appcompat/widget/Toolbar;",
            "Landroid/view/ViewGroup;",
            "Landroid/view/View;",
            "Ljava/lang/Object;",
        ]),
        "Lcom/google/android/material/checkbox/MaterialCheckBox;" => Some(&[
            "Landroidx/appcompat/widget/AppCompatCheckBox;",
            "Landroid/widget/CheckBox;",
            "Landroid/widget/CompoundButton;",
            "Landroid/widget/Button;",
            "Landroid/widget/TextView;",
            "Landroid/view/View;",
            "Ljava/lang/Object;",
        ]),
        "Lcom/google/android/material/radiobutton/MaterialRadioButton;" => Some(&[
            "Landroidx/appcompat/widget/AppCompatRadioButton;",
            "Landroid/widget/RadioButton;",
            "Landroid/widget/CompoundButton;",
            "Landroid/widget/Button;",
            "Landroid/widget/TextView;",
            "Landroid/view/View;",
            "Ljava/lang/Object;",
        ]),
        "Lcom/google/android/material/switchmaterial/SwitchMaterial;" => Some(&[
            "Landroidx/appcompat/widget/SwitchCompat;",
            "Landroid/widget/CompoundButton;",
            "Landroid/widget/Button;",
            "Landroid/widget/TextView;",
            "Landroid/view/View;",
            "Ljava/lang/Object;",
        ]),
        "Lcom/google/android/material/slider/Slider;" => Some(VIEW),
        "Lcom/google/android/material/slider/RangeSlider;" => Some(VIEW),
        "Lcom/google/android/material/progressindicator/CircularProgressIndicator;" => Some(VIEW),
        "Lcom/google/android/material/progressindicator/LinearProgressIndicator;" => Some(VIEW),
        "Lcom/google/android/material/bottomappbar/BottomAppBar;" => Some(&[
            "Landroidx/appcompat/widget/Toolbar;",
            "Landroid/view/ViewGroup;",
            "Landroid/view/View;",
            "Ljava/lang/Object;",
        ]),
        "Lcom/google/android/material/imageview/ShapeableImageView;" => Some(&[
            "Landroidx/appcompat/widget/AppCompatImageView;",
            "Landroid/widget/ImageView;",
            "Landroid/view/View;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/os/Bundle;" => Some(OBJECT),
        "Landroid/os/Handler;" => Some(OBJECT),
        "Landroid/os/Looper;" => Some(OBJECT),
        "Landroid/view/Window;" => Some(OBJECT),
        "Landroid/view/ViewGroup$LayoutParams;" => Some(VIEWGROUP_LAYOUT_PARAMS),
        "Landroid/view/ViewGroup$MarginLayoutParams;" => Some(MARGIN_LAYOUT_PARAMS),
        "Landroid/widget/LinearLayout$LayoutParams;" => Some(LINEAR_LAYOUT_PARAMS),
        "Landroid/widget/FrameLayout$LayoutParams;" => Some(FRAME_LAYOUT_PARAMS),
        "Landroid/widget/RelativeLayout$LayoutParams;" => Some(RELATIVE_LAYOUT_PARAMS),
        "Landroid/view/View;" => Some(VIEW),
        "Landroid/view/ViewGroup;" => Some(VIEWGROUP),
        "Landroid/widget/TextView;" => Some(TEXTVIEW),
        "Landroid/widget/ImageView;" => Some(IMAGEVIEW),
        "Landroid/widget/ImageButton;" => Some(IMAGEBUTTON),
        "Landroid/widget/ProgressBar;" => Some(PROGRESSBAR),
        "Landroid/widget/CompoundButton;" => Some(COMPOUND_BUTTON),
        "Landroid/widget/RadioButton;" => Some(RADIO_BUTTON),
        "Landroid/widget/Button;" => Some(BUTTON),
        "Landroid/widget/LinearLayout;" => Some(LINEARLAYOUT),
        "Landroid/widget/RadioGroup;" => Some(RADIO_GROUP),
        "Landroid/widget/FrameLayout;" => Some(FRAMELAYOUT),
        "Landroid/widget/ScrollView;" => Some(SCROLL_VIEW),
        "Landroid/widget/HorizontalScrollView;" => Some(HORIZONTAL_SCROLL_VIEW),
        "Landroid/widget/RelativeLayout;" => Some(RELATIVE_LAYOUT),
        "Landroid/widget/TableLayout;" => Some(TABLE_LAYOUT),
        "Landroid/widget/TableRow;" => Some(TABLE_ROW),
        "Landroid/widget/AdapterView;" => Some(ADAPTER_VIEW),
        "Landroid/widget/AbsListView;" => Some(ABS_LIST_VIEW),
        "Landroid/widget/ListView;" | "Landroid/widget/ExpandableListView;" => Some(LIST_VIEW),
        "Landroid/widget/GridView;" => Some(GRID_VIEW),
        "Landroid/widget/AbsSpinner;" => Some(ABS_SPINNER),
        "Landroid/widget/Spinner;" => Some(SPINNER),
        "Landroid/widget/AbsoluteLayout;" => Some(ABSOLUTE_LAYOUT),
        "Landroid/webkit/WebView;" => Some(WEB_VIEW),
        "Landroid/view/ViewStub;" => Some(VIEW),
        // ── M8: classes host do compat real (cadeia completa de ancestrais) ──
        "Ljava/util/AbstractCollection;" => Some(OBJECT),
        "Ljava/util/AbstractList;" => {
            Some(&["Ljava/util/AbstractCollection;", "Ljava/lang/Object;"])
        }
        "Ljava/util/AbstractSequentialList;" => Some(&[
            "Ljava/util/AbstractList;",
            "Ljava/util/AbstractCollection;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/util/AbstractSet;" => {
            Some(&["Ljava/util/AbstractCollection;", "Ljava/lang/Object;"])
        }
        "Ljava/util/HashSet;" => Some(&[
            "Ljava/util/AbstractSet;",
            "Ljava/util/AbstractCollection;",
            "Ljava/lang/Object;",
        ]),
        // M8: LinkedHashSet mantém a cadeia real de HashSet e, no host,
        // preserva ordem de inserção no Vec usado pelo Set.
        "Ljava/util/LinkedHashSet;" => Some(&[
            "Ljava/util/HashSet;",
            "Ljava/util/AbstractSet;",
            "Ljava/util/AbstractCollection;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/util/ArrayDeque;" => Some(&["Ljava/util/AbstractCollection;", "Ljava/lang/Object;"]),
        "Ljava/util/PriorityQueue;" => Some(&[
            "Ljava/util/AbstractQueue;",
            "Ljava/util/AbstractCollection;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/util/AbstractMap;" => Some(OBJECT),
        "Ljava/util/HashMap;" => Some(&["Ljava/util/AbstractMap;", "Ljava/lang/Object;"]),
        "Ljava/util/LinkedHashMap;" => Some(&[
            "Ljava/util/HashMap;",
            "Ljava/util/AbstractMap;",
            "Ljava/lang/Object;",
        ]),
        // ConcurrentHashMap NÃO estende HashMap na JDK (AbstractMap direto)
        "Ljava/util/concurrent/ConcurrentHashMap;" => {
            Some(&["Ljava/util/AbstractMap;", "Ljava/lang/Object;"])
        }
        "Ljava/util/WeakHashMap;" | "Ljava/util/IdentityHashMap;" => {
            Some(&["Ljava/util/AbstractMap;", "Ljava/lang/Object;"])
        }
        "Ljava/util/ArrayList;" => Some(&[
            "Ljava/util/AbstractList;",
            "Ljava/util/AbstractCollection;",
            "Ljava/lang/Object;",
        ]),
        // Vector/Stack share list storage intrinsics but retain their JDK
        // inheritance chain for guest check-cast and instanceof operations.
        "Ljava/util/Vector;" => Some(&[
            "Ljava/util/AbstractList;",
            "Ljava/util/AbstractCollection;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/util/Stack;" => Some(&[
            "Ljava/util/Vector;",
            "Ljava/util/AbstractList;",
            "Ljava/util/AbstractCollection;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/util/LinkedList;" => Some(&[
            "Ljava/util/AbstractSequentialList;",
            "Ljava/util/AbstractList;",
            "Ljava/util/AbstractCollection;",
            "Ljava/lang/Object;",
        ]),
        // CopyOnWrite*: na JDK real NÃO estendem Abstract* (implementam direto)
        "Ljava/util/concurrent/CopyOnWriteArrayList;" => Some(OBJECT),
        "Ljava/util/concurrent/LinkedBlockingQueue;" => Some(&[
            "Ljava/util/AbstractQueue;",
            "Ljava/util/AbstractCollection;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/util/AbstractQueue;" => {
            Some(&["Ljava/util/AbstractCollection;", "Ljava/lang/Object;"])
        }
        "Ljava/util/concurrent/CopyOnWriteArraySet;" => Some(OBJECT),
        "Ljava/util/concurrent/atomic/AtomicReference;" => Some(OBJECT),
        "Ljava/lang/Number;" => Some(OBJECT),
        // primitive wrappers (boxes.rs): numeric ones extend Number
        "Ljava/lang/Integer;"
        | "Ljava/lang/Long;"
        | "Ljava/lang/Short;"
        | "Ljava/lang/Byte;"
        | "Ljava/lang/Float;"
        | "Ljava/lang/Double;" => Some(&["Ljava/lang/Number;", "Ljava/lang/Object;"]),
        "Ljava/lang/Boolean;" | "Ljava/lang/Character;" => Some(OBJECT),
        "Ljava/util/concurrent/AbstractExecutorService;" => Some(OBJECT),
        "Ljava/util/concurrent/ForkJoinPool;" => Some(&[
            "Ljava/util/concurrent/AbstractExecutorService;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/text/method/BaseMovementMethod;" => Some(OBJECT),
        "Landroid/text/method/LinkMovementMethod;" => Some(&[
            "Landroid/text/method/BaseMovementMethod;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/lang/Thread;" => Some(OBJECT),
        "Ljava/lang/ThreadLocal;" => Some(OBJECT),
        "Ljava/lang/InheritableThreadLocal;" => {
            Some(&["Ljava/lang/ThreadLocal;", "Ljava/lang/Object;"])
        }
        "Ljava/lang/Class;" => Some(OBJECT),
        "Ljava/lang/ClassLoader;" => Some(OBJECT),
        // M8: ServiceLoader host é um Iterable vazio enquanto a descoberta
        // `META-INF/services` do APK permanece fora do runtime.
        "Ljava/util/ServiceLoader;" => Some(OBJECT),
        "Ljava/util/concurrent/atomic/AtomicInteger;"
        | "Ljava/util/concurrent/atomic/AtomicLong;" => {
            Some(&["Ljava/lang/Number;", "Ljava/lang/Object;"])
        }
        "Ljava/util/concurrent/atomic/AtomicBoolean;" => Some(OBJECT),
        "Ljava/util/concurrent/atomic/AtomicReferenceFieldUpdater;" => Some(OBJECT),
        "Ljava/lang/Enum;" => Some(OBJECT),
        "Landroid/util/JsonReader;" | "Landroid/util/JsonWriter;" => Some(OBJECT),
        "Landroid/util/JsonToken;" => Some(&["Ljava/lang/Enum;", "Ljava/lang/Object;"]),
        "Landroid/util/SimpleArrayMap;" => Some(OBJECT),
        "Landroid/util/ArrayMap;" => Some(&["Landroid/util/SimpleArrayMap;", "Ljava/lang/Object;"]),
        "Landroid/util/ArraySet;" => Some(OBJECT),
        // java.util.Random (jrandom.rs)
        "Ljava/util/Random;" => Some(OBJECT),
        "Ljava/util/concurrent/ThreadLocalRandom;" => {
            Some(&["Ljava/util/Random;", "Ljava/lang/Object;"])
        }
        // java.math (jbigdecimal.rs)
        "Ljava/math/BigDecimal;" | "Ljava/math/BigInteger;" => {
            Some(&["Ljava/lang/Number;", "Ljava/lang/Object;"])
        }
        "Ljava/math/RoundingMode;" | "Ljava/text/Normalizer$Form;" => {
            Some(&["Ljava/lang/Enum;", "Ljava/lang/Object;"])
        }
        "Ljava/lang/ref/Reference;" => Some(OBJECT),
        "Ljava/lang/ref/WeakReference;" => {
            Some(&["Ljava/lang/ref/Reference;", "Ljava/lang/Object;"])
        }
        "Ljava/lang/ref/SoftReference;" => {
            Some(&["Ljava/lang/ref/Reference;", "Ljava/lang/Object;"])
        }
        "Ljava/lang/ref/ReferenceQueue;" => Some(OBJECT),
        // Appendix C stub surfaces promoted to bounded partial slices.
        "Landroid/webkit/WebSettings;"
        | "Landroid/provider/DocumentsContract;"
        | "Landroid/provider/DocumentsContract$Path;"
        | "Landroid/security/NetworkSecurityPolicy;"
        | "Landroid/security/KeyChain;"
        | "Landroid/hardware/camera2/CameraManager;"
        | "Landroid/hardware/camera2/CameraMetadata;"
        | "Landroid/hardware/camera2/impl/CameraMetadataNative;"
        | "Landroid/hardware/camera2/impl/CameraMetadataNative$Key;"
        | "Landroid/hardware/camera2/utils/TypeReference;"
        | "Landroid/hardware/camera2/utils/HashCodeHelpers;"
        | "Landroid/hardware/camera2/CameraCharacteristics$Key;"
        | "Landroid/hardware/camera2/CaptureRequest$Key;"
        | "Landroid/hardware/camera2/CaptureRequest$Builder;"
        | "Landroid/hardware/camera2/CaptureResult$Key;"
        | "Landroid/hardware/camera2/params/StreamConfiguration;"
        | "Landroid/hardware/camera2/params/StreamConfigurationDuration;"
        | "Landroid/hardware/camera2/params/StreamConfigurationMap;"
        | "Landroid/hardware/camera2/params/HighSpeedVideoConfiguration;"
        | "Landroid/hardware/camera2/params/ReprocessFormatsMap;"
        | "Landroid/hardware/camera2/params/MeteringRectangle;"
        | "Landroid/hardware/camera2/params/RggbChannelVector;"
        | "Landroid/hardware/camera2/params/BlackLevelPattern;"
        | "Landroid/hardware/camera2/params/ColorSpaceTransform;"
        | "Landroid/view/textclassifier/TextClassifier;"
        | "Landroid/view/textclassifier/TextClassifier$EntityConfig;"
        | "Landroid/view/textclassifier/TextClassifier$EntityConfig$Builder;"
        | "Landroid/view/textclassifier/TextClassification;"
        | "Landroid/view/textclassifier/TextClassification$Builder;"
        | "Landroid/view/textclassifier/TextClassification$Request;"
        | "Landroid/view/textclassifier/TextClassification$Request$Builder;"
        | "Landroid/view/textclassifier/TextSelection;"
        | "Landroid/view/textclassifier/TextSelection$Builder;"
        | "Landroid/view/textclassifier/TextSelection$Request;"
        | "Landroid/view/textclassifier/TextSelection$Request$Builder;"
        | "Landroid/view/textclassifier/TextLanguage;"
        | "Landroid/view/textclassifier/TextLanguage$Builder;"
        | "Landroid/view/textclassifier/TextLanguage$Request;"
        | "Landroid/view/textclassifier/TextLanguage$Request$Builder;"
        | "Landroid/view/textclassifier/TextLinks;"
        | "Landroid/view/textclassifier/TextLinks$Builder;"
        | "Landroid/view/textclassifier/TextLinks$TextLink;"
        | "Landroid/view/textclassifier/TextLinks$Request;"
        | "Landroid/view/textclassifier/TextLinks$Request$Builder;"
        | "Landroid/view/textclassifier/ConversationActions;"
        | "Landroid/view/textclassifier/ConversationAction;"
        | "Landroid/view/textclassifier/ConversationAction$Builder;"
        | "Landroid/view/textclassifier/ConversationActions$Message;"
        | "Landroid/view/textclassifier/ConversationActions$Message$Builder;"
        | "Landroid/view/textclassifier/ConversationActions$Request;"
        | "Landroid/view/textclassifier/ConversationActions$Request$Builder;"
        | "Landroid/view/textclassifier/TextClassificationManager;"
        | "Landroid/icu/util/ULocale;"
        | "Landroid/view/textclassifier/EntityConfidence;" => Some(OBJECT),
        "Landroid/animation/Animator;"
        | "Landroid/animation/Animator$AnimatorListener;"
        | "Landroid/animation/Animator$AnimatorPauseListener;"
        | "Landroid/animation/ValueAnimator$AnimatorUpdateListener;"
        | "Landroid/animation/ValueAnimator$DurationScaleChangeListener;"
        | "Landroid/animation/AnimationHandler;"
        | "Landroid/animation/AnimationHandler$AnimationFrameCallback;"
        | "Landroid/animation/PropertyValuesHolder;"
        | "Landroid/animation/Keyframe;"
        | "Landroid/animation/TypeEvaluator;"
        | "Landroid/animation/TypeConverter;"
        | "Landroid/animation/BidirectionalTypeConverter;"
        | "Landroid/animation/IntEvaluator;"
        | "Landroid/animation/FloatEvaluator;"
        | "Landroid/animation/ArgbEvaluator;"
        | "Landroid/animation/AnimatorListenerAdapter;"
        | "Landroid/util/Property;" => Some(OBJECT),
        "Landroid/util/IntProperty;" | "Landroid/util/FloatProperty;" => {
            Some(&["Landroid/util/Property;", "Ljava/lang/Object;"])
        }
        "Landroid/animation/ValueAnimator;" => {
            Some(&["Landroid/animation/Animator;", "Ljava/lang/Object;"])
        }
        "Landroid/animation/ObjectAnimator;" => Some(&[
            "Landroid/animation/ValueAnimator;",
            "Landroid/animation/Animator;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/animation/Keyframe$ObjectKeyframe;"
        | "Landroid/animation/Keyframe$IntKeyframe;"
        | "Landroid/animation/Keyframe$FloatKeyframe;" => {
            Some(&["Landroid/animation/Keyframe;", "Ljava/lang/Object;"])
        }
        "Landroid/service/autofill/AutofillService;" => Some(&[
            "Landroid/app/Service;",
            "Landroid/content/ContextWrapper;",
            "Landroid/content/Context;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/service/autofill/RegexValidator;"
        | "Landroid/service/autofill/RequiredValidators;"
        | "Landroid/service/autofill/OptionalValidators;"
        | "Landroid/service/autofill/NegationValidator;" => Some(&[
            "Landroid/service/autofill/InternalValidator;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/service/autofill/TextValueSanitizer;"
        | "Landroid/service/autofill/DateValueSanitizer;" => Some(&[
            "Landroid/service/autofill/InternalSanitizer;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/view/autofill/AutofillId;"
        | "Landroid/view/autofill/AutofillValue;"
        | "Landroid/service/autofill/Dataset;"
        | "Landroid/service/autofill/Dataset$Builder;"
        | "Landroid/service/autofill/Dataset$DatasetFieldFilter;"
        | "Landroid/service/autofill/Field;"
        | "Landroid/service/autofill/Field$Builder;"
        | "Landroid/service/autofill/Presentations;"
        | "Landroid/service/autofill/Presentations$Builder;"
        | "Landroid/service/autofill/InlinePresentation;"
        | "Landroid/service/autofill/FillResponse;"
        | "Landroid/service/autofill/FillResponse$Builder;"
        | "Landroid/service/autofill/SaveInfo;"
        | "Landroid/service/autofill/SaveInfo$Builder;"
        | "Landroid/service/autofill/UserData;"
        | "Landroid/service/autofill/UserData$Builder;"
        | "Landroid/service/autofill/InternalValidator;"
        | "Landroid/service/autofill/InternalSanitizer;"
        | "Landroid/service/autofill/Validator;"
        | "Landroid/service/autofill/Sanitizer;"
        | "Landroid/service/autofill/ValueFinder;"
        | "Landroid/service/autofill/Validators;"
        | "Landroid/service/autofill/FillRequest;"
        | "Landroid/service/autofill/SaveRequest;"
        | "Landroid/service/autofill/FillContext;"
        | "Landroid/service/autofill/FillCallback;"
        | "Landroid/service/autofill/SaveCallback;"
        | "Landroid/service/autofill/IFillCallback;"
        | "Landroid/service/autofill/ISaveCallback;"
        | "Landroid/service/autofill/FieldClassification;"
        | "Landroid/service/autofill/FieldClassification$Match;"
        | "Landroid/service/autofill/FieldClassificationUserData;"
        | "Landroid/service/autofill/CustomDescription;"
        | "Landroid/service/autofill/CustomDescription$Builder;"
        | "Landroid/service/autofill/SavedDatasetsInfo;"
        | "Landroid/service/autofill/SavedDatasetsInfoCallback;"
        | "Landroid/app/assist/AssistStructure;"
        | "Landroid/app/slice/Slice;"
        | "Landroid/widget/inline/InlinePresentationSpec;" => Some(OBJECT),
        "Landroid/hardware/camera2/CameraCharacteristics;"
        | "Landroid/hardware/camera2/CaptureRequest;"
        | "Landroid/hardware/camera2/CaptureResult;" => Some(&[
            "Landroid/hardware/camera2/CameraMetadata;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/hardware/camera2/TotalCaptureResult;" => Some(&[
            "Landroid/hardware/camera2/CaptureResult;",
            "Landroid/hardware/camera2/CameraMetadata;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/hardware/camera2/CameraAccessException;" => Some(&[
            "Landroid/util/AndroidException;",
            "Ljava/lang/Exception;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/transition/Transition;"
        | "Landroid/transition/Transition$TransitionListener;"
        | "Landroid/transition/Transition$EpicenterCallback;"
        | "Landroid/transition/Transition$AnimationInfo;"
        | "Landroid/transition/TransitionValues;"
        | "Landroid/transition/TransitionValuesMaps;"
        | "Landroid/transition/TransitionManager;"
        | "Landroid/transition/TransitionManager$MultiListener;"
        | "Landroid/transition/Scene;"
        | "Landroid/transition/TransitionPropagation;"
        | "Landroid/transition/PathMotion;"
        | "Landroid/transition/TransitionListenerAdapter;" => Some(OBJECT),
        "Landroid/transition/Visibility;" => {
            Some(&["Landroid/transition/Transition;", "Ljava/lang/Object;"])
        }
        "Landroid/transition/Fade;" => Some(&[
            "Landroid/transition/Visibility;",
            "Landroid/transition/Transition;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/transition/ChangeBounds;" => {
            Some(&["Landroid/transition/Transition;", "Ljava/lang/Object;"])
        }
        "Landroid/transition/TransitionSet;" => {
            Some(&["Landroid/transition/Transition;", "Ljava/lang/Object;"])
        }
        "Landroid/transition/AutoTransition;" => Some(&[
            "Landroid/transition/TransitionSet;",
            "Landroid/transition/Transition;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/transition/Transition$1;" => {
            Some(&["Landroid/transition/PathMotion;", "Ljava/lang/Object;"])
        }
        "Landroid/transition/Transition$2;" | "Landroid/transition/Transition$3;" => Some(&[
            "Landroid/animation/AnimatorListenerAdapter;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/transition/Fade$FadeAnimatorListener;" => Some(&[
            "Landroid/animation/AnimatorListenerAdapter;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/transition/Fade$1;"
        | "Landroid/transition/TransitionSet$TransitionSetListener;"
        | "Landroid/transition/TransitionSet$1;"
        | "Landroid/transition/TransitionManager$1;"
        | "Landroid/transition/Visibility$DisappearListener;"
        | "Landroid/transition/Visibility$1;" => Some(&[
            "Landroid/transition/TransitionListenerAdapter;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/transition/ChangeBounds$1;" | "Landroid/transition/ChangeBounds$ViewBounds;" => {
            Some(OBJECT)
        }
        "Landroid/system/Os;"
        | "Landroid/system/OsConstants;"
        | "Landroid/system/StructStat;"
        | "Landroid/system/StructStatVfs;"
        | "Landroid/system/StructTimespec;"
        | "Landroid/system/StructUtsname;" => Some(OBJECT),
        "Landroid/system/ErrnoException;" => Some(&[
            "Ljava/lang/Exception;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        // Appendix C Batch 57-A bounded partial slices.
        "Landroid/view/textservice/TextServicesManager;"
        | "Landroid/view/textservice/SpellCheckerSession;"
        | "Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionListener;"
        | "Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionParams;"
        | "Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionParams$BaseBuilder;"
        | "Landroid/view/textservice/SpellCheckerSession$SpellCheckerSessionParams$Builder;"
        | "Landroid/view/textservice/TextInfo;"
        | "Landroid/view/textservice/SuggestionsInfo;"
        | "Landroid/view/textservice/SentenceSuggestionsInfo;"
        | "Landroid/view/textservice/SpellCheckerSubtype;"
        | "Landroid/view/textservice/SpellCheckerInfo;"
        | "Landroid/view/translation/TranslationManager;"
        | "Landroid/view/translation/TranslationSpec;"
        | "Landroid/view/translation/TranslationRequestValue;"
        | "Landroid/view/translation/ViewTranslationRequest;"
        | "Landroid/view/translation/ViewTranslationRequest$Builder;"
        | "Landroid/view/translation/TranslationContext;"
        | "Landroid/view/translation/TranslationContext$BaseBuilder;"
        | "Landroid/view/translation/TranslationCapability;"
        | "Landroid/view/translation/Translator;"
        | "Landroid/hardware/usb/UsbManager;"
        | "Landroid/hardware/usb/UsbConstants;"
        | "Landroid/hardware/usb/UsbDevice;"
        | "Landroid/hardware/usb/UsbDevice$Builder;"
        | "Landroid/hardware/usb/UsbConfiguration;"
        | "Landroid/hardware/usb/UsbInterface;"
        | "Landroid/hardware/usb/UsbEndpoint;"
        | "Landroid/hardware/usb/UsbAccessory;"
        | "Landroid/hardware/usb/UsbDeviceConnection;"
        | "Landroid/telephony/euicc/EuiccManager;"
        | "Landroid/telephony/euicc/EuiccInfo;"
        | "Landroid/app/usage/UsageStatsManager;"
        | "Landroid/app/usage/UsageStats;"
        | "Landroid/app/usage/UsageEvents;"
        | "Landroid/app/usage/UsageEvents$Event;"
        | "Landroid/print/PrintManager;"
        | "Landroid/print/PrintManager$PrintJobStateChangeListener;"
        | "Landroid/print/PrintManager$PrintServicesChangeListener;"
        | "Landroid/print/PrintManager$PrintServiceRecommendationsChangeListener;"
        | "Landroid/print/PrintAttributes;"
        | "Landroid/print/PrintAttributes$Builder;"
        | "Landroid/print/PrintAttributes$MediaSize;"
        | "Landroid/print/PrintAttributes$Resolution;"
        | "Landroid/print/PrintAttributes$Margins;"
        | "Landroid/print/PageRange;"
        | "Landroid/print/PrinterId;"
        | "Landroid/print/PrintJobId;"
        | "Landroid/security/keystore/KeyProperties;"
        | "Landroid/security/keystore/KeyGenParameterSpec;"
        | "Landroid/security/keystore/KeyGenParameterSpec$Builder;"
        | "Landroid/security/keystore/KeyProtection;"
        | "Landroid/security/keystore/KeyProtection$Builder;"
        | "Landroid/security/keystore/KeyInfo;"
        | "Landroid/mtp/MtpConstants;"
        | "Landroid/mtp/MtpDeviceInfo;"
        | "Landroid/mtp/MtpStorageInfo;"
        | "Landroid/mtp/MtpObjectInfo;"
        | "Landroid/mtp/MtpObjectInfo$Builder;"
        | "Landroid/mtp/MtpDevice;"
        | "Landroid/mtp/MtpEvent;" => Some(OBJECT),
        // Appendix C Batch 49-A bounded partial slices.
        "Landroid/hardware/fingerprint/FingerprintManager;"
        | "Landroid/hardware/fingerprint/FingerprintManager$AuthenticationResult;"
        | "Landroid/hardware/biometrics/BiometricManager;"
        | "Landroid/hardware/biometrics/BiometricManager$Authenticators;"
        | "Landroid/hardware/biometrics/BiometricManager$Strings;"
        | "Landroid/hardware/biometrics/BiometricPrompt;"
        | "Landroid/hardware/biometrics/BiometricPrompt$Builder;"
        | "Landroid/hardware/biometrics/CryptoObject;"
        | "Landroid/hardware/biometrics/PromptInfo;"
        | "Landroid/hardware/biometrics/BiometricAuthenticator;"
        | "Landroid/hardware/biometrics/BiometricAuthenticator$Identifier;"
        | "Landroid/hardware/biometrics/BiometricAuthenticator$AuthenticationResult;"
        | "Landroid/hardware/biometrics/BiometricAuthenticator$AuthenticationCallback;"
        | "Landroid/hardware/biometrics/BiometricConstants;"
        | "Landroid/hardware/biometrics/BiometricFingerprintConstants;"
        | "Landroid/hardware/hdmi/HdmiControlManager;"
        | "Landroid/hardware/hdmi/HdmiUtils;"
        | "Landroid/hardware/hdmi/DeviceFeatures;"
        | "Landroid/hardware/hdmi/DeviceFeatures$Builder;"
        | "Landroid/hardware/hdmi/HdmiDeviceInfo;"
        | "Landroid/hardware/hdmi/HdmiDeviceInfo$Builder;"
        | "Landroid/hardware/hdmi/HdmiPortInfo;"
        | "Landroid/hardware/hdmi/HdmiPortInfo$Builder;"
        | "Landroid/hardware/input/InputManager;"
        | "Landroid/hardware/input/InputManager$InputDeviceListener;"
        | "Landroid/hardware/input/HostUsiVersion;"
        | "Landroid/hardware/input/InputDeviceIdentifier;"
        | "Landroid/view/InputDevice;"
        | "Landroid/view/InputDevice$Builder;"
        | "Landroid/view/InputDevice$MotionRange;"
        | "Landroid/hardware/soundtrigger/SoundTrigger;"
        | "Landroid/hardware/soundtrigger/SoundTrigger$SoundModel;"
        | "Landroid/hardware/soundtrigger/SoundTrigger$Keyphrase;"
        | "Landroid/hardware/soundtrigger/SoundTrigger$ModelParamRange;"
        | "Landroid/hardware/soundtrigger/SoundTrigger$RecognitionConfig;"
        | "Landroid/hardware/soundtrigger/SoundTrigger$ConfidenceLevel;"
        | "Landroid/hardware/soundtrigger/SoundTrigger$KeyphraseRecognitionExtra;"
        | "Landroid/hardware/soundtrigger/KeyphraseMetadata;"
        | "Landroid/provider/OneTimeUseBuilder;"
        | "Landroid/media/midi/MidiManager;"
        | "Landroid/media/midi/MidiManager$DeviceCallback;"
        | "Landroid/media/midi/MidiDeviceInfo;"
        | "Landroid/media/midi/MidiDeviceInfo$PortInfo;"
        | "Landroid/media/midi/MidiReceiver;"
        | "Landroid/media/tv/TvInputManager;"
        | "Landroid/media/tv/TvInputManager$TvInputCallback;"
        | "Landroid/media/tv/TvContentRating;"
        | "Landroid/media/tv/TvTrackInfo;"
        | "Landroid/media/tv/TvTrackInfo$Builder;"
        | "Landroid/companion/CompanionDeviceManager;"
        | "Landroid/companion/CompanionDeviceManager$Callback;"
        | "Landroid/companion/CompanionDeviceManager$OnAssociationsChangedListener;"
        | "Landroid/companion/AssociationRequest;"
        | "Landroid/companion/AssociationInfo;"
        | "Landroid/companion/AssociationInfo$Builder;"
        | "Landroid/companion/AssociationInfo$NonActionableBuilder;"
        | "Landroid/companion/BluetoothDeviceFilter;"
        | "Landroid/companion/BluetoothLeDeviceFilter;"
        | "Landroid/companion/WifiDeviceFilter;"
        | "Landroid/companion/WifiDeviceFilter$Builder;"
        | "Landroid/companion/DeviceFilter;"
        | "Landroid/net/MacAddress;" => Some(OBJECT),
        "Landroid/hardware/fingerprint/Fingerprint;" => Some(&[
            "Landroid/hardware/biometrics/BiometricAuthenticator$Identifier;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/hardware/fingerprint/FingerprintManager$CryptoObject;"
        | "Landroid/hardware/biometrics/BiometricPrompt$CryptoObject;" => Some(&[
            "Landroid/hardware/biometrics/CryptoObject;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/hardware/fingerprint/FingerprintManager$AuthenticationCallback;"
        | "Landroid/hardware/biometrics/BiometricPrompt$AuthenticationCallback;" => Some(&[
            "Landroid/hardware/biometrics/BiometricAuthenticator$AuthenticationCallback;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/hardware/biometrics/BiometricPrompt$AuthenticationResult;" => Some(&[
            "Landroid/hardware/biometrics/BiometricAuthenticator$AuthenticationResult;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/hardware/soundtrigger/SoundTrigger$GenericSoundModel;"
        | "Landroid/hardware/soundtrigger/SoundTrigger$KeyphraseSoundModel;" => Some(&[
            "Landroid/hardware/soundtrigger/SoundTrigger$SoundModel;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/companion/AssociationRequest$Builder;"
        | "Landroid/companion/BluetoothDeviceFilter$Builder;"
        | "Landroid/companion/BluetoothLeDeviceFilter$Builder;" => {
            Some(&["Landroid/provider/OneTimeUseBuilder;", "Ljava/lang/Object;"])
        }
        "Landroid/view/translation/TranslationContext$Builder;" => Some(&[
            "Landroid/view/translation/TranslationContext$BaseBuilder;",
            "Ljava/lang/Object;",
        ]),
        // Appendix C Batch 41-A bounded service-extension slices.
        "Landroid/service/quicksettings/TileService;"
        | "Landroid/service/wallpaper/WallpaperService;"
        | "Landroid/service/dreams/DreamService;"
        | "Landroid/service/chooser/ChooserTargetService;"
        | "Landroid/service/carrier/CarrierService;"
        | "Landroid/service/media/MediaBrowserService;" => Some(&[
            "Landroid/app/Service;",
            "Landroid/content/ContextWrapper;",
            "Landroid/content/Context;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/service/quicksettings/Tile;"
        | "Landroid/service/wallpaper/WallpaperService$Engine;"
        | "Lcom/android/internal/view/BaseSurfaceHolder;"
        | "Landroid/service/dreams/DreamService$DreamMetadata;"
        | "Landroid/service/chooser/ChooserTarget;"
        | "Landroid/service/carrier/CarrierIdentifier;"
        | "Landroid/service/media/MediaBrowserService$BrowserRoot;"
        | "Landroid/service/media/MediaBrowserService$Result;" => Some(OBJECT),
        "Landroid/service/wallpaper/WallpaperService$Engine$1;" => Some(&[
            "Lcom/android/internal/view/BaseSurfaceHolder;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/service/chooser/ChooserTargetService$IChooserTargetServiceWrapper;" => Some(&[
            "Landroid/service/chooser/IChooserTargetService$Stub;",
            "Landroid/os/Binder;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/service/chooser/IChooserTargetService$Stub;" => {
            Some(&["Landroid/os/Binder;", "Ljava/lang/Object;"])
        }
        "Landroid/service/carrier/CarrierService$ICarrierServiceWrapper;" => Some(&[
            "Landroid/service/carrier/ICarrierService$Stub;",
            "Landroid/os/Binder;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/service/carrier/ICarrierService$Stub;" => {
            Some(&["Landroid/os/Binder;", "Ljava/lang/Object;"])
        }
        "Landroid/service/media/MediaBrowserService$ServiceBinder;" => Some(&[
            "Landroid/service/media/IMediaBrowserService$Stub;",
            "Landroid/os/Binder;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/service/media/IMediaBrowserService$Stub;" => {
            Some(&["Landroid/os/Binder;", "Ljava/lang/Object;"])
        }
        // Appendix C Batch 35-A bounded android.speech /
        // android.service.textservice slices.
        "Landroid/speech/RecognitionService;"
        | "Landroid/service/textservice/SpellCheckerService;" => Some(&[
            "Landroid/app/Service;",
            "Landroid/content/ContextWrapper;",
            "Landroid/content/Context;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/speech/SpeechRecognizer;"
        | "Landroid/speech/RecognizerIntent;"
        | "Landroid/speech/RecognitionService$Callback;"
        | "Landroid/speech/RecognitionService$SupportCallback;"
        | "Landroid/speech/RecognitionSupport;"
        | "Landroid/speech/RecognitionSupport$Builder;"
        | "Landroid/speech/RecognitionPart;"
        | "Landroid/speech/RecognitionPart$BaseBuilder;"
        | "Landroid/speech/AlternativeSpan;"
        | "Landroid/service/textservice/SpellCheckerService$Session;" => Some(OBJECT),
        "Landroid/speech/SpeechRecognizerProxy;" | "Landroid/speech/SpeechRecognizerImpl;" => {
            Some(&["Landroid/speech/SpeechRecognizer;", "Ljava/lang/Object;"])
        }
        "Landroid/speech/RecognitionPart$Builder;" => Some(&[
            "Landroid/speech/RecognitionPart$BaseBuilder;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/speech/RecognitionService$RecognitionServiceBinder;" => Some(&[
            "Landroid/speech/IRecognitionService$Stub;",
            "Landroid/os/Binder;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/speech/IRecognitionService$Stub;" => {
            Some(&["Landroid/os/Binder;", "Ljava/lang/Object;"])
        }
        "Landroid/service/textservice/SpellCheckerService$InternalISpellCheckerSession;" => {
            Some(&[
                "Lcom/android/internal/textservice/ISpellCheckerSession$Stub;",
                "Landroid/os/Binder;",
                "Ljava/lang/Object;",
            ])
        }
        "Lcom/android/internal/textservice/ISpellCheckerSession$Stub;"
        | "Lcom/android/internal/textservice/ISpellCheckerService$Stub;" => {
            Some(&["Landroid/os/Binder;", "Ljava/lang/Object;"])
        }
        "Landroid/service/textservice/SpellCheckerService$SpellCheckerServiceBinder;" => Some(&[
            "Lcom/android/internal/textservice/ISpellCheckerService$Stub;",
            "Landroid/os/Binder;",
            "Ljava/lang/Object;",
        ]),
        // Appendix C Batch 35-B bounded android.speech.tts /
        // android.service.voice slices.
        "Landroid/speech/tts/TextToSpeechService;"
        | "Landroid/service/voice/VoiceInteractionService;" => Some(&[
            "Landroid/app/Service;",
            "Landroid/content/ContextWrapper;",
            "Landroid/content/Context;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/speech/tts/TextToSpeech;"
        | "Landroid/speech/tts/TextToSpeech$Engine;"
        | "Landroid/speech/tts/TextToSpeech$EngineInfo;"
        | "Landroid/speech/tts/Voice;"
        | "Landroid/speech/tts/SynthesisRequest;"
        | "Landroid/speech/tts/UtteranceProgressListener;"
        | "Landroid/service/voice/VoiceInteractionSession;"
        | "Landroid/service/voice/VoiceInteractionSession$AssistState;"
        | "Landroid/service/voice/VoiceInteractionSession$ActivityId;"
        | "Landroid/service/voice/HotwordDetectedResult;"
        | "Landroid/service/voice/HotwordDetectedResult$BaseBuilder;"
        | "Landroid/service/voice/HotwordRejectedResult;"
        | "Landroid/service/voice/HotwordRejectedResult$Builder;" => Some(OBJECT),
        "Landroid/service/voice/HotwordDetectedResult$Builder;" => Some(&[
            "Landroid/service/voice/HotwordDetectedResult$BaseBuilder;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/speech/tts/ITextToSpeechService$Stub;"
        | "Landroid/service/voice/IVoiceInteractionService$Stub;" => {
            Some(&["Landroid/os/Binder;", "Ljava/lang/Object;"])
        }
        // Appendix C Batch 35-C bounded android.location /
        // android.app.slice slices.
        "Landroid/location/Location;"
        | "Landroid/location/Criteria;"
        | "Landroid/app/slice/Slice$Builder;"
        | "Landroid/app/slice/SliceItem;"
        | "Landroid/app/slice/SliceSpec;"
        | "Landroid/app/slice/SliceManager;" => Some(OBJECT),
        // Appendix C Batch 35-D bounded android.app.backup /
        // android.telephony.gsm slices.
        "Landroid/app/backup/BackupAgent;" => Some(&[
            "Landroid/content/ContextWrapper;",
            "Landroid/content/Context;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/app/backup/BackupManager;"
        | "Landroid/app/backup/RestoreObserver;"
        | "Landroid/app/backup/BackupObserver;"
        | "Landroid/app/backup/BackupRestoreEventLogger;"
        | "Landroid/telephony/CellLocation;" => Some(OBJECT),
        "Landroid/telephony/gsm/GsmCellLocation;" => {
            Some(&["Landroid/telephony/CellLocation;", "Ljava/lang/Object;"])
        }
        "Landroid/app/backup/BackupAgent$BackupServiceBinder;" => Some(&[
            "Landroid/app/IBackupAgent$Stub;",
            "Landroid/os/Binder;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/app/IBackupAgent$Stub;" => Some(&["Landroid/os/Binder;", "Ljava/lang/Object;"]),
        // Appendix C Batch 35-E bounded android.net.http /
        // android.se.omapi slices.
        "Landroid/net/http/SslError;"
        | "Landroid/net/http/SslCertificate;"
        | "Landroid/net/http/SslCertificate$DName;"
        | "Landroid/se/omapi/SEService;"
        | "Landroid/se/omapi/Reader;"
        | "Landroid/se/omapi/SEService$1;" => Some(OBJECT),
        "Landroid/se/omapi/SEService$SEListener;" => Some(&[
            "Landroid/se/omapi/ISecureElementListener$Stub;",
            "Landroid/os/Binder;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/se/omapi/ISecureElementListener$Stub;" => {
            Some(&["Landroid/os/Binder;", "Ljava/lang/Object;"])
        }
        // Appendix C Batch 36-A bounded android.opengl /
        // android.security.identity slices.
        "Landroid/opengl/Matrix;" | "Landroid/security/identity/IdentityCredentialStore;" => {
            Some(OBJECT)
        }
        "Landroid/security/identity/CredstoreIdentityCredentialStore;" => Some(&[
            "Landroid/security/identity/IdentityCredentialStore;",
            "Ljava/lang/Object;",
        ]),
        // Appendix C Batch 36-B bounded android.hardware.Camera /
        // android.nfc slices.
        "Landroid/hardware/Camera;"
        | "Landroid/hardware/Camera$CameraInfo;"
        | "Landroid/hardware/Camera$Size;"
        | "Landroid/hardware/Camera$Area;"
        | "Landroid/nfc/NfcAdapter;"
        | "Landroid/nfc/NfcManager;" => Some(OBJECT),
        // Appendix C Batch 36-C bounded android.net.nsd /
        // android.net.sip slices.
        "Landroid/net/nsd/NsdServiceInfo;"
        | "Landroid/net/nsd/NsdManager;"
        | "Landroid/net/nsd/NsdManager$NsdCallbackImpl;"
        | "Landroid/net/sip/SipManager;" => Some(OBJECT),
        "Landroid/net/sip/SipException;" => Some(EXC),
        // Appendix C Batch 36-D bounded Bluetooth / LE and NFC
        // card-emulation / tag-technology slices.
        "Landroid/content/AttributionSource;"
        | "Landroid/bluetooth/BluetoothAdapter;"
        | "Landroid/bluetooth/BluetoothManager;"
        | "Landroid/bluetooth/BluetoothDevice;"
        | "Landroid/bluetooth/le/BluetoothLeScanner;"
        | "Landroid/bluetooth/le/BluetoothLeAdvertiser;"
        | "Landroid/bluetooth/le/ScanSettings;"
        | "Landroid/bluetooth/le/ScanSettings$Builder;"
        | "Landroid/bluetooth/le/ScanFilter;"
        | "Landroid/bluetooth/le/ScanFilter$Builder;"
        | "Landroid/nfc/Tag;"
        | "Landroid/nfc/tech/BasicTagTechnology;"
        | "Landroid/nfc/cardemulation/CardEmulation;"
        | "Landroid/nfc/cardemulation/AidGroup;" => Some(OBJECT),
        "Landroid/nfc/tech/NfcA;" => Some(&[
            "Landroid/nfc/tech/BasicTagTechnology;",
            "Ljava/lang/Object;",
        ]),
        // Appendix C Batch 36-E bounded media values / session /
        // browse / projection / audiofx slices.
        "Landroid/media/MediaMetadata;"
        | "Landroid/media/MediaMetadata$Builder;"
        | "Landroid/media/MediaDescription;"
        | "Landroid/media/MediaDescription$Builder;"
        | "Landroid/media/Rating;"
        | "Landroid/media/session/PlaybackState;"
        | "Landroid/media/session/PlaybackState$Builder;"
        | "Landroid/media/session/PlaybackState$CustomAction;"
        | "Landroid/media/session/PlaybackState$CustomAction$Builder;"
        | "Landroid/media/session/MediaSession;"
        | "Landroid/media/session/MediaSession$QueueItem;"
        | "Landroid/media/session/MediaSession$Token;"
        | "Landroid/media/session/MediaSession$Callback;"
        | "Landroid/media/session/MediaSessionManager;"
        | "Landroid/media/session/MediaSessionManager$SessionsChangedWrapper;"
        | "Landroid/media/session/MediaController;"
        | "Landroid/media/browse/MediaBrowser;"
        | "Landroid/media/browse/MediaBrowser$MediaItem;"
        | "Landroid/media/browse/MediaBrowser$ConnectionCallback;"
        | "Landroid/media/browse/MediaBrowser$SubscriptionCallback;"
        | "Landroid/media/browse/MediaBrowser$ItemCallback;"
        | "Landroid/media/browse/MediaBrowser$Subscription;"
        | "Landroid/media/projection/MediaProjectionManager;"
        | "Landroid/media/projection/MediaProjectionManager$Callback;"
        | "Landroid/media/projection/MediaProjection;"
        | "Landroid/media/projection/MediaProjection$Callback;"
        | "Landroid/media/projection/MediaProjection$CallbackRecord;"
        | "Landroid/media/projection/MediaProjectionInfo;"
        | "Landroid/media/audiofx/AudioEffect;"
        | "Landroid/media/audiofx/AudioEffect$Descriptor;" => Some(OBJECT),
        "Landroid/media/session/MediaSession$CallbackStub;" => Some(&[
            "Landroid/media/session/ISessionCallback$Stub;",
            "Landroid/os/Binder;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/media/projection/MediaProjectionManager$CallbackDelegate;" => Some(&[
            "Landroid/media/projection/IMediaProjectionWatcherCallback$Stub;",
            "Landroid/os/Binder;",
            "Ljava/lang/Object;",
        ]),
        // Appendix C Batch 36-F bounded PDF / RenderScript slices.
        "Landroid/graphics/pdf/PdfDocument;"
        | "Landroid/graphics/pdf/PdfDocument$PageInfo;"
        | "Landroid/graphics/pdf/PdfDocument$PageInfo$Builder;"
        | "Landroid/graphics/pdf/PdfDocument$Page;"
        | "Landroid/graphics/pdf/PdfRenderer;"
        | "Landroid/graphics/pdf/PdfRenderer$Page;"
        | "Landroid/renderscript/RenderScript;"
        | "Landroid/renderscript/BaseObj;"
        | "Landroid/renderscript/Float2;"
        | "Landroid/renderscript/Float3;"
        | "Landroid/renderscript/Float4;"
        | "Landroid/renderscript/Matrix2f;"
        | "Landroid/renderscript/Matrix3f;"
        | "Landroid/renderscript/Matrix4f;" => Some(OBJECT),
        "Landroid/renderscript/Element;" => {
            Some(&["Landroid/renderscript/BaseObj;", "Ljava/lang/Object;"])
        }
        "Landroid/renderscript/RenderScript$ContextType;"
        | "Landroid/renderscript/Element$DataType;"
        | "Landroid/renderscript/Element$DataKind;" => {
            Some(&["Ljava/lang/Enum;", "Ljava/lang/Object;"])
        }
        "Landroid/renderscript/RSRuntimeException;" => Some(RUNTIME),
        "Landroid/renderscript/RSInvalidStateException;"
        | "Landroid/renderscript/RSIllegalArgumentException;" => Some(&[
            "Landroid/renderscript/RSRuntimeException;",
            "Ljava/lang/RuntimeException;",
            "Ljava/lang/Exception;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        // Appendix C Batch 36-G bounded credentials / device-admin slices.
        "Landroid/service/credentials/Action;"
        | "Landroid/service/credentials/BeginCreateCredentialRequest;"
        | "Landroid/service/credentials/BeginCreateCredentialResponse;"
        | "Landroid/service/credentials/BeginCreateCredentialResponse$Builder;"
        | "Landroid/service/credentials/BeginGetCredentialOption;"
        | "Landroid/service/credentials/BeginGetCredentialRequest;"
        | "Landroid/service/credentials/BeginGetCredentialRequest$Builder;"
        | "Landroid/service/credentials/BeginGetCredentialResponse;"
        | "Landroid/service/credentials/BeginGetCredentialResponse$Builder;"
        | "Landroid/service/credentials/CallingAppInfo;"
        | "Landroid/service/credentials/ClearCredentialStateRequest;"
        | "Landroid/service/credentials/CreateCredentialRequest;"
        | "Landroid/service/credentials/CreateEntry;"
        | "Landroid/service/credentials/CredentialEntry;"
        | "Landroid/service/credentials/GetCredentialRequest;"
        | "Landroid/service/credentials/RemoteEntry;"
        | "Landroid/credentials/CredentialOption;"
        | "Landroid/credentials/CredentialOption$Builder;"
        | "Landroid/app/admin/DevicePolicyManager;"
        | "Landroid/app/admin/SystemUpdatePolicy;" => Some(OBJECT),
        "Landroid/service/credentials/CredentialProviderService;" => Some(&[
            "Landroid/app/Service;",
            "Landroid/content/Context;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/app/admin/SystemUpdatePolicy$ValidationFailedException;" => Some(&[
            "Ljava/lang/IllegalArgumentException;",
            "Ljava/lang/RuntimeException;",
            "Ljava/lang/Exception;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        // Appendix C Batch 36-H bounded Wi-Fi / IPsec-IKE slices.
        "Landroid/net/wifi/WifiManager;"
        | "Landroid/net/wifi/WifiInfo;"
        | "Landroid/net/wifi/ScanResult;"
        | "Landroid/net/wifi/WifiSsid;"
        | "Landroid/net/ipsec/ike/SaProposal;"
        | "Landroid/net/ipsec/ike/SaProposal$Builder;"
        | "Landroid/net/ipsec/ike/IkeTrafficSelector;"
        | "Landroid/net/ipsec/ike/IkeIdentification;"
        | "Landroid/net/ipsec/ike/ChildSessionParams;"
        | "Landroid/net/ipsec/ike/ChildSessionParams$Builder;"
        | "Landroid/util/Pair;"
        | "Ljava/net/InetAddress;" => Some(OBJECT),
        "Landroid/net/wifi/SupplicantState;" | "Landroid/net/NetworkInfo$DetailedState;" => {
            Some(&["Ljava/lang/Enum;", "Ljava/lang/Object;"])
        }
        "Landroid/net/ipsec/ike/IkeSaProposal;" | "Landroid/net/ipsec/ike/ChildSaProposal;" => {
            Some(&["Landroid/net/ipsec/ike/SaProposal;", "Ljava/lang/Object;"])
        }
        "Landroid/net/ipsec/ike/IkeSaProposal$Builder;"
        | "Landroid/net/ipsec/ike/ChildSaProposal$Builder;" => Some(&[
            "Landroid/net/ipsec/ike/SaProposal$Builder;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/net/ipsec/ike/IkeFqdnIdentification;"
        | "Landroid/net/ipsec/ike/IkeRfc822AddrIdentification;"
        | "Landroid/net/ipsec/ike/IkeKeyIdIdentification;"
        | "Landroid/net/ipsec/ike/IkeIpv4AddrIdentification;" => Some(&[
            "Landroid/net/ipsec/ike/IkeIdentification;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/net/ipsec/ike/TransportModeChildSessionParams;"
        | "Landroid/net/ipsec/ike/TunnelModeChildSessionParams;" => Some(&[
            "Landroid/net/ipsec/ike/ChildSessionParams;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/net/ipsec/ike/TransportModeChildSessionParams$Builder;"
        | "Landroid/net/ipsec/ike/TunnelModeChildSessionParams$Builder;" => Some(&[
            "Landroid/net/ipsec/ike/ChildSessionParams$Builder;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/net/Inet4Address;" | "Ljava/net/Inet6Address;" => {
            Some(&["Ljava/net/InetAddress;", "Ljava/lang/Object;"])
        }
        // Appendix C Batch 36-I bounded telephony / MBMS / telecom slices.
        "Landroid/telephony/PhoneNumberUtils;"
        | "Landroid/telephony/TelephonyManager;"
        | "Landroid/telephony/SubscriptionManager;"
        | "Landroid/telephony/ServiceState;"
        | "Landroid/telephony/CellBroadcastIdRange;"
        | "Landroid/telephony/mbms/DownloadRequest;"
        | "Landroid/telephony/mbms/DownloadRequest$Builder;"
        | "Landroid/telephony/mbms/FileInfo;"
        | "Landroid/telecom/TelecomManager;"
        | "Landroid/telecom/PhoneAccountHandle;"
        | "Landroid/telecom/PhoneAccount;"
        | "Landroid/telecom/PhoneAccount$Builder;"
        | "Landroid/telecom/DisconnectCause;"
        | "Landroid/telecom/CallAudioState;"
        | "Landroid/telecom/VideoProfile;"
        | "Landroid/telecom/VideoProfile$CameraCapabilities;"
        | "Landroid/telecom/StatusHints;"
        | "Landroid/telecom/GatewayInfo;" => Some(OBJECT),
        _ => None,
    }
}

/// Interfaces de plataforma conhecidas sem DEX (issue #26: check-cast /
/// instanceof para CharSequence/Comparable/… sobre String/StringBuilder).
fn builtin_interfaces(class: &str) -> Option<&'static [&'static str]> {
    const STRING_IFACES: &[&str] = &[
        "Ljava/lang/CharSequence;",
        "Ljava/lang/Comparable;",
        "Ljava/io/Serializable;",
    ];
    const SB_IFACES: &[&str] = &["Ljava/lang/CharSequence;", "Ljava/lang/Appendable;"];
    // M8: closure transitiva explícita (List/Set → Collection → Iterable)
    const COLL: &[&str] = &["Ljava/util/Collection;", "Ljava/lang/Iterable;"];
    const LIST_IFACES: &[&str] = &[
        "Ljava/util/List;",
        "Ljava/util/Collection;",
        "Ljava/lang/Iterable;",
    ];
    const VECTOR_IFACES: &[&str] = &[
        "Ljava/util/List;",
        "Ljava/util/Collection;",
        "Ljava/lang/Iterable;",
        "Ljava/util/RandomAccess;",
        "Ljava/lang/Cloneable;",
        "Ljava/io/Serializable;",
    ];
    const TIMEZONE_IFACES: &[&str] = &["Ljava/lang/Cloneable;", "Ljava/io/Serializable;"];
    const CALENDAR_IFACES: &[&str] = &[
        "Ljava/lang/Cloneable;",
        "Ljava/io/Serializable;",
        "Ljava/lang/Comparable;",
    ];
    const MAP_IFACES: &[&str] = &["Ljava/util/Map;"];
    match class {
        "Ljava/lang/String;" => Some(STRING_IFACES),
        // S7 (Apêndice D): donos de lifecycle AndroidX — check-cast para
        // LifecycleOwner & cia. funciona mesmo sem o DEX das libs.
        "Landroidx/activity/ComponentActivity;"
        | "Landroidx/fragment/app/FragmentActivity;"
        | "Landroidx/appcompat/app/AppCompatActivity;" => Some(&[
            "Landroidx/lifecycle/LifecycleOwner;",
            "Landroidx/lifecycle/ViewModelStoreOwner;",
            "Landroidx/savedstate/SavedStateRegistryOwner;",
            "Landroidx/activity/OnBackPressedDispatcherOwner;",
            "Landroidx/activity/result/ActivityResultRegistryOwner;",
        ]),
        "Landroidx/fragment/app/Fragment;" => Some(&[
            "Landroidx/lifecycle/LifecycleOwner;",
            "Landroidx/lifecycle/ViewModelStoreOwner;",
            "Landroidx/savedstate/SavedStateRegistryOwner;",
        ]),
        "Ljava/lang/StringBuilder;" | "Ljava/lang/StringBuffer;" => Some(SB_IFACES),
        "Ljava/util/regex/Pattern;" => Some(&["Ljava/io/Serializable;"]),
        "Ljava/util/regex/Matcher;" => Some(&["Ljava/util/regex/MatchResult;"]),
        "Ljava/text/Format;"
        | "Ljava/text/NumberFormat;"
        | "Ljava/text/DecimalFormat;"
        | "Ljava/text/DecimalFormatSymbols;"
        | "Ljava/text/DateFormatSymbols;" => {
            Some(&["Ljava/lang/Cloneable;", "Ljava/io/Serializable;"])
        }
        "Ljava/nio/charset/Charset;" => Some(&["Ljava/lang/Comparable;"]),
        "Ljava/util/UUID;" => Some(&["Ljava/lang/Comparable;", "Ljava/io/Serializable;"]),
        "Landroid/net/Uri;" => Some(&["Landroid/os/Parcelable;", "Ljava/lang/Comparable;"]),
        "Landroid/os/Binder;" => Some(&["Landroid/os/IBinder;"]),
        // framework value types that apps (and Compose's SaveableStateRegistry)
        // test with `Parcelable.class.isInstance(..)` / check-cast
        "Landroid/os/Bundle;"
        | "Landroid/os/PersistableBundle;"
        | "Landroid/content/Intent;"
        | "Landroid/graphics/Bitmap;"
        | "Landroid/graphics/Rect;"
        | "Landroid/graphics/RectF;"
        | "Landroid/graphics/Point;"
        | "Landroid/graphics/PointF;" => {
            Some(&["Landroid/os/Parcelable;", "Ljava/lang/Cloneable;"])
        }
        "Landroid/util/SparseArray;" => Some(&["Ljava/lang/Cloneable;"]),
        "Landroid/text/SpannableString;" => Some(&[
            "Landroid/text/Spannable;",
            "Landroid/text/Spanned;",
            "Ljava/lang/CharSequence;",
            "Landroid/text/GetChars;",
        ]),
        "Landroid/text/SpannableStringBuilder;" => Some(&[
            "Landroid/text/Editable;",
            "Landroid/text/Spannable;",
            "Landroid/text/Spanned;",
            "Ljava/lang/CharSequence;",
            "Landroid/text/GetChars;",
            "Ljava/lang/Appendable;",
        ]),
        "Landroid/text/method/BaseMovementMethod;" | "Landroid/text/method/LinkMovementMethod;" => {
            Some(&["Landroid/text/method/MovementMethod;"])
        }
        "Landroid/content/ComponentName;" => Some(&[
            "Landroid/os/Parcelable;",
            "Ljava/lang/Cloneable;",
            "Ljava/lang/Comparable;",
        ]),
        "Ljava/io/Writer;" => Some(&[
            "Ljava/lang/Appendable;",
            "Ljava/io/Closeable;",
            "Ljava/io/Flushable;",
            "Ljava/lang/AutoCloseable;",
        ]),
        "Ljava/io/Reader;" => Some(&[
            "Ljava/lang/Readable;",
            "Ljava/io/Closeable;",
            "Ljava/lang/AutoCloseable;",
        ]),
        // jsqlite host classes
        "Landroid/database/sqlite/SQLiteDirectCursorDriver;" => {
            Some(&["Landroid/database/sqlite/SQLiteCursorDriver;"])
        }
        "Landroid/database/sqlite/SQLiteClosable;"
        | "Landroid/database/sqlite/SQLiteProgram;"
        | "Landroid/database/sqlite/SQLiteQuery;"
        | "Landroid/database/sqlite/SQLiteStatement;"
        | "Landroid/database/sqlite/SQLiteDatabase;" => {
            Some(&["Ljava/io/Closeable;", "Ljava/lang/AutoCloseable;"])
        }
        "Landroid/database/sqlite/SQLiteCursor;"
        | "Landroid/database/AbstractWindowedCursor;"
        | "Landroid/database/AbstractCursor;" => Some(&[
            "Landroid/database/CrossProcessCursor;",
            "Landroid/database/Cursor;",
            "Ljava/io/Closeable;",
            "Ljava/lang/AutoCloseable;",
        ]),
        // java.util.concurrent.locks host classes (jlocks.rs)
        "Ljava/util/concurrent/locks/ReentrantLock;" => Some(&[
            "Ljava/util/concurrent/locks/Lock;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/concurrent/locks/ReentrantReadWriteLock;" => Some(&[
            "Ljava/util/concurrent/locks/ReadWriteLock;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/concurrent/locks/ReentrantReadWriteLock$ReadLock;"
        | "Ljava/util/concurrent/locks/ReentrantReadWriteLock$WriteLock;" => Some(&[
            "Ljava/util/concurrent/locks/Lock;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/io/File;" => Some(&["Ljava/lang/Comparable;", "Ljava/io/Serializable;"]),
        "Ljava/io/FileOutputStream;" | "Ljava/io/FileInputStream;" => {
            Some(&["Ljava/io/Closeable;", "Ljava/lang/AutoCloseable;"])
        }
        "Landroid/util/JsonReader;" => Some(&["Ljava/io/Closeable;", "Ljava/lang/AutoCloseable;"]),
        "Landroid/util/JsonWriter;" => Some(&[
            "Ljava/io/Closeable;",
            "Ljava/io/Flushable;",
            "Ljava/lang/AutoCloseable;",
        ]),
        "Ljava/io/BufferedOutputStream;"
        | "Ljava/io/BufferedInputStream;"
        | "Ljava/io/FilterOutputStream;"
        | "Ljava/io/FilterInputStream;"
        | "Ljava/io/OutputStream;"
        | "Ljava/io/InputStream;" => Some(&[
            "Ljava/io/Closeable;",
            "Ljava/io/Flushable;",
            "Ljava/lang/AutoCloseable;",
        ]),
        "Ljava/nio/channels/FileLock;" => Some(&["Ljava/lang/AutoCloseable;"]),
        // host XML resource parser (xml_res.rs) is a pull parser + AttributeSet
        "Landroid/content/res/XmlResourceParser;" => Some(&[
            "Lorg/xmlpull/v1/XmlPullParser;",
            "Landroid/util/AttributeSet;",
            "Ljava/lang/AutoCloseable;",
        ]),
        // java.util time classes inherit these standard JDK interfaces.
        "Ljava/util/Calendar;" | "Ljava/util/GregorianCalendar;" => Some(CALENDAR_IFACES),
        "Ljava/util/TimeZone;" => Some(TIMEZONE_IFACES),
        // ── M8: interfaces das classes host do compat real ──
        "Ljava/util/AbstractCollection;" => Some(COLL),
        // JDK collection interfaces: super-interface closure, so a DEX class
        // implementing Set/List/Queue/… is a Collection/Iterable (check-cast)
        "Ljava/util/Collection;" => Some(&["Ljava/lang/Iterable;"]),
        // AOSP/JDK: ListIterator extends Iterator; Kotlin's EmptyIterator
        // implements ListIterator and is returned as java.util.Iterator.
        "Ljava/util/ListIterator;" => Some(&["Ljava/util/Iterator;"]),
        "Ljava/util/Set;" | "Ljava/util/List;" | "Ljava/util/Queue;" => Some(COLL),
        "Ljava/util/Deque;" | "Ljava/util/concurrent/BlockingQueue;" => Some(&[
            "Ljava/util/Queue;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
        ]),
        "Ljava/util/SortedSet;" => Some(&[
            "Ljava/util/Set;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
        ]),
        "Ljava/util/NavigableSet;" => Some(&[
            "Ljava/util/SortedSet;",
            "Ljava/util/Set;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
        ]),
        "Ljava/util/SortedMap;" | "Ljava/util/concurrent/ConcurrentMap;" => {
            Some(&["Ljava/util/Map;"])
        }
        "Ljava/util/NavigableMap;" => Some(&["Ljava/util/SortedMap;", "Ljava/util/Map;"]),
        "Ljava/util/TreeSet;" => Some(&[
            "Ljava/util/NavigableSet;",
            "Ljava/util/SortedSet;",
            "Ljava/util/Set;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
            "Ljava/lang/Cloneable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/TreeMap;" => Some(&[
            "Ljava/util/NavigableMap;",
            "Ljava/util/SortedMap;",
            "Ljava/util/Map;",
            "Ljava/lang/Cloneable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/AbstractList;" | "Ljava/util/AbstractSequentialList;" => Some(LIST_IFACES),
        "Ljava/util/AbstractMap;" => Some(MAP_IFACES),
        "Ljava/util/HashMap;" => Some(&[
            "Ljava/util/Map;",
            "Ljava/lang/Cloneable;",
            "Ljava/io/Serializable;",
        ]),
        "Landroid/util/ArrayMap;" => Some(&["Ljava/util/Map;"]),
        "Landroid/util/ArraySet;" => Some(&[
            "Ljava/util/Set;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
        ]),
        "Ljava/util/WeakHashMap;"
        | "Ljava/util/IdentityHashMap;"
        | "Ljava/util/LinkedHashMap;"
        | "Ljava/util/concurrent/ConcurrentHashMap;" => Some(&[
            "Ljava/util/Map;",
            "Ljava/util/concurrent/ConcurrentMap;",
            "Ljava/lang/Cloneable;",
            "Ljava/io/Serializable;",
        ]),
        // Immutable views from java.util.Collections (intrinsics.rs markers).
        "Ljava/util/Collections$UnmodifiableSet;" => Some(&[
            "Ljava/util/Set;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/Collections$SingletonList;" => Some(&[
            "Ljava/util/List;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
            "Ljava/util/RandomAccess;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/Collections$UnmodifiableMap;" => {
            Some(&["Ljava/util/Map;", "Ljava/io/Serializable;"])
        }
        "Ljava/util/AbstractSet;" | "Ljava/util/HashSet;" | "Ljava/util/LinkedHashSet;" => Some(&[
            "Ljava/util/Set;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
            "Ljava/lang/Cloneable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/ArrayList;" => Some(&[
            "Ljava/util/List;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
            "Ljava/util/RandomAccess;",
            "Ljava/lang/Cloneable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/Vector;" | "Ljava/util/Stack;" => Some(VECTOR_IFACES),
        "Ljava/util/LinkedList;" => Some(&[
            "Ljava/util/List;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
            "Ljava/util/Deque;",
            "Ljava/util/Queue;",
            "Ljava/lang/Cloneable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/concurrent/LinkedBlockingQueue;" => Some(&[
            "Ljava/util/concurrent/BlockingQueue;",
            "Ljava/util/Queue;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/AbstractQueue;" => Some(&[
            "Ljava/util/Queue;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
        ]),
        "Ljava/util/PriorityQueue;" => Some(&[
            "Ljava/util/Queue;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/ArrayDeque;" => Some(&[
            "Ljava/util/Deque;",
            "Ljava/util/Queue;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
            "Ljava/lang/Cloneable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/concurrent/CopyOnWriteArrayList;" => Some(&[
            "Ljava/util/List;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
            "Ljava/util/RandomAccess;",
            "Ljava/lang/Cloneable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/concurrent/CopyOnWriteArraySet;" => Some(&[
            "Ljava/util/Set;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
            "Ljava/lang/Cloneable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/lang/Integer;"
        | "Ljava/lang/Long;"
        | "Ljava/lang/Short;"
        | "Ljava/lang/Byte;"
        | "Ljava/lang/Float;"
        | "Ljava/lang/Double;"
        | "Ljava/lang/Boolean;"
        | "Ljava/lang/Character;" => Some(&["Ljava/lang/Comparable;", "Ljava/io/Serializable;"]),
        "Ljava/util/concurrent/atomic/AtomicReference;"
        | "Ljava/util/concurrent/atomic/AtomicInteger;"
        | "Ljava/util/concurrent/atomic/AtomicLong;"
        | "Ljava/util/concurrent/atomic/AtomicBoolean;" => Some(&["Ljava/io/Serializable;"]),
        "Ljava/util/concurrent/AbstractExecutorService;"
        | "Ljava/util/concurrent/ForkJoinPool;" => Some(&[
            "Ljava/util/concurrent/ExecutorService;",
            "Ljava/util/concurrent/Executor;",
        ]),
        "Ljava/util/concurrent/ExecutorService;" => Some(&["Ljava/util/concurrent/Executor;"]),
        "Ljava/util/ServiceLoader;" => Some(&["Ljava/lang/Iterable;"]),
        "Ljava/math/BigDecimal;" | "Ljava/math/BigInteger;" => {
            Some(&["Ljava/lang/Comparable;", "Ljava/io/Serializable;"])
        }
        "Ljava/lang/Enum;" => Some(&["Ljava/lang/Comparable;", "Ljava/io/Serializable;"]),
        "Ljava/lang/Thread;" => Some(&["Ljava/lang/Runnable;"]),
        "Landroid/animation/Animator;"
        | "Landroid/animation/PropertyValuesHolder;"
        | "Landroid/animation/Keyframe;"
        | "Landroid/animation/Keyframe$ObjectKeyframe;"
        | "Landroid/animation/Keyframe$IntKeyframe;"
        | "Landroid/animation/Keyframe$FloatKeyframe;" => Some(&["Ljava/lang/Cloneable;"]),
        "Landroid/animation/ValueAnimator;" => Some(&[
            "Ljava/lang/Cloneable;",
            "Landroid/animation/AnimationHandler$AnimationFrameCallback;",
        ]),
        "Landroid/animation/ObjectAnimator;" => Some(&[
            "Ljava/lang/Cloneable;",
            "Landroid/animation/AnimationHandler$AnimationFrameCallback;",
        ]),
        "Landroid/animation/IntEvaluator;"
        | "Landroid/animation/FloatEvaluator;"
        | "Landroid/animation/ArgbEvaluator;" => Some(&["Landroid/animation/TypeEvaluator;"]),
        "Landroid/animation/BidirectionalTypeConverter;" => {
            Some(&["Landroid/animation/TypeConverter;"])
        }
        "Landroid/animation/AnimatorListenerAdapter;" => Some(&[
            "Landroid/animation/Animator$AnimatorListener;",
            "Landroid/animation/Animator$AnimatorPauseListener;",
        ]),
        // Parcelable value types in the bounded Camera2/TextClassifier slices.
        "Landroid/hardware/camera2/impl/CameraMetadataNative;"
        | "Landroid/hardware/camera2/CaptureRequest;"
        | "Landroid/view/textclassifier/TextClassifier$EntityConfig;"
        | "Landroid/view/textclassifier/TextClassification;"
        | "Landroid/view/textclassifier/TextClassification$Request;"
        | "Landroid/view/textclassifier/TextSelection;"
        | "Landroid/view/textclassifier/TextSelection$Request;"
        | "Landroid/view/textclassifier/TextLanguage;"
        | "Landroid/view/textclassifier/TextLanguage$Request;"
        | "Landroid/view/textclassifier/TextLinks;"
        | "Landroid/view/textclassifier/TextLinks$TextLink;"
        | "Landroid/view/textclassifier/TextLinks$Request;"
        | "Landroid/view/textclassifier/ConversationActions;"
        | "Landroid/view/textclassifier/ConversationAction;"
        | "Landroid/view/textclassifier/ConversationActions$Message;"
        | "Landroid/view/textclassifier/ConversationActions$Request;"
        | "Landroid/view/textclassifier/EntityConfidence;" => Some(&["Landroid/os/Parcelable;"]),
        "Landroid/icu/util/ULocale;" => Some(&[
            "Ljava/lang/Cloneable;",
            "Ljava/io/Serializable;",
            "Ljava/lang/Comparable;",
        ]),
        "Landroid/view/autofill/AutofillId;"
        | "Landroid/view/autofill/AutofillValue;"
        | "Landroid/service/autofill/Dataset;"
        | "Landroid/service/autofill/Dataset$DatasetFieldFilter;"
        | "Landroid/service/autofill/InlinePresentation;"
        | "Landroid/service/autofill/FillResponse;"
        | "Landroid/service/autofill/SaveInfo;"
        | "Landroid/service/autofill/FillRequest;"
        | "Landroid/service/autofill/SaveRequest;"
        | "Landroid/service/autofill/FillContext;"
        | "Landroid/service/autofill/CustomDescription;"
        | "Landroid/app/assist/AssistStructure;"
        | "Landroid/app/slice/Slice;"
        | "Landroid/widget/inline/InlinePresentationSpec;" => Some(&["Landroid/os/Parcelable;"]),
        "Landroid/service/autofill/UserData;" => Some(&[
            "Landroid/os/Parcelable;",
            "Landroid/service/autofill/FieldClassificationUserData;",
        ]),
        "Landroid/service/autofill/InternalValidator;"
        | "Landroid/service/autofill/RegexValidator;"
        | "Landroid/service/autofill/RequiredValidators;"
        | "Landroid/service/autofill/OptionalValidators;"
        | "Landroid/service/autofill/NegationValidator;" => Some(&[
            "Landroid/service/autofill/Validator;",
            "Landroid/os/Parcelable;",
        ]),
        "Landroid/service/autofill/InternalSanitizer;"
        | "Landroid/service/autofill/TextValueSanitizer;"
        | "Landroid/service/autofill/DateValueSanitizer;" => Some(&[
            "Landroid/service/autofill/Sanitizer;",
            "Landroid/os/Parcelable;",
        ]),
        "Landroid/transition/Transition;" => Some(&["Ljava/lang/Cloneable;"]),
        "Landroid/transition/TransitionListenerAdapter;"
        | "Landroid/transition/Fade$1;"
        | "Landroid/transition/TransitionSet$TransitionSetListener;"
        | "Landroid/transition/TransitionSet$1;"
        | "Landroid/transition/TransitionManager$1;"
        | "Landroid/transition/Visibility$1;" => {
            Some(&["Landroid/transition/Transition$TransitionListener;"])
        }
        "Landroid/transition/Visibility$DisappearListener;" => Some(&[
            "Landroid/transition/Transition$TransitionListener;",
            "Landroid/animation/Animator$AnimatorListener;",
            "Landroid/animation/Animator$AnimatorPauseListener;",
        ]),
        "Landroid/transition/TransitionManager$MultiListener;" => Some(&[
            "Landroid/view/ViewTreeObserver$OnPreDrawListener;",
            "Landroid/view/View$OnAttachStateChangeListener;",
        ]),
        "Landroid/transition/ChangeBounds$1;" => {
            Some(&["Landroid/animation/ValueAnimator$AnimatorUpdateListener;"])
        }
        "Landroid/system/StructTimespec;" => Some(&["Ljava/lang/Comparable;"]),
        // Parcelable value types in the Batch 57-A bounded slices.
        "Landroid/view/textservice/TextInfo;"
        | "Landroid/view/textservice/SuggestionsInfo;"
        | "Landroid/view/textservice/SentenceSuggestionsInfo;"
        | "Landroid/view/textservice/SpellCheckerSubtype;"
        | "Landroid/view/textservice/SpellCheckerInfo;"
        | "Landroid/view/translation/TranslationSpec;"
        | "Landroid/view/translation/TranslationRequestValue;"
        | "Landroid/view/translation/ViewTranslationRequest;"
        | "Landroid/view/translation/TranslationContext;"
        | "Landroid/view/translation/TranslationCapability;"
        | "Landroid/hardware/usb/UsbDevice;"
        | "Landroid/hardware/usb/UsbConfiguration;"
        | "Landroid/hardware/usb/UsbInterface;"
        | "Landroid/hardware/usb/UsbEndpoint;"
        | "Landroid/hardware/usb/UsbAccessory;"
        | "Landroid/telephony/euicc/EuiccInfo;"
        | "Landroid/app/usage/UsageStats;"
        | "Landroid/app/usage/UsageEvents;"
        | "Landroid/print/PrintAttributes;"
        | "Landroid/print/PageRange;"
        | "Landroid/print/PrinterId;"
        | "Landroid/print/PrintJobId;" => Some(&["Landroid/os/Parcelable;"]),
        "Landroid/security/keystore/KeyGenParameterSpec;" => Some(&[
            "Ljava/security/spec/AlgorithmParameterSpec;",
            "Landroid/security/keystore/UserAuthArgs;",
        ]),
        "Landroid/security/keystore/KeyProtection;" => Some(&[
            "Ljava/security/KeyStore$ProtectionParameter;",
            "Landroid/security/keystore/UserAuthArgs;",
        ]),
        "Landroid/security/keystore/KeyInfo;" => Some(&["Ljava/security/spec/KeySpec;"]),
        // Interfaces in the Batch 49-A bounded slices.
        "Landroid/hardware/fingerprint/FingerprintManager;" => Some(&[
            "Landroid/hardware/biometrics/BiometricAuthenticator;",
            "Landroid/hardware/biometrics/BiometricFingerprintConstants;",
        ]),
        "Landroid/hardware/biometrics/BiometricPrompt;" => Some(&[
            "Landroid/hardware/biometrics/BiometricAuthenticator;",
            "Landroid/hardware/biometrics/BiometricConstants;",
        ]),
        "Landroid/hardware/biometrics/BiometricAuthenticator$Identifier;"
        | "Landroid/hardware/fingerprint/Fingerprint;"
        | "Landroid/hardware/biometrics/PromptInfo;"
        | "Landroid/hardware/hdmi/HdmiDeviceInfo;"
        | "Landroid/hardware/hdmi/HdmiPortInfo;"
        | "Landroid/hardware/input/HostUsiVersion;"
        | "Landroid/hardware/input/InputDeviceIdentifier;"
        | "Landroid/view/InputDevice;"
        | "Landroid/hardware/soundtrigger/SoundTrigger$Keyphrase;"
        | "Landroid/hardware/soundtrigger/SoundTrigger$GenericSoundModel;"
        | "Landroid/hardware/soundtrigger/SoundTrigger$KeyphraseSoundModel;"
        | "Landroid/hardware/soundtrigger/SoundTrigger$ModelParamRange;"
        | "Landroid/hardware/soundtrigger/SoundTrigger$RecognitionConfig;"
        | "Landroid/hardware/soundtrigger/SoundTrigger$ConfidenceLevel;"
        | "Landroid/hardware/soundtrigger/SoundTrigger$KeyphraseRecognitionExtra;"
        | "Landroid/hardware/soundtrigger/KeyphraseMetadata;"
        | "Landroid/media/midi/MidiDeviceInfo;"
        | "Landroid/media/tv/TvTrackInfo;"
        | "Landroid/companion/AssociationRequest;"
        | "Landroid/companion/AssociationInfo;"
        | "Landroid/net/MacAddress;" => Some(&["Landroid/os/Parcelable;"]),
        "Landroid/companion/DeviceFilter;" => Some(&["Landroid/os/Parcelable;"]),
        "Landroid/companion/BluetoothDeviceFilter;"
        | "Landroid/companion/BluetoothLeDeviceFilter;"
        | "Landroid/companion/WifiDeviceFilter;" => Some(&[
            "Landroid/companion/DeviceFilter;",
            "Landroid/os/Parcelable;",
        ]),
        "Landroid/companion/AssociationInfo$Builder;" => {
            Some(&["Landroid/companion/AssociationInfo$NonActionableBuilder;"])
        }
        // Interfaces in the Batch 41-A bounded slices.
        "Landroid/service/quicksettings/Tile;"
        | "Landroid/service/chooser/ChooserTarget;"
        | "Landroid/service/carrier/CarrierIdentifier;" => Some(&["Landroid/os/Parcelable;"]),
        "Lcom/android/internal/view/BaseSurfaceHolder;"
        | "Landroid/service/wallpaper/WallpaperService$Engine$1;" => {
            Some(&["Landroid/view/SurfaceHolder;"])
        }
        "Landroid/service/dreams/DreamService;" => Some(&["Landroid/view/Window$Callback;"]),
        "Landroid/service/chooser/ChooserTargetService$IChooserTargetServiceWrapper;"
        | "Landroid/service/chooser/IChooserTargetService$Stub;" => Some(&[
            "Landroid/service/chooser/IChooserTargetService;",
            "Landroid/os/IBinder;",
            "Landroid/os/IInterface;",
        ]),
        "Landroid/service/carrier/CarrierService$ICarrierServiceWrapper;"
        | "Landroid/service/carrier/ICarrierService$Stub;" => Some(&[
            "Landroid/service/carrier/ICarrierService;",
            "Landroid/os/IBinder;",
            "Landroid/os/IInterface;",
        ]),
        "Landroid/service/media/MediaBrowserService$ServiceBinder;"
        | "Landroid/service/media/IMediaBrowserService$Stub;" => Some(&[
            "Landroid/service/media/IMediaBrowserService;",
            "Landroid/os/IBinder;",
            "Landroid/os/IInterface;",
        ]),
        // Interfaces in the Batch 35-A bounded slices.
        "Landroid/speech/RecognitionSupport;"
        | "Landroid/speech/RecognitionPart;"
        | "Landroid/speech/AlternativeSpan;" => Some(&["Landroid/os/Parcelable;"]),
        "Landroid/speech/RecognitionService$RecognitionServiceBinder;"
        | "Landroid/speech/IRecognitionService$Stub;" => Some(&[
            "Landroid/speech/IRecognitionService;",
            "Landroid/os/IBinder;",
            "Landroid/os/IInterface;",
        ]),
        "Landroid/service/textservice/SpellCheckerService$InternalISpellCheckerSession;"
        | "Lcom/android/internal/textservice/ISpellCheckerSession$Stub;" => Some(&[
            "Lcom/android/internal/textservice/ISpellCheckerSession;",
            "Landroid/os/IBinder;",
            "Landroid/os/IInterface;",
        ]),
        "Landroid/service/textservice/SpellCheckerService$SpellCheckerServiceBinder;"
        | "Lcom/android/internal/textservice/ISpellCheckerService$Stub;" => Some(&[
            "Lcom/android/internal/textservice/ISpellCheckerService;",
            "Landroid/os/IBinder;",
            "Landroid/os/IInterface;",
        ]),
        // Interfaces in the Batch 35-B bounded slices.
        "Landroid/speech/tts/Voice;"
        | "Landroid/speech/tts/TextToSpeech$EngineInfo;"
        | "Landroid/service/voice/HotwordDetectedResult;"
        | "Landroid/service/voice/HotwordRejectedResult;" => Some(&["Landroid/os/Parcelable;"]),
        "Landroid/service/voice/VoiceInteractionSession;" => Some(&[
            "Landroid/view/KeyEvent$Callback;",
            "Landroid/content/ComponentCallbacks2;",
            "Landroid/content/ComponentCallbacks;",
        ]),
        "Landroid/speech/tts/ITextToSpeechService$Stub;" => Some(&[
            "Landroid/speech/tts/ITextToSpeechService;",
            "Landroid/os/IBinder;",
            "Landroid/os/IInterface;",
        ]),
        "Landroid/service/voice/IVoiceInteractionService$Stub;" => Some(&[
            "Landroid/service/voice/IVoiceInteractionService;",
            "Landroid/os/IBinder;",
            "Landroid/os/IInterface;",
        ]),
        // Interfaces in the Batch 35-C bounded slices.
        "Landroid/location/Location;"
        | "Landroid/location/Criteria;"
        | "Landroid/app/slice/SliceItem;"
        | "Landroid/app/slice/SliceSpec;" => Some(&["Landroid/os/Parcelable;"]),
        // Interfaces in the Batch 35-D bounded slices.
        "Landroid/app/backup/BackupAgent$BackupServiceBinder;"
        | "Landroid/app/IBackupAgent$Stub;" => Some(&[
            "Landroid/app/IBackupAgent;",
            "Landroid/os/IBinder;",
            "Landroid/os/IInterface;",
        ]),
        // Interfaces in the Batch 35-E bounded slices.
        "Landroid/se/omapi/SEService$1;" => Some(&["Landroid/content/ServiceConnection;"]),
        "Landroid/se/omapi/SEService$SEListener;"
        | "Landroid/se/omapi/ISecureElementListener$Stub;" => Some(&[
            "Landroid/se/omapi/ISecureElementListener;",
            "Landroid/os/IBinder;",
            "Landroid/os/IInterface;",
        ]),
        // Interfaces in the Batch 36-C bounded slices.
        "Landroid/net/nsd/NsdServiceInfo;" => Some(&["Landroid/os/Parcelable;"]),
        // Interfaces in the Batch 36-D bounded slices.
        "Landroid/content/AttributionSource;"
        | "Landroid/bluetooth/BluetoothDevice;"
        | "Landroid/bluetooth/le/ScanSettings;"
        | "Landroid/bluetooth/le/ScanFilter;"
        | "Landroid/nfc/Tag;"
        | "Landroid/nfc/cardemulation/AidGroup;" => Some(&["Landroid/os/Parcelable;"]),
        "Landroid/nfc/tech/BasicTagTechnology;" | "Landroid/nfc/tech/NfcA;" => Some(&[
            "Landroid/nfc/tech/TagTechnology;",
            "Ljava/io/Closeable;",
            "Ljava/lang/AutoCloseable;",
        ]),
        // Interfaces in the Batch 36-E bounded slices.
        "Landroid/media/MediaMetadata;"
        | "Landroid/media/MediaDescription;"
        | "Landroid/media/Rating;"
        | "Landroid/media/session/PlaybackState;"
        | "Landroid/media/session/PlaybackState$CustomAction;"
        | "Landroid/media/session/MediaSession$QueueItem;"
        | "Landroid/media/session/MediaSession$Token;"
        | "Landroid/media/browse/MediaBrowser$MediaItem;"
        | "Landroid/media/projection/MediaProjectionInfo;" => Some(&["Landroid/os/Parcelable;"]),
        "Landroid/media/session/MediaSession$CallbackStub;"
        | "Landroid/media/session/ISessionCallback$Stub;" => Some(&[
            "Landroid/media/session/ISessionCallback;",
            "Landroid/os/IInterface;",
            "Landroid/os/IBinder;",
        ]),
        "Landroid/media/projection/MediaProjectionManager$CallbackDelegate;"
        | "Landroid/media/projection/IMediaProjectionWatcherCallback$Stub;" => Some(&[
            "Landroid/media/projection/IMediaProjectionWatcherCallback;",
            "Landroid/os/IInterface;",
            "Landroid/os/IBinder;",
        ]),
        // Interfaces in the Batch 36-F bounded slices.
        "Landroid/graphics/pdf/PdfRenderer;" | "Landroid/graphics/pdf/PdfRenderer$Page;" => {
            Some(&["Ljava/lang/AutoCloseable;"])
        }
        // Interfaces in the Batch 36-G bounded slices.
        "Landroid/service/credentials/Action;"
        | "Landroid/service/credentials/BeginCreateCredentialRequest;"
        | "Landroid/service/credentials/BeginCreateCredentialResponse;"
        | "Landroid/service/credentials/BeginGetCredentialOption;"
        | "Landroid/service/credentials/BeginGetCredentialRequest;"
        | "Landroid/service/credentials/BeginGetCredentialResponse;"
        | "Landroid/service/credentials/CallingAppInfo;"
        | "Landroid/service/credentials/ClearCredentialStateRequest;"
        | "Landroid/service/credentials/CreateCredentialRequest;"
        | "Landroid/service/credentials/CreateEntry;"
        | "Landroid/service/credentials/CredentialEntry;"
        | "Landroid/service/credentials/GetCredentialRequest;"
        | "Landroid/service/credentials/RemoteEntry;"
        | "Landroid/credentials/CredentialOption;"
        | "Landroid/app/admin/SystemUpdatePolicy;"
        | "Landroid/app/admin/SystemUpdatePolicy$ValidationFailedException;" => {
            Some(&["Landroid/os/Parcelable;"])
        }
        // Interfaces in the Batch 36-H bounded slices.
        "Landroid/net/wifi/WifiInfo;"
        | "Landroid/net/wifi/ScanResult;"
        | "Landroid/net/wifi/WifiSsid;" => Some(&["Landroid/os/Parcelable;"]),
        // Interfaces in the Batch 36-I bounded slices.
        "Landroid/telephony/ServiceState;"
        | "Landroid/telephony/CellBroadcastIdRange;"
        | "Landroid/telephony/mbms/DownloadRequest;"
        | "Landroid/telephony/mbms/FileInfo;"
        | "Landroid/telecom/PhoneAccountHandle;"
        | "Landroid/telecom/PhoneAccount;"
        | "Landroid/telecom/DisconnectCause;"
        | "Landroid/telecom/CallAudioState;"
        | "Landroid/telecom/VideoProfile;"
        | "Landroid/telecom/VideoProfile$CameraCapabilities;"
        | "Landroid/telecom/StatusHints;"
        | "Landroid/telecom/GatewayInfo;" => Some(&["Landroid/os/Parcelable;"]),
        _ => None,
    }
}

/// Erro de resolução com sugestão (Lei 1 — nunca silencioso).
/// Whether the host runtime models `desc` (a JDK/platform type outside the
/// app DEX).
pub fn host_class_known(desc: &str) -> bool {
    builtin_hierarchy(desc).is_some() || builtin_interfaces(desc).is_some()
}

/// Like [`unresolved_method`], naming the receiver's runtime class when it
/// differs from the declaring class of the method reference.
pub fn unresolved_method_on(class: &str, recv_class: &str, name: &str, proto: &str) -> RdError {
    if class == recv_class {
        return unresolved_method(class, name, proto);
    }
    crate::err::with_suggestion(
        crate::err::vm_error(
            "NOT_FOUND",
            format!("método {class}->{name}{proto} não resolvido (receiver {recv_class})"),
        ),
        "a classe pode depender de plataforma fora do escopo M2",
    )
}

pub fn unresolved_method(class: &str, name: &str, proto: &str) -> RdError {
    crate::err::with_suggestion(
        crate::err::vm_error(
            "NOT_FOUND",
            format!("método {class}->{name}{proto} não resolvido"),
        ),
        "a classe pode depender de plataforma fora do escopo M2",
    )
}

/// Parent-first class loading: a *synthetic* class that the app DEX defines
/// in a platform package is an R8/D8 API stub (its `<clinit>` typically just
/// throws `NoClassDefFoundError`, for devices below the API level that
/// introduced the class). On a device the boot classpath class shadows it,
/// so here the platform (host) class wins too.
fn is_shadowed_platform_stub(descriptor: &str, access_flags: u32) -> bool {
    const ACC_SYNTHETIC: u32 = 0x1000;
    access_flags & ACC_SYNTHETIC != 0
        && ["Landroid/", "Ljava/", "Ljavax/", "Ldalvik/"]
            .iter()
            .any(|p| descriptor.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn legacy_interface_closure(cp: &Classpath, start: &str, sup: &str) -> bool {
        let mut queue = cp.interfaces_of(start);
        let mut visited: std::collections::HashSet<String> = queue.iter().cloned().collect();
        let mut budget = 64;
        while let Some(cur) = queue.pop() {
            if cur == sup {
                return true;
            }
            // host (JDK/platform) interfaces: builtin super-interface closure
            if builtin_interfaces(&cur).is_some_and(|v| v.contains(&sup)) {
                return true;
            }
            budget -= 1;
            if budget == 0 {
                return false;
            }
            for next in cp.interfaces_of(&cur) {
                if visited.insert(next.clone()) {
                    queue.push(next);
                }
            }
        }
        false
    }

    fn legacy_is_subtype(cp: &Classpath, sub: &str, sup: &str) -> bool {
        if sub == sup {
            return true;
        }
        // JLS 4.10: every reference type (incl. unmodelled host classes such
        // as android.content.res.Configuration) is a subtype of Object
        if sup == "Ljava/lang/Object;" && (sub.starts_with('L') || sub.starts_with('[')) {
            return true;
        }
        // issue #42: subtipagem de ARRAYS (JLS 4.10.2/4.10.3) — antes arrays
        // só casavam textualmente (ArrayStoreException espúria em código
        // covariante idiomático, check-cast/instanceof falhos):
        //   · todo array <: Object, Cloneable, Serializable
        //   · [LSub; <: [LSup; ⇔ Sub <: Sup (recursão no elemento)
        //   · [X <: [LObject; ⇔ X é tipo de referência (primitivos invariantes)
        if let Some(sub_elem) = sub.strip_prefix('[') {
            if matches!(
                sup,
                "Ljava/lang/Object;" | "Ljava/lang/Cloneable;" | "Ljava/io/Serializable;"
            ) {
                return true;
            }
            if let Some(sup_elem) = sup.strip_prefix('[') {
                if sup_elem == "Ljava/lang/Object;" {
                    return !matches!(sub_elem, "B" | "C" | "D" | "F" | "I" | "J" | "S" | "Z");
                }
                return legacy_is_subtype(cp, sub_elem, sup_elem);
            }
            return false; // array vs classe não-array (além de Object/…)
        }
        // hierarquia mínima da plataforma conhecida pela VM (classes que não
        // estão no DEX mas fazem parte do contrato do interpretador M2)
        if let Some(supers) = builtin_hierarchy(sub) {
            if supers.contains(&sup) {
                return true;
            }
        }
        if let Some(ifaces) = builtin_interfaces(sub) {
            if ifaces.contains(&sup) {
                return true;
            }
        }
        let mut cur = sub.to_string();
        for _ in 0..16 {
            if cur == sup {
                return true;
            }
            // interfaces declaradas por esta classe (closure transitiva)
            if legacy_interface_closure(cp, &cur, sup) {
                return true;
            }
            let Some(next) = cp.superclass_of(&cur) else {
                return false;
            };
            if next == sup {
                return true;
            }
            if let Some(supers) = builtin_hierarchy(&next) {
                if supers.contains(&sup) {
                    return true;
                }
            }
            if let Some(ifaces) = builtin_interfaces(&next) {
                if ifaces.contains(&sup) {
                    return true;
                }
            }
            cur = next;
        }
        false
    }

    #[test]
    fn borrowed_subtype_walk_matches_legacy_with_cycles_interfaces_and_depth_caps() {
        use rd_testsupport::vm::DexBuilder;
        let mut b = DexBuilder::new();
        let mut app = Vec::new();
        for i in 0..24 {
            let name = format!("LWalk{i};");
            let parent = if i == 0 {
                "Ljava/lang/Object;".into()
            } else {
                format!("LWalk{};", i - 1)
            };
            b.class(&name, &parent);
            app.push(name);
        }
        for name in ["LIface0;", "LIface1;", "LIface2;"] {
            b.class(name, "Ljava/lang/Object;");
            app.push(name.into());
        }
        let mut dex = Dex::parse(b.finish()).unwrap();
        // Explicit interface cycle and diamond, with full bounded type_lists.
        for (owner, ifaces) in [
            ("LWalk5;", vec!["LIface0;", "LIface2;"]),
            ("LIface0;", vec!["LIface1;"]),
            ("LIface1;", vec!["LIface2;"]),
            ("LIface2;", vec!["LIface0;"]),
        ] {
            let types: Vec<_> = ifaces
                .iter()
                .map(|iface| {
                    dex.class_defs
                        .iter()
                        .find(|def| dex.type_str(def.class_idx) == *iface)
                        .unwrap()
                        .class_idx
                })
                .collect();
            let offset = dex.data.len() as u32;
            dex.data
                .extend_from_slice(&(types.len() as u32).to_le_bytes());
            for id in types {
                dex.data.extend_from_slice(&(id as u16).to_le_bytes());
            }
            let class = dex
                .class_defs
                .iter()
                .position(|def| dex.type_str(def.class_idx) == owner)
                .unwrap();
            dex.class_defs[class].interfaces_off = offset;
        }
        let mut cp = Classpath::new(vec![dex]);
        let mut types = app;
        types.extend(
            [
                "",
                "?",
                "LMissing;",
                "Ljava/lang/Object;",
                "Ljava/lang/String;",
                "Ljava/lang/CharSequence;",
                "Ljava/io/Serializable;",
                "Ljava/util/List;",
                "Ljava/util/ArrayList;",
                "Ljava/util/Map;",
                "Ljava/util/HashMap;",
                "Landroid/app/Activity;",
                "Landroid/content/Context;",
                "[I",
                "[J",
                "[Ljava/lang/String;",
                "[Ljava/lang/Object;",
                "[[I",
            ]
            .map(str::to_string),
        );
        for pass in 0..3 {
            if pass == 1 {
                // In-place metadata edits must be visible: no stale subtype cache.
                let a = cp.dexes[0]
                    .class_defs
                    .iter()
                    .position(|def| cp.dexes[0].type_str(def.class_idx) == "LWalk7;")
                    .unwrap();
                let b = cp.dexes[0]
                    .class_defs
                    .iter()
                    .position(|def| cp.dexes[0].type_str(def.class_idx) == "LWalk8;")
                    .unwrap();
                cp.dexes[0].class_defs[a].superclass_idx = cp.dexes[0].class_defs[b].class_idx;
            } else if pass == 2 {
                // Malformed interface data must keep the old tolerant fallback.
                cp.dexes[0].class_defs[0].interfaces_off = u32::MAX;
                cp.dexes[0].class_defs[0].superclass_idx = u32::MAX;
            }
            for sub in &types {
                for sup in &types {
                    assert_eq!(
                        cp.is_subtype(sub, sup),
                        legacy_is_subtype(&cp, sub, sup),
                        "pass={pass} {sub} <: {sup}"
                    );
                }
            }
        }
    }

    #[test]
    fn borrowed_method_name_matches_owned_predicate_including_invalid_ids() {
        use rd_testsupport::vm::DexBuilder;
        let mut b = DexBuilder::new();
        let c = b.class("LBorrowed;", "Ljava/lang/Object;");
        b.direct(c, "pick", "V", vec![], 0x9, None);
        b.direct(c, "pick", "I", vec!["I"], 0x9, None);
        b.direct(c, "ω", "Ljava/lang/String;", vec!["[I"], 0x9, None);
        let mut cp = Classpath::new(vec![Dex::parse(b.finish()).unwrap()]);
        let count = cp.dexes[0].method_ids.len() as u32;
        for pass in 0..2 {
            if pass == 1 {
                cp.dexes[0].method_ids[0].name_idx = u32::MAX;
            }
            for index in (0..count + 2).chain([rd_dex::strings::NO_INDEX]) {
                let owned = cp.dexes[0]
                    .method(index)
                    .ok()
                    .map(|m| cp.dexes[0].string(m.name_idx).to_string());
                for name in ["pick", "ω", "", "missing"] {
                    assert_eq!(
                        cp.method_name(0, index) == Some(name),
                        owned == Some(name.to_string())
                    );
                }
            }
            for name in ["pick", "ω", "", "missing"] {
                for proto in ["()V", "(I)I", "([I)Ljava/lang/String;", "(I)V", "?"] {
                    let (dex_idx, def) = cp.find_class("LBorrowed;").unwrap();
                    let def = *def;
                    let data = cp.class_data(dex_idx, &def).unwrap().unwrap();
                    let expected = data
                        .direct_methods
                        .iter()
                        .chain(&data.virtual_methods)
                        .find(|m| {
                            m.method_idx != rd_dex::strings::NO_INDEX
                                && cp.dexes[0]
                                    .method(m.method_idx)
                                    .ok()
                                    .map(|id| cp.dexes[0].string(id.name_idx).to_string())
                                    == Some(name.to_string())
                                && cp.method_proto(dex_idx, m.method_idx).as_deref() == Some(proto)
                        })
                        .map(|m| m.method_idx);
                    let actual = cp
                        .find_own_method("LBorrowed;", name, proto)
                        .map(|(_, _, m)| m.method_idx);
                    assert_eq!(actual, expected, "name={name} proto={proto}");
                }
            }
        }
        assert_eq!(cp.method_name(1, 0), None);
    }

    /// M8: hierarquia das classes host do compat real — check-cast/instanceof
    /// para interfaces de coleção (sexto gap do org.primftpd: COWSet → Set).
    #[test]
    fn host_collections_subtype_their_interfaces() {
        let cp = Classpath::new(Vec::new());
        // Kotlin EmptyIterator implements ListIterator and is cast to Iterator.
        assert!(cp.is_subtype("Ljava/util/ListIterator;", "Ljava/util/Iterator;"));
        // Set concorrente <: Set <: Collection <: Iterable <: Object
        assert!(cp.is_subtype(
            "Ljava/util/concurrent/CopyOnWriteArraySet;",
            "Ljava/util/Set;"
        ));
        assert!(cp.is_subtype(
            "Ljava/util/concurrent/CopyOnWriteArraySet;",
            "Ljava/util/Collection;"
        ));
        assert!(cp.is_subtype(
            "Ljava/util/concurrent/CopyOnWriteArraySet;",
            "Ljava/lang/Iterable;"
        ));
        assert!(cp.is_subtype(
            "Ljava/util/concurrent/CopyOnWriteArrayList;",
            "Ljava/util/List;"
        ));
        // Logback's Joran interpreter stores nested configuration actions in
        // Vector; it must satisfy the List check-cast used by its DEX code.
        assert!(cp.is_subtype("Ljava/util/Vector;", "Ljava/util/List;"));
        assert!(cp.is_subtype("Ljava/util/Vector;", "Ljava/util/RandomAccess;"));
        assert!(cp.is_subtype("Ljava/util/Stack;", "Ljava/util/Vector;"));
        assert!(cp.is_subtype("Ljava/util/Stack;", "Ljava/util/List;"));
        // cadeias de classe
        assert!(cp.is_subtype("Ljava/util/HashMap;", "Ljava/util/AbstractMap;"));
        assert!(cp.is_subtype("Ljava/util/LinkedList;", "Ljava/util/AbstractList;"));
        assert!(cp.is_subtype("Ljava/util/ArrayList;", "Ljava/lang/Object;"));
        // interfaces
        assert!(cp.is_subtype("Ljava/util/HashMap;", "Ljava/util/Map;"));
        assert!(cp.is_subtype("Ljava/util/LinkedList;", "Ljava/util/Deque;"));
        assert!(cp.is_subtype("Ljava/lang/Enum;", "Ljava/lang/Comparable;"));
        assert!(cp.is_subtype("Ljava/lang/ref/WeakReference;", "Ljava/lang/ref/Reference;"));
        // negativo: Map NÃO é List; Set NÃO é Map
        assert!(!cp.is_subtype("Ljava/util/HashMap;", "Ljava/util/List;"));
        assert!(!cp.is_subtype(
            "Ljava/util/concurrent/CopyOnWriteArraySet;",
            "Ljava/util/Map;"
        ));
    }

    #[test]
    fn jutil_calendar_and_timezone_have_jdk_type_relations() {
        let cp = Classpath::new(Vec::new());
        assert!(cp.is_subtype("Ljava/util/GregorianCalendar;", "Ljava/util/Calendar;"));
        assert!(cp.is_subtype("Ljava/util/GregorianCalendar;", "Ljava/io/Serializable;"));
        assert!(cp.is_subtype("Ljava/util/TimeZone;", "Ljava/lang/Cloneable;"));
        assert!(cp.is_subtype("Ljava/util/TimeZone;", "Ljava/io/Serializable;"));
    }

    /// M8 (bug real do app org.primftpd): a cadeia builtin de Activity apontava
    /// para "Ljava/content/Context;" (typo) — check-cast Activity→Context falhava.
    #[test]
    fn android_activity_is_android_context() {
        let cp = Classpath::new(Vec::new());
        assert!(cp.is_subtype("Landroid/app/Activity;", "Landroid/content/Context;"));
        assert!(cp.is_subtype("Landroid/app/Application;", "Landroid/content/Context;"));
        assert!(!cp.is_subtype("Landroid/app/Activity;", "Ljava/content/Context;"));
    }

    #[test]
    fn android_scroll_and_container_widgets_are_views() {
        let cp = Classpath::new(Vec::new());
        for class in [
            "Landroid/widget/HorizontalScrollView;",
            "Landroid/widget/ScrollView;",
            "Landroid/widget/RelativeLayout;",
            "Landroid/widget/TableLayout;",
            "Landroid/widget/ListView;",
            "Landroid/widget/GridView;",
            "Landroid/widget/Spinner;",
            "Landroid/webkit/WebView;",
        ] {
            assert!(cp.is_subtype(class, "Landroid/view/View;"), "{class}");
            assert!(cp.is_subtype(class, "Landroid/view/ViewGroup;"), "{class}");
        }
        assert!(cp.is_subtype(
            "Landroid/widget/HorizontalScrollView;",
            "Landroid/widget/FrameLayout;"
        ));
    }

    /// issue #42: covariância de arrays no is_subtype (JLS 4.10.2/4.10.3).
    #[test]
    fn array_subtyping_is_covariant_for_reference_elements() {
        let cp = Classpath::new(Vec::new());
        // identidade
        assert!(cp.is_subtype("[Ljava/lang/String;", "[Ljava/lang/String;"));
        // elemento via builtin: String <: CharSequence
        assert!(cp.is_subtype("[Ljava/lang/String;", "[Ljava/lang/CharSequence;"));
        // qualquer array de referência <: Object[]
        assert!(cp.is_subtype("[Ljava/lang/String;", "[Ljava/lang/Object;"));
        // array de arrays é referência → [[I <: [Object]
        assert!(cp.is_subtype("[[I", "[Ljava/lang/Object;"));
        // todo array <: Object/Cloneable/Serializable
        assert!(cp.is_subtype("[I", "Ljava/lang/Object;"));
        assert!(cp.is_subtype("[J", "Ljava/lang/Cloneable;"));
        // primitivo invariante: int[] NÃO é Object[] nem long[]
        assert!(!cp.is_subtype("[I", "[Ljava/lang/Object;"));
        assert!(!cp.is_subtype("[I", "[J"));
        // não-relacionado: Integer[] não é CharSequence[]
        assert!(!cp.is_subtype("[Ljava/lang/Integer;", "[Ljava/lang/CharSequence;"));
    }
}
