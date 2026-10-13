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

#[derive(Debug, Default)]
pub struct Classpath {
    pub dexes: Vec<Dex>,
    /// (índice do dex, offset do code_item) → parse em cache
    code_cache: HashMap<(usize, usize), Arc<CodeItem>>,
    /// (índice do dex, class_idx) → class_data em cache
    class_data_cache: HashMap<(usize, u32), Option<Arc<ClassData>>>,
}

impl Classpath {
    pub fn new(dexes: Vec<Dex>) -> Self {
        Classpath {
            dexes,
            code_cache: HashMap::new(),
            class_data_cache: HashMap::new(),
        }
    }

    /// method_id cru (class/name/idx).
    pub fn method_ref(&self, dex_idx: usize, idx: u32) -> RdResult<MethodRef> {
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
        Ok(MethodRef {
            class: dex.type_str(m.class_idx as u32).to_string(),
            name: dex.string(m.name_idx).to_string(),
            proto,
            param_types,
            return_type: ret,
        })
    }

    /// field_id cru → (classe, nome, tipo).
    pub fn field_ref(&self, dex_idx: usize, idx: u32) -> RdResult<(String, String, String)> {
        let dex = &self.dexes[dex_idx];
        let f = dex.field(idx)?;
        Ok((
            dex.type_str(f.class_idx as u32).to_string(),
            dex.string(f.name_idx).to_string(),
            dex.type_str(f.type_idx as u32).to_string(),
        ))
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
        for (i, dex) in self.dexes.iter().enumerate() {
            if let Some(def) = dex.find_class(descriptor) {
                return Some((i, def));
            }
        }
        None
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
                    && self.method_name(dex_idx, m.method_idx) == Some(name.to_string())
                    && self.method_proto(dex_idx, m.method_idx).as_deref() == Some(proto)
            })
            .cloned()
            .map(|m| (dex_idx, def, m))
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
                            && self.method_name(dex_idx, m.method_idx) == Some(name.to_string())
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
                        && self.method_name(dex_idx, m.method_idx) == Some(name.to_string())
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

    fn method_name(&self, dex_idx: usize, idx: u32) -> Option<String> {
        let d = self.dexes.get(dex_idx)?;
        let m = d.method(idx).ok()?;
        Some(d.string(m.name_idx).to_string())
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
    fn interface_closure_contains(&self, start: &str, sup: &str) -> bool {
        let mut queue = self.interfaces_of(start);
        let mut visited: std::collections::HashSet<String> = queue.iter().cloned().collect();
        let mut budget = 64;
        while let Some(cur) = queue.pop() {
            if cur == sup {
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
        let mut cur = sub.to_string();
        for _ in 0..16 {
            if cur == sup {
                return true;
            }
            // interfaces declaradas por esta classe (closure transitiva)
            if self.interface_closure_contains(&cur, sup) {
                return true;
            }
            let Some(next) = self.superclass_of(&cur) else {
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

    /// Offset do call_site_item N (para invokedynamic/string concat).
    pub fn call_site_offset(&self, dex_idx: usize, n: usize) -> Option<u32> {
        self.dexes[dex_idx].call_site_offsets.get(n).copied()
    }
}

/// Hierarquia embutida mínima para classes de plataforma que a VM conhece sem
/// DEX (catch/instanceof/ hierarquia de exceções; M3.2: hierarquia Android do
/// framework host — APKs REAIS não definem android.* no DEX, e o LayoutInflater
/// precisa de is_subtype(View/Activity/…) para attach/dispatch corretos).
fn builtin_hierarchy(class: &str) -> Option<Vec<&'static str>> {
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
    const CONTEXT: &[&str] = &["Ljava/content/Context;", "Ljava/lang/Object;"];
    const VIEW: &[&str] = &["Landroid/view/View;", "Ljava/lang/Object;"];
    const VIEWGROUP: &[&str] = &[
        "Landroid/view/ViewGroup;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const TEXTVIEW: &[&str] = &[
        "Landroid/widget/TextView;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    const BUTTON: &[&str] = &[
        "Landroid/widget/Button;",
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
    const FRAMELAYOUT: &[&str] = &[
        "Landroid/widget/FrameLayout;",
        "Landroid/view/ViewGroup;",
        "Landroid/view/View;",
        "Ljava/lang/Object;",
    ];
    match class {
        "Ljava/lang/String;" => Some(vec!["Ljava/lang/Object;"]),
        "Ljava/lang/StringBuilder;" => Some(vec!["Ljava/lang/Object;"]),
        "Ljava/lang/ArithmeticException;" => Some(RUNTIME.to_vec()),
        "Ljava/lang/NullPointerException;" => Some(RUNTIME.to_vec()),
        "Ljava/lang/ClassCastException;" => Some(RUNTIME.to_vec()),
        "Ljava/lang/ArrayIndexOutOfBoundsException;" => Some(RUNTIME.to_vec()),
        "Ljava/lang/NegativeArraySizeException;" => Some(RUNTIME.to_vec()),
        "Ljava/lang/IllegalStateException;" => Some(RUNTIME.to_vec()),
        "Ljava/lang/IllegalArgumentException;" => Some(RUNTIME.to_vec()),
        "Ljava/lang/NumberFormatException;" => Some(RUNTIME.to_vec()),
        "Ljava/lang/ArrayStoreException;" => Some(RUNTIME.to_vec()),
        "Ljava/lang/StringIndexOutOfBoundsException;" => Some(RUNTIME.to_vec()),
        "Ljava/lang/RuntimeException;" => Some(EXC.to_vec()),
        "Ljava/lang/UnsupportedOperationException;" => Some(RUNTIME.to_vec()),
        "Ljava/lang/StackOverflowError;" => Some(ERR.to_vec()),
        "Ljava/lang/OutOfMemoryError;" => Some(ERR.to_vec()),
        // issue #41: hierarquia de ERROS de link/init — sem elas, EIIE/NCDFE
        // não casam nem com catch (Throwable) (não são subtipo de Throwable)
        "Ljava/lang/LinkageError;" => Some(ERR.to_vec()),
        "Ljava/lang/ExceptionInInitializerError;" => Some(vec![
            "Ljava/lang/LinkageError;",
            "Ljava/lang/Error;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/lang/NoClassDefFoundError;" => Some(vec![
            "Ljava/lang/LinkageError;",
            "Ljava/lang/Error;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        // issue #57: campo estático inexistente na cadeia (JVMS 5.4.3.2) —
        // capturável como LinkageError/Error/Throwable, mesma família
        "Ljava/lang/NoSuchFieldError;" => Some(vec![
            "Ljava/lang/LinkageError;",
            "Ljava/lang/Error;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/lang/Throwable;" => Some(vec!["Ljava/lang/Object;"]),
        "Ljava/lang/Exception;" => Some(EXC.to_vec()),
        "Ljava/lang/Error;" => Some(ERR.to_vec()),
        // ── M3.2: framework host Android ───────────────────────────────────
        "Landroid/content/Context;" => Some(OBJECT.to_vec()),
        "Landroid/app/Activity;" => Some(CONTEXT.to_vec()),
        "Landroid/app/Application;" => Some(CONTEXT.to_vec()),
        "Landroid/content/Intent;" => Some(OBJECT.to_vec()),
        "Landroid/os/Bundle;" => Some(OBJECT.to_vec()),
        "Landroid/os/Handler;" => Some(OBJECT.to_vec()),
        "Landroid/os/Looper;" => Some(OBJECT.to_vec()),
        "Landroid/view/Window;" => Some(OBJECT.to_vec()),
        "Landroid/view/View;" => Some(VIEW.to_vec()),
        "Landroid/view/ViewGroup;" => Some(VIEWGROUP.to_vec()),
        "Landroid/widget/TextView;" => Some(TEXTVIEW.to_vec()),
        "Landroid/widget/Button;" => Some(BUTTON.to_vec()),
        "Landroid/widget/LinearLayout;" => Some(LINEARLAYOUT.to_vec()),
        "Landroid/widget/FrameLayout;" => Some(FRAMELAYOUT.to_vec()),
        // ── M8: classes host do compat real (cadeia completa de ancestrais) ──
        "Ljava/util/AbstractCollection;" => Some(OBJECT.to_vec()),
        "Ljava/util/AbstractList;" => {
            Some(vec!["Ljava/util/AbstractCollection;", "Ljava/lang/Object;"])
        }
        "Ljava/util/AbstractSequentialList;" => Some(vec![
            "Ljava/util/AbstractList;",
            "Ljava/util/AbstractCollection;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/util/AbstractMap;" => Some(OBJECT.to_vec()),
        "Ljava/util/HashMap;" => Some(vec!["Ljava/util/AbstractMap;", "Ljava/lang/Object;"]),
        "Ljava/util/LinkedHashMap;" => Some(vec![
            "Ljava/util/HashMap;",
            "Ljava/util/AbstractMap;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/util/WeakHashMap;" => Some(vec!["Ljava/util/AbstractMap;", "Ljava/lang/Object;"]),
        "Ljava/util/ArrayList;" => Some(vec![
            "Ljava/util/AbstractList;",
            "Ljava/util/AbstractCollection;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/util/LinkedList;" => Some(vec![
            "Ljava/util/AbstractSequentialList;",
            "Ljava/util/AbstractList;",
            "Ljava/util/AbstractCollection;",
            "Ljava/lang/Object;",
        ]),
        // CopyOnWrite*: na JDK real NÃO estendem Abstract* (implementam direto)
        "Ljava/util/concurrent/CopyOnWriteArrayList;" => Some(OBJECT.to_vec()),
        "Ljava/util/concurrent/CopyOnWriteArraySet;" => Some(OBJECT.to_vec()),
        "Ljava/util/concurrent/atomic/AtomicReference;" => Some(OBJECT.to_vec()),
        "Ljava/util/concurrent/atomic/AtomicReferenceFieldUpdater;" => Some(OBJECT.to_vec()),
        "Ljava/lang/Enum;" => Some(OBJECT.to_vec()),
        "Ljava/lang/ref/Reference;" => Some(OBJECT.to_vec()),
        "Ljava/lang/ref/WeakReference;" => {
            Some(vec!["Ljava/lang/ref/Reference;", "Ljava/lang/Object;"])
        }
        "Ljava/lang/ref/SoftReference;" => {
            Some(vec!["Ljava/lang/ref/Reference;", "Ljava/lang/Object;"])
        }
        "Ljava/lang/ref/ReferenceQueue;" => Some(OBJECT.to_vec()),
        _ => None,
    }
}

/// Interfaces de plataforma conhecidas sem DEX (issue #26: check-cast /
/// instanceof para CharSequence/Comparable/… sobre String/StringBuilder).
fn builtin_interfaces(class: &str) -> Option<Vec<&'static str>> {
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
    const MAP_IFACES: &[&str] = &["Ljava/util/Map;"];
    match class {
        "Ljava/lang/String;" => Some(STRING_IFACES.to_vec()),
        "Ljava/lang/StringBuilder;" => Some(SB_IFACES.to_vec()),
        // ── M8: interfaces das classes host do compat real ──
        "Ljava/util/AbstractCollection;" => Some(COLL.to_vec()),
        "Ljava/util/AbstractList;" | "Ljava/util/AbstractSequentialList;" => {
            Some(LIST_IFACES.to_vec())
        }
        "Ljava/util/AbstractMap;" => Some(MAP_IFACES.to_vec()),
        "Ljava/util/HashMap;" => Some(vec![
            "Ljava/util/Map;",
            "Ljava/lang/Cloneable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/WeakHashMap;" | "Ljava/util/LinkedHashMap;" => Some(MAP_IFACES.to_vec()),
        "Ljava/util/ArrayList;" => Some(vec![
            "Ljava/util/List;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
            "Ljava/util/RandomAccess;",
            "Ljava/lang/Cloneable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/LinkedList;" => Some(vec![
            "Ljava/util/List;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
            "Ljava/util/Deque;",
            "Ljava/util/Queue;",
            "Ljava/lang/Cloneable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/concurrent/CopyOnWriteArrayList;" => Some(vec![
            "Ljava/util/List;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
            "Ljava/util/RandomAccess;",
            "Ljava/lang/Cloneable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/concurrent/CopyOnWriteArraySet;" => Some(vec![
            "Ljava/util/Set;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
            "Ljava/lang/Cloneable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/concurrent/atomic/AtomicReference;" => Some(vec!["Ljava/io/Serializable;"]),
        "Ljava/lang/Enum;" => Some(vec!["Ljava/lang/Comparable;", "Ljava/io/Serializable;"]),
        _ => None,
    }
}

/// Erro de resolução com sugestão (Lei 1 — nunca silencioso).
pub fn unresolved_method(class: &str, name: &str, proto: &str) -> RdError {
    crate::err::with_suggestion(
        crate::err::vm_error(
            "NOT_FOUND",
            format!("método {class}->{name}{proto} não resolvido"),
        ),
        "a classe pode depender de plataforma fora do escopo M2",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// M8: hierarquia das classes host do compat real — check-cast/instanceof
    /// para interfaces de coleção (sexto gap do org.primftpd: COWSet → Set).
    #[test]
    fn host_collections_subtype_their_interfaces() {
        let cp = Classpath::new(Vec::new());
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
