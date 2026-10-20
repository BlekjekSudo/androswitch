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
                if is_shadowed_platform_stub(descriptor, def.access_flags) {
                    return None;
                }
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
                    self.method_name(dex_idx, m.method_idx)?,
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
pub(crate) fn builtin_hierarchy(class: &str) -> Option<Vec<&'static str>> {
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
        "Ljava/lang/StringBuilder;" | "Ljava/lang/StringBuffer;" => {
            Some(vec!["Ljava/lang/Object;"])
        }
        "Ljava/lang/ArithmeticException;" => Some(RUNTIME.to_vec()),
        "Ljava/lang/NullPointerException;" => Some(RUNTIME.to_vec()),
        "Ljava/lang/ClassCastException;" => Some(RUNTIME.to_vec()),
        "Ljava/lang/ArrayIndexOutOfBoundsException;"
        | "Ljava/lang/StringIndexOutOfBoundsException;" => Some(vec![
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
        | "Ljava/io/UncheckedIOException;" => Some(RUNTIME.to_vec()),
        "Ljava/util/concurrent/CancellationException;" => Some(vec![
            "Ljava/lang/IllegalStateException;",
            "Ljava/lang/RuntimeException;",
            "Ljava/lang/Exception;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/util/regex/PatternSyntaxException;"
        | "Ljava/util/IllegalFormatException;"
        | "Ljava/nio/charset/IllegalCharsetNameException;"
        | "Ljava/nio/charset/UnsupportedCharsetException;" => Some(vec![
            "Ljava/lang/IllegalArgumentException;",
            "Ljava/lang/RuntimeException;",
            "Ljava/lang/Exception;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/util/UnknownFormatConversionException;"
        | "Ljava/util/IllegalFormatConversionException;"
        | "Ljava/util/MissingFormatArgumentException;" => Some(vec![
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
        | "Landroid/util/AndroidException;" => Some(EXC.to_vec()),
        "Ljava/lang/ClassNotFoundException;"
        | "Ljava/lang/InstantiationException;"
        | "Ljava/lang/NoSuchMethodException;"
        | "Ljava/lang/NoSuchFieldException;"
        | "Ljava/lang/IllegalAccessException;"
        | "Ljava/lang/reflect/InvocationTargetException;" => Some(vec![
            "Ljava/lang/ReflectiveOperationException;",
            "Ljava/lang/Exception;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/io/FileNotFoundException;" | "Ljava/io/EOFException;" => Some(vec![
            "Ljava/io/IOException;",
            "Ljava/lang/Exception;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/provider/Settings$SettingNotFoundException;" => Some(vec![
            "Landroid/util/AndroidException;",
            "Ljava/lang/Exception;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/lang/AbstractMethodError;" | "Ljava/lang/IncompatibleClassChangeError;" => {
            Some(vec![
                "Ljava/lang/IncompatibleClassChangeError;",
                "Ljava/lang/LinkageError;",
                "Ljava/lang/Error;",
                "Ljava/lang/Throwable;",
                "Ljava/lang/Object;",
            ])
        }
        "Ljava/lang/NegativeArraySizeException;" => Some(RUNTIME.to_vec()),
        "Ljava/lang/IllegalStateException;" => Some(RUNTIME.to_vec()),
        "Ljava/lang/IllegalArgumentException;" => Some(RUNTIME.to_vec()),
        "Ljava/lang/NumberFormatException;" => Some(RUNTIME.to_vec()),
        "Ljava/lang/ArrayStoreException;" => Some(RUNTIME.to_vec()),
        "Ljava/lang/RuntimeException;" => Some(EXC.to_vec()),
        // android.database exceptions (thrown by the SQLite backend; Room's
        // @Upsert catches SQLiteConstraintException to fall back to UPDATE)
        "Landroid/database/SQLException;" => Some(RUNTIME.to_vec()),
        "Landroid/database/sqlite/SQLiteException;" => {
            let mut v = vec!["Landroid/database/SQLException;"];
            v.extend_from_slice(RUNTIME);
            Some(v)
        }
        "Landroid/database/sqlite/SQLiteConstraintException;"
        | "Landroid/database/sqlite/SQLiteCantOpenDatabaseException;"
        | "Landroid/database/sqlite/SQLiteDiskIOException;"
        | "Landroid/database/sqlite/SQLiteFullException;"
        | "Landroid/database/sqlite/SQLiteDatabaseCorruptException;"
        | "Landroid/database/sqlite/SQLiteDatabaseLockedException;"
        | "Landroid/database/sqlite/SQLiteReadOnlyDatabaseException;"
        | "Landroid/database/sqlite/SQLiteDoneException;"
        | "Landroid/database/sqlite/SQLiteBlobTooBigException;" => {
            let mut v = vec![
                "Landroid/database/sqlite/SQLiteException;",
                "Landroid/database/SQLException;",
            ];
            v.extend_from_slice(RUNTIME);
            Some(v)
        }
        "Ljava/lang/UnsupportedOperationException;" => Some(RUNTIME.to_vec()),
        "Ljava/lang/StackOverflowError;" => Some(ERR.to_vec()),
        "Ljava/lang/OutOfMemoryError;" => Some(ERR.to_vec()),
        // issue #41: hierarquia de ERROS de link/init — sem elas, EIIE/NCDFE
        // não casam nem com catch (Throwable) (não são subtipo de Throwable)
        "Ljava/lang/LinkageError;" => Some(ERR.to_vec()),
        "Ljava/lang/UnsatisfiedLinkError;" | "Ljava/lang/NoSuchMethodError;" => Some(vec![
            "Ljava/lang/LinkageError;",
            "Ljava/lang/Error;",
            "Ljava/lang/Throwable;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/lang/SecurityException;" => Some(RUNTIME.to_vec()),
        "Landroid/app/PendingIntent$CanceledException;" => Some(EXC.to_vec()),
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
        "Ljava/text/BreakIterator;" => Some(OBJECT.to_vec()),
        // android.text layouts (graphics.rs): StaticLayout/BoringLayout/
        // DynamicLayout are Layouts (Compose casts them to Layout)
        "Landroid/text/Layout;" => Some(OBJECT.to_vec()),
        "Landroid/text/StaticLayout;"
        | "Landroid/text/BoringLayout;"
        | "Landroid/text/DynamicLayout;" => {
            Some(vec!["Landroid/text/Layout;", "Ljava/lang/Object;"])
        }
        "Landroid/text/BoringLayout$Metrics;" => Some(vec![
            "Landroid/graphics/Paint$FontMetricsInt;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/content/ContextWrapper;" => Some(CONTEXT.to_vec()),
        "Landroid/view/ContextThemeWrapper;" => Some(vec![
            "Landroid/content/ContextWrapper;",
            "Landroid/content/Context;",
            "Ljava/lang/Object;",
        ]),
        // AOSP: Activity → ContextThemeWrapper → ContextWrapper → Context;
        // Application/Service → ContextWrapper (apps unwrap contexts with
        // `while (c instanceof ContextWrapper)` looking for owners)
        "Landroid/app/Activity;" => Some(vec![
            "Landroid/view/ContextThemeWrapper;",
            "Landroid/content/ContextWrapper;",
            "Landroid/content/Context;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/app/Application;" | "Landroid/app/Service;" => Some(vec![
            "Landroid/content/ContextWrapper;",
            "Landroid/content/Context;",
            "Ljava/lang/Object;",
        ]),
        // the base context ending every ContextWrapper chain
        "Landroid/app/ContextImpl;" => Some(CONTEXT.to_vec()),
        "Landroid/inputmethodservice/InputMethodService;"
        | "Landroid/accessibilityservice/AccessibilityService;"
        | "Landroid/service/notification/NotificationListenerService;" => Some(vec![
            "Landroid/app/Service;",
            "Landroid/content/Context;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/widget/EditText;" => Some(TEXTVIEW.to_vec()),
        "Landroid/appwidget/AppWidgetProvider;" => Some(vec![
            "Landroid/content/BroadcastReceiver;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/view/inputmethod/BaseInputConnection;" => Some(vec![
            "Landroid/view/inputmethod/InputConnection;",
            "Ljava/lang/Object;",
        ]),
        // M8: FragmentManager legacy (29º gap real) — objeto simples do host.
        "Landroid/app/FragmentManager;" => Some(OBJECT.to_vec()),
        // M8: FragmentTransaction legacy (31º gap real) — ops até o commit.
        "Landroid/app/FragmentTransaction;" => Some(OBJECT.to_vec()),
        // M8: Fragment legacy (32º gap real) — super das fragments do app.
        "Landroid/app/Fragment;" => Some(OBJECT.to_vec()),
        "Landroid/content/Intent;" => Some(OBJECT.to_vec()),
        // ── M8 full scope ─────────────────────────────────────────────
        "Landroid/app/Dialog;" => Some(OBJECT.to_vec()),
        "Landroid/app/AlertDialog;" => Some(vec!["Landroid/app/Dialog;", "Ljava/lang/Object;"]),
        "Landroid/app/AlertDialog$Builder;" => Some(OBJECT.to_vec()),
        "Landroid/database/sqlite/SQLiteDatabase;" => Some(OBJECT.to_vec()),
        "Landroid/database/sqlite/SQLiteOpenHelper;" => Some(OBJECT.to_vec()),
        // jtext_format
        "Ljava/text/Format;" | "Ljava/text/DecimalFormatSymbols;" => Some(OBJECT.to_vec()),
        "Ljava/text/NumberFormat;" => Some(vec!["Ljava/text/Format;", "Ljava/lang/Object;"]),
        "Ljava/text/DecimalFormat;" => Some(vec![
            "Ljava/text/NumberFormat;",
            "Ljava/text/Format;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/content/ComponentName;" => Some(OBJECT.to_vec()),
        "Landroid/graphics/drawable/Drawable;" | "Landroid/content/res/ColorStateList;" => {
            Some(OBJECT.to_vec())
        }
        "Landroid/graphics/drawable/ColorDrawable;"
        | "Landroid/graphics/drawable/LayerDrawable;" => Some(vec![
            "Landroid/graphics/drawable/Drawable;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/graphics/drawable/RippleDrawable;" => Some(vec![
            "Landroid/graphics/drawable/LayerDrawable;",
            "Landroid/graphics/drawable/Drawable;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/view/MotionEvent;" => {
            Some(vec!["Landroid/view/InputEvent;", "Ljava/lang/Object;"])
        }
        "Landroid/view/MotionEvent$PointerCoords;"
        | "Landroid/view/MotionEvent$PointerProperties;" => Some(OBJECT.to_vec()),
        "Landroid/graphics/ColorSpace;"
        | "Landroid/graphics/ColorSpace$Named;"
        | "Landroid/graphics/ColorSpace$Model;"
        | "Landroid/graphics/ColorSpace$Rgb$TransferParameters;" => Some(OBJECT.to_vec()),
        "Landroid/graphics/ColorSpace$Rgb;" => {
            Some(vec!["Landroid/graphics/ColorSpace;", "Ljava/lang/Object;"])
        }
        "Landroid/text/SpannableString;" | "Landroid/text/SpannableStringBuilder;" => {
            Some(OBJECT.to_vec())
        }
        "Landroid/content/pm/PackageManager;"
        | "Landroid/view/Display;"
        | "Landroid/content/pm/PackageInfo;"
        | "Landroid/content/pm/PackageItemInfo;" => Some(OBJECT.to_vec()),
        "Landroid/app/ApplicationPackageManager;" => Some(vec![
            "Landroid/content/pm/PackageManager;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/content/pm/ApplicationInfo;" | "Landroid/content/pm/ComponentInfo;" => {
            Some(vec![
                "Landroid/content/pm/PackageItemInfo;",
                "Ljava/lang/Object;",
            ])
        }
        "Landroid/content/pm/ActivityInfo;"
        | "Landroid/content/pm/ServiceInfo;"
        | "Landroid/content/pm/ProviderInfo;" => Some(vec![
            "Landroid/content/pm/ComponentInfo;",
            "Landroid/content/pm/PackageItemInfo;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/nio/charset/Charset;" | "Ljava/util/UUID;" => Some(OBJECT.to_vec()),
        // jio_chars
        "Ljava/io/Writer;" | "Ljava/io/Reader;" => Some(OBJECT.to_vec()),
        "Ljava/io/StringWriter;"
        | "Ljava/io/OutputStreamWriter;"
        | "Ljava/io/BufferedWriter;"
        | "Ljava/io/PrintWriter;" => Some(vec!["Ljava/io/Writer;", "Ljava/lang/Object;"]),
        "Ljava/io/FileWriter;" => Some(vec![
            "Ljava/io/OutputStreamWriter;",
            "Ljava/io/Writer;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/io/StringReader;" | "Ljava/io/InputStreamReader;" | "Ljava/io/BufferedReader;" => {
            Some(vec!["Ljava/io/Reader;", "Ljava/lang/Object;"])
        }
        "Ljava/io/FileReader;" => Some(vec![
            "Ljava/io/InputStreamReader;",
            "Ljava/io/Reader;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/io/ByteArrayOutputStream;" => {
            Some(vec!["Ljava/io/OutputStream;", "Ljava/lang/Object;"])
        }
        "Ljava/io/ByteArrayInputStream;" => {
            Some(vec!["Ljava/io/InputStream;", "Ljava/lang/Object;"])
        }
        // jsqlite: programs, cursors and the cursor driver.
        "Landroid/database/sqlite/SQLiteClosable;"
        | "Landroid/database/sqlite/SQLiteDirectCursorDriver;"
        | "Landroid/database/AbstractCursor;" => Some(OBJECT.to_vec()),
        "Landroid/database/sqlite/SQLiteProgram;" => Some(vec![
            "Landroid/database/sqlite/SQLiteClosable;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/database/sqlite/SQLiteQuery;" | "Landroid/database/sqlite/SQLiteStatement;" => {
            Some(vec![
                "Landroid/database/sqlite/SQLiteProgram;",
                "Landroid/database/sqlite/SQLiteClosable;",
                "Ljava/lang/Object;",
            ])
        }
        "Landroid/database/AbstractWindowedCursor;" => Some(vec![
            "Landroid/database/AbstractCursor;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/database/sqlite/SQLiteCursor;" => Some(vec![
            "Landroid/database/AbstractWindowedCursor;",
            "Landroid/database/AbstractCursor;",
            "Ljava/lang/Object;",
        ]),
        "Landroid/content/ContentValues;" => Some(OBJECT.to_vec()),
        "Landroid/net/Uri;" => Some(OBJECT.to_vec()),
        "Landroid/net/Uri$Builder;" => Some(OBJECT.to_vec()),
        "Landroid/app/job/JobScheduler;" => Some(OBJECT.to_vec()),
        "Landroidx/recyclerview/widget/RecyclerView;" => Some(VIEWGROUP.to_vec()),
        "Landroidx/recyclerview/widget/RecyclerView$Adapter;" => Some(OBJECT.to_vec()),
        "Landroidx/recyclerview/widget/RecyclerView$ViewHolder;" => Some(OBJECT.to_vec()),
        "Landroidx/recyclerview/widget/RecyclerView$LayoutManager;" => Some(OBJECT.to_vec()),
        "Landroidx/recyclerview/widget/LinearLayoutManager;" => Some(vec![
            "Landroidx/recyclerview/widget/RecyclerView$LayoutManager;",
            "Ljava/lang/Object;",
        ]),
        "Landroidx/recyclerview/widget/GridLayoutManager;" => Some(vec![
            "Landroidx/recyclerview/widget/LinearLayoutManager;",
            "Landroidx/recyclerview/widget/RecyclerView$LayoutManager;",
            "Ljava/lang/Object;",
        ]),
        "Landroidx/fragment/app/Fragment;" => Some(OBJECT.to_vec()),
        "Landroidx/fragment/app/FragmentManager;" => Some(OBJECT.to_vec()),
        "Landroidx/fragment/app/FragmentTransaction;" => Some(OBJECT.to_vec()),
        "Lokhttp3/OkHttpClient;" => Some(OBJECT.to_vec()),
        "Lokhttp3/Request;" => Some(OBJECT.to_vec()),
        "Lokhttp3/Request$Builder;" => Some(OBJECT.to_vec()),
        "Lokhttp3/Call;" => Some(OBJECT.to_vec()),
        "Lokhttp3/Response;" => Some(OBJECT.to_vec()),
        "Lokhttp3/ResponseBody;" => Some(OBJECT.to_vec()),
        "Landroidx/work/WorkManager;" => Some(OBJECT.to_vec()),
        "Landroidx/work/Operation;" => Some(OBJECT.to_vec()),
        "Landroidx/work/WorkRequest;" => Some(OBJECT.to_vec()),
        "Landroidx/room/RoomDatabase;" => Some(OBJECT.to_vec()),
        "Landroidx/room/RoomDatabase$Builder;" => Some(OBJECT.to_vec()),
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
        "Ljava/util/AbstractSet;" => {
            Some(vec!["Ljava/util/AbstractCollection;", "Ljava/lang/Object;"])
        }
        "Ljava/util/HashSet;" => Some(vec![
            "Ljava/util/AbstractSet;",
            "Ljava/util/AbstractCollection;",
            "Ljava/lang/Object;",
        ]),
        // M8: LinkedHashSet mantém a cadeia real de HashSet e, no host,
        // preserva ordem de inserção no Vec usado pelo Set.
        "Ljava/util/LinkedHashSet;" => Some(vec![
            "Ljava/util/HashSet;",
            "Ljava/util/AbstractSet;",
            "Ljava/util/AbstractCollection;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/util/ArrayDeque;" => {
            Some(vec!["Ljava/util/AbstractCollection;", "Ljava/lang/Object;"])
        }
        "Ljava/util/PriorityQueue;" => Some(vec![
            "Ljava/util/AbstractQueue;",
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
        // ConcurrentHashMap NÃO estende HashMap na JDK (AbstractMap direto)
        "Ljava/util/concurrent/ConcurrentHashMap;" => {
            Some(vec!["Ljava/util/AbstractMap;", "Ljava/lang/Object;"])
        }
        "Ljava/util/WeakHashMap;" | "Ljava/util/IdentityHashMap;" => {
            Some(vec!["Ljava/util/AbstractMap;", "Ljava/lang/Object;"])
        }
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
        "Ljava/util/concurrent/LinkedBlockingQueue;" => Some(vec![
            "Ljava/util/AbstractQueue;",
            "Ljava/util/AbstractCollection;",
            "Ljava/lang/Object;",
        ]),
        "Ljava/util/AbstractQueue;" => {
            Some(vec!["Ljava/util/AbstractCollection;", "Ljava/lang/Object;"])
        }
        "Ljava/util/concurrent/CopyOnWriteArraySet;" => Some(OBJECT.to_vec()),
        "Ljava/util/concurrent/atomic/AtomicReference;" => Some(OBJECT.to_vec()),
        "Ljava/lang/Number;" => Some(OBJECT.to_vec()),
        // primitive wrappers (boxes.rs): numeric ones extend Number
        "Ljava/lang/Integer;"
        | "Ljava/lang/Long;"
        | "Ljava/lang/Short;"
        | "Ljava/lang/Byte;"
        | "Ljava/lang/Float;"
        | "Ljava/lang/Double;" => Some(vec!["Ljava/lang/Number;", "Ljava/lang/Object;"]),
        "Ljava/lang/Boolean;" | "Ljava/lang/Character;" => Some(OBJECT.to_vec()),
        "Ljava/lang/Thread;" => Some(OBJECT.to_vec()),
        "Ljava/lang/ThreadLocal;" => Some(OBJECT.to_vec()),
        "Ljava/lang/InheritableThreadLocal;" => {
            Some(vec!["Ljava/lang/ThreadLocal;", "Ljava/lang/Object;"])
        }
        "Ljava/lang/Class;" => Some(OBJECT.to_vec()),
        "Ljava/lang/ClassLoader;" => Some(OBJECT.to_vec()),
        // M8: ServiceLoader host é um Iterable vazio enquanto a descoberta
        // `META-INF/services` do APK permanece fora do runtime.
        "Ljava/util/ServiceLoader;" => Some(OBJECT.to_vec()),
        "Ljava/util/concurrent/atomic/AtomicInteger;"
        | "Ljava/util/concurrent/atomic/AtomicLong;" => {
            Some(vec!["Ljava/lang/Number;", "Ljava/lang/Object;"])
        }
        "Ljava/util/concurrent/atomic/AtomicBoolean;" => Some(OBJECT.to_vec()),
        "Ljava/util/concurrent/atomic/AtomicReferenceFieldUpdater;" => Some(OBJECT.to_vec()),
        "Ljava/lang/Enum;" => Some(OBJECT.to_vec()),
        // java.util.Random (jrandom.rs)
        "Ljava/util/Random;" => Some(OBJECT.to_vec()),
        "Ljava/util/concurrent/ThreadLocalRandom;" => {
            Some(vec!["Ljava/util/Random;", "Ljava/lang/Object;"])
        }
        // java.math (jbigdecimal.rs)
        "Ljava/math/BigDecimal;" | "Ljava/math/BigInteger;" => {
            Some(vec!["Ljava/lang/Number;", "Ljava/lang/Object;"])
        }
        "Ljava/math/RoundingMode;" | "Ljava/text/Normalizer$Form;" => {
            Some(vec!["Ljava/lang/Enum;", "Ljava/lang/Object;"])
        }
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
        "Ljava/lang/StringBuilder;" | "Ljava/lang/StringBuffer;" => Some(SB_IFACES.to_vec()),
        "Ljava/util/regex/Pattern;" => Some(vec!["Ljava/io/Serializable;"]),
        "Ljava/util/regex/Matcher;" => Some(vec!["Ljava/util/regex/MatchResult;"]),
        "Ljava/text/Format;"
        | "Ljava/text/NumberFormat;"
        | "Ljava/text/DecimalFormat;"
        | "Ljava/text/DecimalFormatSymbols;" => {
            Some(vec!["Ljava/lang/Cloneable;", "Ljava/io/Serializable;"])
        }
        "Ljava/nio/charset/Charset;" => Some(vec!["Ljava/lang/Comparable;"]),
        "Ljava/util/UUID;" => Some(vec!["Ljava/lang/Comparable;", "Ljava/io/Serializable;"]),
        "Landroid/net/Uri;" => Some(vec!["Landroid/os/Parcelable;", "Ljava/lang/Comparable;"]),
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
            Some(vec!["Landroid/os/Parcelable;", "Ljava/lang/Cloneable;"])
        }
        "Landroid/util/SparseArray;" => Some(vec!["Ljava/lang/Cloneable;"]),
        "Landroid/text/SpannableString;" => Some(vec![
            "Landroid/text/Spannable;",
            "Landroid/text/Spanned;",
            "Ljava/lang/CharSequence;",
            "Landroid/text/GetChars;",
        ]),
        "Landroid/text/SpannableStringBuilder;" => Some(vec![
            "Landroid/text/Editable;",
            "Landroid/text/Spannable;",
            "Landroid/text/Spanned;",
            "Ljava/lang/CharSequence;",
            "Landroid/text/GetChars;",
            "Ljava/lang/Appendable;",
        ]),
        "Landroid/content/ComponentName;" => Some(vec![
            "Landroid/os/Parcelable;",
            "Ljava/lang/Cloneable;",
            "Ljava/lang/Comparable;",
        ]),
        "Ljava/io/Writer;" => Some(vec![
            "Ljava/lang/Appendable;",
            "Ljava/io/Closeable;",
            "Ljava/io/Flushable;",
            "Ljava/lang/AutoCloseable;",
        ]),
        "Ljava/io/Reader;" => Some(vec![
            "Ljava/lang/Readable;",
            "Ljava/io/Closeable;",
            "Ljava/lang/AutoCloseable;",
        ]),
        // jsqlite host classes
        "Landroid/database/sqlite/SQLiteDirectCursorDriver;" => {
            Some(vec!["Landroid/database/sqlite/SQLiteCursorDriver;"])
        }
        "Landroid/database/sqlite/SQLiteClosable;"
        | "Landroid/database/sqlite/SQLiteProgram;"
        | "Landroid/database/sqlite/SQLiteQuery;"
        | "Landroid/database/sqlite/SQLiteStatement;"
        | "Landroid/database/sqlite/SQLiteDatabase;" => {
            Some(vec!["Ljava/io/Closeable;", "Ljava/lang/AutoCloseable;"])
        }
        "Landroid/database/sqlite/SQLiteCursor;"
        | "Landroid/database/AbstractWindowedCursor;"
        | "Landroid/database/AbstractCursor;" => Some(vec![
            "Landroid/database/CrossProcessCursor;",
            "Landroid/database/Cursor;",
            "Ljava/io/Closeable;",
            "Ljava/lang/AutoCloseable;",
        ]),
        // java.util.concurrent.locks host classes (jlocks.rs)
        "Ljava/util/concurrent/locks/ReentrantLock;" => Some(vec![
            "Ljava/util/concurrent/locks/Lock;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/concurrent/locks/ReentrantReadWriteLock;" => Some(vec![
            "Ljava/util/concurrent/locks/ReadWriteLock;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/concurrent/locks/ReentrantReadWriteLock$ReadLock;"
        | "Ljava/util/concurrent/locks/ReentrantReadWriteLock$WriteLock;" => Some(vec![
            "Ljava/util/concurrent/locks/Lock;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/io/File;" => Some(vec!["Ljava/lang/Comparable;", "Ljava/io/Serializable;"]),
        "Ljava/io/FileOutputStream;" | "Ljava/io/FileInputStream;" => {
            Some(vec!["Ljava/io/Closeable;", "Ljava/lang/AutoCloseable;"])
        }
        "Ljava/nio/channels/FileLock;" => Some(vec!["Ljava/lang/AutoCloseable;"]),
        // host XML resource parser (xml_res.rs) is a pull parser + AttributeSet
        "Landroid/content/res/XmlResourceParser;" => Some(vec![
            "Lorg/xmlpull/v1/XmlPullParser;",
            "Landroid/util/AttributeSet;",
            "Ljava/lang/AutoCloseable;",
        ]),
        // ── M8: interfaces das classes host do compat real ──
        "Ljava/util/AbstractCollection;" => Some(COLL.to_vec()),
        // JDK collection interfaces: super-interface closure, so a DEX class
        // implementing Set/List/Queue/… is a Collection/Iterable (check-cast)
        "Ljava/util/Collection;" => Some(vec!["Ljava/lang/Iterable;"]),
        "Ljava/util/Set;" | "Ljava/util/List;" | "Ljava/util/Queue;" => Some(COLL.to_vec()),
        "Ljava/util/Deque;" | "Ljava/util/concurrent/BlockingQueue;" => Some(vec![
            "Ljava/util/Queue;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
        ]),
        "Ljava/util/SortedSet;" => Some(vec![
            "Ljava/util/Set;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
        ]),
        "Ljava/util/NavigableSet;" => Some(vec![
            "Ljava/util/SortedSet;",
            "Ljava/util/Set;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
        ]),
        "Ljava/util/SortedMap;" | "Ljava/util/concurrent/ConcurrentMap;" => {
            Some(vec!["Ljava/util/Map;"])
        }
        "Ljava/util/NavigableMap;" => Some(vec!["Ljava/util/SortedMap;", "Ljava/util/Map;"]),
        "Ljava/util/TreeSet;" => Some(vec![
            "Ljava/util/NavigableSet;",
            "Ljava/util/SortedSet;",
            "Ljava/util/Set;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
            "Ljava/lang/Cloneable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/TreeMap;" => Some(vec![
            "Ljava/util/NavigableMap;",
            "Ljava/util/SortedMap;",
            "Ljava/util/Map;",
            "Ljava/lang/Cloneable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/AbstractList;" | "Ljava/util/AbstractSequentialList;" => {
            Some(LIST_IFACES.to_vec())
        }
        "Ljava/util/AbstractMap;" => Some(MAP_IFACES.to_vec()),
        "Ljava/util/HashMap;" => Some(vec![
            "Ljava/util/Map;",
            "Ljava/lang/Cloneable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/WeakHashMap;"
        | "Ljava/util/IdentityHashMap;"
        | "Ljava/util/LinkedHashMap;"
        | "Ljava/util/concurrent/ConcurrentHashMap;" => Some(vec![
            "Ljava/util/Map;",
            "Ljava/util/concurrent/ConcurrentMap;",
            "Ljava/lang/Cloneable;",
            "Ljava/io/Serializable;",
        ]),
        // Immutable views from java.util.Collections (intrinsics.rs markers).
        "Ljava/util/Collections$UnmodifiableSet;" => Some(vec![
            "Ljava/util/Set;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/Collections$SingletonList;" => Some(vec![
            "Ljava/util/List;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
            "Ljava/util/RandomAccess;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/Collections$UnmodifiableMap;" => {
            Some(vec!["Ljava/util/Map;", "Ljava/io/Serializable;"])
        }
        "Ljava/util/AbstractSet;" | "Ljava/util/HashSet;" | "Ljava/util/LinkedHashSet;" => {
            Some(vec![
                "Ljava/util/Set;",
                "Ljava/util/Collection;",
                "Ljava/lang/Iterable;",
                "Ljava/lang/Cloneable;",
                "Ljava/io/Serializable;",
            ])
        }
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
        "Ljava/util/concurrent/LinkedBlockingQueue;" => Some(vec![
            "Ljava/util/concurrent/BlockingQueue;",
            "Ljava/util/Queue;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/AbstractQueue;" => Some(vec![
            "Ljava/util/Queue;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
        ]),
        "Ljava/util/PriorityQueue;" => Some(vec![
            "Ljava/util/Queue;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
            "Ljava/io/Serializable;",
        ]),
        "Ljava/util/ArrayDeque;" => Some(vec![
            "Ljava/util/Deque;",
            "Ljava/util/Queue;",
            "Ljava/util/Collection;",
            "Ljava/lang/Iterable;",
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
        "Ljava/lang/Integer;"
        | "Ljava/lang/Long;"
        | "Ljava/lang/Short;"
        | "Ljava/lang/Byte;"
        | "Ljava/lang/Float;"
        | "Ljava/lang/Double;"
        | "Ljava/lang/Boolean;"
        | "Ljava/lang/Character;" => Some(vec!["Ljava/lang/Comparable;", "Ljava/io/Serializable;"]),
        "Ljava/util/concurrent/atomic/AtomicReference;"
        | "Ljava/util/concurrent/atomic/AtomicInteger;"
        | "Ljava/util/concurrent/atomic/AtomicLong;"
        | "Ljava/util/concurrent/atomic/AtomicBoolean;" => Some(vec!["Ljava/io/Serializable;"]),
        "Ljava/util/concurrent/ExecutorService;" => Some(vec!["Ljava/util/concurrent/Executor;"]),
        "Ljava/util/ServiceLoader;" => Some(vec!["Ljava/lang/Iterable;"]),
        "Ljava/math/BigDecimal;" | "Ljava/math/BigInteger;" => {
            Some(vec!["Ljava/lang/Comparable;", "Ljava/io/Serializable;"])
        }
        "Ljava/lang/Enum;" => Some(vec!["Ljava/lang/Comparable;", "Ljava/io/Serializable;"]),
        "Ljava/lang/Thread;" => Some(vec!["Ljava/lang/Runnable;"]),
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

    /// M8 (bug real do app org.primftpd): a cadeia builtin de Activity apontava
    /// para "Ljava/content/Context;" (typo) — check-cast Activity→Context falhava.
    #[test]
    fn android_activity_is_android_context() {
        let cp = Classpath::new(Vec::new());
        assert!(cp.is_subtype("Landroid/app/Activity;", "Landroid/content/Context;"));
        assert!(cp.is_subtype("Landroid/app/Application;", "Landroid/content/Context;"));
        assert!(!cp.is_subtype("Landroid/app/Activity;", "Ljava/content/Context;"));
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
