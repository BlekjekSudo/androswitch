//! M3.2 — LayoutInflater headless: layout AXML real do APK → árvore de views
//! host (`rd-vm::framework`).
//!
//! Pipeline (setContentView(I)):
//! 1. `resources.arsc` resolve o resid → `@layout/<key>` (`Arsc::resolve_name`);
//! 2. o ZIP do APK entrega `res/layout/<key>.xml` (fallback: qualquer
//!    `res/layout*/<key>.xml` para configs — land/night — primeira em ordem);
//! 3. `rd_apk::axml::parse` decodifica o XML binário (attrs já tipados);
//! 4. cada elemento vira uma view host: `alloc_instance` + `attach_host_state`
//!    (hierarquia Android é BUILTIN no classpath — APKs reais não definem
//!    android.* no DEX) + attrs aplicados + `add_child` recursivo.
//!
//! Semântica de attrs do modelo headless (M3 — o render pixel foi o M4 ✅):
//! - `id` → resid numérico do view (findViewById e o dump do agente usam);
//! - `text` → string crua OU `@string/x` resolvida no arsc (fallback nome);
//! - `orientation`/`visibility`/`enabled` → estado host direto;
//! - `onClick` → nome de método resolvido na ACTIVITY corrente no toque;
//! - `layout_height` de FOLHA com dimensão → px (DENSITY 2.0 = viewport 360dp);
//!   match_parent/wrap_content e `layout_width` mantêm o default do modelo
//!   (largura = container, altura de leaf = touch target);
//! - `layout_weight` de filhos do LinearLayout é armazenado no LayoutParams
//!   para a distribuição de espaço na medição; attrs não modelados (textSize,
//!   padding, gravity, background/…) são IGNORADOS com log sob `RD_FW_DEBUG`.

use crate::engine::Engine;
use crate::err::VmExit;
use crate::framework;
use crate::heap::ObjRef;
use crate::value::Value;

use rd_apk::arsc::ResolvedValue;
use rd_apk::axml::{AttrValue, ComplexUnit, XmlAttribute, XmlElement};

/// Infla `layout_resid` (R.layout.x) do APK injetado e devolve a raiz da
/// árvore de views host. Quem anexa na window é o chamador (dispatch de
/// `Activity.setContentView(I)` → `framework::set_content_view`). O contexto
/// de `android:onClick` é a ACTIVITY corrente, resolvida no toque.
pub fn inflate_resource(vm: &mut Engine, layout_resid: u32) -> Result<ObjRef, VmExit> {
    let context = vm.fw.current_activity;
    let doc = load_layout_document(vm, layout_resid)?;
    let mut count = 0usize;
    inflate_element_depth(vm, &doc.root, 0, &mut count, context)
}

/// Equivalente host de `LayoutInflater.inflate(resource, root, attachToRoot)`.
/// Um root `<merge>` só é válido quando há ViewGroup e attachToRoot=true;
/// nesse caso seus filhos são adicionados diretamente ao root.
pub fn inflate_resource_into(
    vm: &mut Engine,
    layout_resid: u32,
    root: Option<ObjRef>,
    attach_to_root: bool,
) -> Result<ObjRef, VmExit> {
    let context = vm.fw.current_activity;
    inflate_resource_into_with_context(vm, layout_resid, root, attach_to_root, context)
}

pub(crate) fn inflate_resource_into_with_context(
    vm: &mut Engine,
    layout_resid: u32,
    root: Option<ObjRef>,
    attach_to_root: bool,
    context: Option<ObjRef>,
) -> Result<ObjRef, VmExit> {
    let doc = load_layout_document(vm, layout_resid)?;
    let mut count = 0usize;
    if doc.root.name == "merge" {
        let Some(root) = root.filter(|_| attach_to_root) else {
            return Err(inflate_error(
                "<merge> exige um ViewGroup root e attachToRoot=true",
            ));
        };
        count += 1;
        inflate_children(vm, root, &doc.root.children, 1, &mut count, context)?;
        return Ok(root);
    }

    let view = inflate_element_depth(vm, &doc.root, 0, &mut count, context)?;
    if let Some(parent) = root {
        assign_layout_params(vm, parent, view, &[&doc.root])?;
    }
    if let Some(root) = root.filter(|_| attach_to_root) {
        framework::attach_child(vm, root, view)?;
        Ok(root)
    } else {
        Ok(view)
    }
}

fn layout_param_attr<'a>(elements: &[&'a XmlElement], name: &str) -> Option<&'a XmlAttribute> {
    elements.iter().find_map(|element| {
        element
            .attrs
            .iter()
            .find(|attr| attr.is_android() && attr.name == name)
    })
}

fn layout_param_size(
    vm: &Engine,
    elements: &[&XmlElement],
    name: &str,
    cap: i32,
    default: i32,
) -> Result<i32, VmExit> {
    let Some(attr) = layout_param_attr(elements, name) else {
        return Ok(default);
    };
    match &attr.value {
        AttrValue::Int(value) if *value < 0 => Ok(*value), // MATCH_PARENT/WRAP_CONTENT
        AttrValue::Reference(resid) => ref_dimension_px(vm, name, *resid, cap),
        AttrValue::Int(value) if *value <= cap => Ok(*value),
        AttrValue::Int(value) => Err(inflate_error(format!(
            "android:{name}={value}px excede o limite de {cap}px"
        ))),
        _ => Ok(dimension_px(attr, cap, name)?.unwrap_or(default)),
    }
}

fn layout_param_weight(elements: &[&XmlElement]) -> Result<f32, VmExit> {
    let Some(attr) = layout_param_attr(elements, "layout_weight") else {
        return Ok(0.0);
    };
    let weight = match &attr.value {
        AttrValue::Float(value) => *value,
        AttrValue::Int(value) => *value as f32,
        AttrValue::String(value) => value.parse::<f32>().map_err(|_| {
            inflate_error(format!(
                "android:layout_weight={value:?} não é um float válido"
            ))
        })?,
        _ => {
            return Err(inflate_error(format!(
                "android:layout_weight={} não é um float válido",
                attr.value.as_text()
            )))
        }
    };
    if !weight.is_finite() || weight < 0.0 {
        return Err(inflate_error(format!(
            "android:layout_weight={weight} deve ser finito e não negativo"
        )));
    }
    Ok(weight)
}

fn layout_margin(
    vm: &Engine,
    elements: &[&XmlElement],
    name: &str,
    default: i32,
) -> Result<i32, VmExit> {
    let Some(attr) = layout_param_attr(elements, name) else {
        return Ok(default);
    };
    match &attr.value {
        AttrValue::Int(value) if value.unsigned_abs() <= framework::WINDOW_H as u32 => Ok(*value),
        AttrValue::Int(value) => Err(inflate_error(format!(
            "android:{name}={value}px excede o limite de margem"
        ))),
        AttrValue::Reference(resid) => ref_dimension_px(vm, name, *resid, framework::WINDOW_H),
        _ => Ok(dimension_px(attr, framework::WINDOW_H, name)?.unwrap_or(default)),
    }
}

fn layout_params_class(vm: &Engine, parent: ObjRef) -> Result<String, VmExit> {
    let parent_class = vm.heap.class_of(parent)?;
    // Many AndroidX/custom ViewGroups define their own nested LayoutParams and
    // cast children to it from onMeasure/onLayout. Prefer that actual DEX type
    // when present; platform widget defaults below cover framework classes that
    // are modeled natively and therefore have no LayoutParams class in the APK.
    let custom = format!("{}$LayoutParams;", parent_class.trim_end_matches(';'));
    if vm.cp.find_class(&custom).is_some()
        && vm
            .cp
            .is_subtype(&custom, "Landroid/view/ViewGroup$LayoutParams;")
    {
        return Ok(custom);
    }
    Ok(if vm
        .cp
        .is_subtype(parent_class, "Landroid/widget/LinearLayout;")
    {
        "Landroid/widget/LinearLayout$LayoutParams;"
    } else if vm
        .cp
        .is_subtype(parent_class, "Landroid/widget/FrameLayout;")
    {
        "Landroid/widget/FrameLayout$LayoutParams;"
    } else if vm
        .cp
        .is_subtype(parent_class, "Landroid/widget/RelativeLayout;")
    {
        "Landroid/widget/RelativeLayout$LayoutParams;"
    } else if vm.cp.is_subtype(
        parent_class,
        "Landroidx/coordinatorlayout/widget/CoordinatorLayout;",
    ) {
        // CoordinatorLayout casts every child LayoutParams to this DEX
        // subtype during onAttachedToWindow/resetTouchBehaviors.
        "Landroidx/coordinatorlayout/widget/CoordinatorLayout$LayoutParams;"
    } else if vm.cp.is_subtype(
        parent_class,
        "Landroidx/constraintlayout/widget/ConstraintLayout;",
    ) {
        // S7: idem — o solver/checks do ConstraintLayout fazem cast dos
        // LayoutParams dos filhos para o subtipo próprio.
        "Landroidx/constraintlayout/widget/ConstraintLayout$LayoutParams;"
    } else if vm
        .cp
        .is_subtype(parent_class, "Landroidx/drawerlayout/widget/DrawerLayout;")
    {
        "Landroidx/drawerlayout/widget/DrawerLayout$LayoutParams;"
    } else if vm
        .cp
        .is_subtype(parent_class, "Landroidx/viewpager/widget/ViewPager;")
    {
        "Landroidx/viewpager/widget/ViewPager$LayoutParams;"
    } else if vm
        .cp
        .is_subtype(parent_class, "Landroidx/recyclerview/widget/RecyclerView;")
    {
        "Landroidx/recyclerview/widget/RecyclerView$LayoutParams;"
    } else {
        "Landroid/view/ViewGroup$MarginLayoutParams;"
    }
    .to_string())
}

/// Android's LayoutInflater asks the supplied parent to generate params even
/// when `attachToRoot` is false. Keep them on the View heap object so normal
/// View.getLayoutParams() and GC reachability both see the same object.
fn assign_layout_params(
    vm: &mut Engine,
    parent: ObjRef,
    child: ObjRef,
    elements: &[&XmlElement],
) -> Result<(), VmExit> {
    let params_class = layout_params_class(vm, parent)?;
    let width = layout_param_size(vm, elements, "layout_width", framework::WINDOW_W, -2)?;
    let height = layout_param_size(vm, elements, "layout_height", framework::WINDOW_H, -2)?;
    let params = vm
        .heap
        .alloc_instance(params_class.clone(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    let has_dex_size_ctor = vm
        .cp
        .find_own_method(&params_class, "<init>", "(II)V")
        .is_some();
    let has_dex_default_ctor = vm
        .cp
        .find_own_method(&params_class, "<init>", "()V")
        .is_some();
    if vm.cp.find_class(&params_class).is_none() && !params_class.starts_with("Landroid/") {
        // S7: LayoutParams de lib conhecido só pela hierarquia builtin
        // (ConstraintLayout, DrawerLayout…) sem DEX no APK — não há corpo
        // de construtor para executar; semeia width/height herdados, tal
        // como o ramo do construtor no-arg abaixo.
        crate::err::put_field_result(vm.heap.put_field(params, "width", Value::Int(width)))?;
        crate::err::put_field_result(vm.heap.put_field(params, "height", Value::Int(height)))?;
    } else if vm.cp.find_class(&params_class).is_some()
        && !has_dex_size_ctor
        && has_dex_default_ctor
    {
        // AndroidX ViewPager.LayoutParams has only a no-arg DEX constructor;
        // the host inflater still has to seed the inherited width/height fields.
        vm.invoke_instance(params, "<init>", "()V", &[])?;
        crate::err::put_field_result(vm.heap.put_field(params, "width", Value::Int(width)))?;
        crate::err::put_field_result(vm.heap.put_field(params, "height", Value::Int(height)))?;
    } else {
        vm.invoke_instance(
            params,
            "<init>",
            "(II)V",
            &[Value::Int(width), Value::Int(height)],
        )?;
    }

    let all = layout_margin(vm, elements, "layout_margin", 0)?;
    let horizontal = layout_margin(vm, elements, "layout_marginHorizontal", all)?;
    let vertical = layout_margin(vm, elements, "layout_marginVertical", all)?;
    let left = layout_margin(vm, elements, "layout_marginLeft", horizontal)?;
    let right = layout_margin(vm, elements, "layout_marginRight", horizontal)?;
    let top = layout_margin(vm, elements, "layout_marginTop", vertical)?;
    let bottom = layout_margin(vm, elements, "layout_marginBottom", vertical)?;
    let left = layout_margin(vm, elements, "layout_marginStart", left)?;
    let right = layout_margin(vm, elements, "layout_marginEnd", right)?;
    for (field, value) in [
        ("leftMargin", left),
        ("topMargin", top),
        ("rightMargin", right),
        ("bottomMargin", bottom),
    ] {
        crate::err::put_field_result(vm.heap.put_field(params, field, Value::Int(value)))?;
    }
    let parent_class = vm.heap.class_of(parent)?;
    if vm
        .cp
        .is_subtype(parent_class, "Landroid/widget/LinearLayout;")
    {
        let weight = layout_param_weight(elements)?;
        crate::err::put_field_result(vm.heap.put_field(params, "weight", Value::Float(weight)))?;
    }
    crate::err::put_field_result(
        vm.heap
            .put_field(child, "mLayoutParams", Value::Obj(params)),
    )?;
    Ok(())
}

fn load_layout_document(
    vm: &Engine,
    layout_resid: u32,
) -> Result<rd_apk::axml::AxmlDocument, VmExit> {
    let apk = vm.resources()?;
    let arsc = apk.arsc.as_ref().ok_or_else(|| {
        inflate_error("APK sem resources.arsc — LayoutInflater exige tabela de recursos")
    })?;
    let name = arsc.resolve_name(layout_resid).ok_or_else(|| {
        inflate_error(format!(
            "resid 0x{layout_resid:08x} não existe no resources.arsc"
        ))
    })?;
    let Some(key) = name.strip_prefix("@layout/") else {
        return Err(inflate_error(format!(
            "resid 0x{layout_resid:08x} não é layout (resolve para {name})"
        )));
    };
    // Compiled APKs may rename resource ZIP paths (e.g. `res/Ab.xml`) while
    // resources.arsc retains the actual path. Synthetic/legacy fixtures still
    // use the canonical-name fallback.
    let entry = layout_entry_for_resource(apk, arsc, layout_resid, key)?;
    let bytes = apk
        .container()
        .read(&entry)
        .map_err(|e| inflate_error(format!("{entry}: {e}")))?;
    rd_apk::axml::parse(&bytes).map_err(|e| inflate_error(format!("{entry}: {e}")))
}

/// Entrada ZIP do layout: caminho canônico primeiro; fallback varre configs
/// (`res/layout-land/…`, `res/layout-night/…`).
/// issue #61: a escolha NÃO é mais cega à ordem do ZIP — pontua qualificadores
/// conhecidos contra o device virtual (retrato, tema dia): qualificador que
/// CONTRADIZ o device perde; empate → ordem lexicográfica (determinística).
/// FALHA TIPADA se nenhuma existir.
fn layout_entry_for_resource(
    apk: &rd_apk::Apk,
    arsc: &rd_apk::arsc::Arsc,
    resid: u32,
    key: &str,
) -> Result<String, VmExit> {
    let mut current = resid;
    let mut seen = Vec::new();
    for _ in 0..64 {
        if seen.contains(&current) {
            break; // malformed resource alias cycle; use the legacy lookup below
        }
        seen.push(current);
        match arsc.resolve_value(current) {
            Some(ResolvedValue::Str(path))
                if path.starts_with("res/")
                    && path.ends_with(".xml")
                    && apk.container().find(&path).is_some() =>
            {
                return Ok(path);
            }
            Some(ResolvedValue::Reference(next)) => current = next,
            _ => break,
        }
    }
    layout_entry(apk, key)
}

fn layout_entry(apk: &rd_apk::Apk, key: &str) -> Result<String, VmExit> {
    let canonical = format!("res/layout/{key}.xml");
    if apk.container().find(&canonical).is_some() {
        return Ok(canonical);
    }
    let suffix = format!("/{key}.xml");
    let mut best: Option<(u32, &str)> = None;
    for n in apk.container().names_with_prefix("res/layout") {
        // issue #46: o prefixo cru casa "res/layoutfoo/" — exigir res/layout/
        // (canonical já tratado acima) ou res/layout-<config>/
        if !n["res/layout".len()..].starts_with('-') {
            continue;
        }
        if !n.ends_with(&suffix) {
            continue;
        }
        let score = config_dir_score(n);
        let better = match best {
            None => true,
            Some((bs, bn)) => score < bs || (score == bs && n < bn),
        };
        if better {
            best = Some((score, n));
        }
    }
    match best {
        Some((_, n)) => Ok(n.to_string()),
        None => Err(crate::err::with_suggestion(
            crate::err::vm_error(
                "INFLATE",
                format!("layout '{key}' não encontrado no APK (sem res/layout/{key}.xml)"),
            ),
            "confira se o resid é R.layout.* deste APK",
        )
        .into()),
    }
}

/// issue #61: penalidade de um diretório `res/layout-<cfg>/` contra o device
/// virtual (retrato, tema dia, locale/densidade neutros no modelo headless).
/// Qualificadores conhecidos que contradizem o device perdem; desconhecidos
/// são neutros (o arquivo exato continua determinístico pelo desempate).
fn config_dir_score(name: &str) -> u32 {
    let mut s = 0u32;
    if name.contains("-land") {
        s += 4; // device é retrato
    }
    if name.contains("-night") {
        s += 4; // device é tema dia
    }
    s
}

/// issue #40: cap de profundidade da inflação — AXML hostil aninhado (a
/// profundidade é controlada pelo atacante) não pode esgotar a stack Rust e
/// abortar o processo. O caminho host é recursivo e os frames Rust, sobretudo
/// em builds debug, são muito maiores que os frames Dalvik; 128 níveis deixam
/// margem em stacks de worker/teste de 2 MiB e retornam erro INFLATE tipado.
const MAX_INFLATE_DEPTH: usize = 128;

/// issue #45: cap de CONTAGEM de views — proteção de CPU/memória adicional
/// (layout O(N) pós-inflação; 10k views ≈ apps reais; acima disso é hostil).
const MAX_INFLATE_VIEWS: usize = 10_000;

fn inflate_element_depth(
    vm: &mut Engine,
    el: &XmlElement,
    depth: usize,
    count: &mut usize,
    context: Option<ObjRef>,
) -> Result<ObjRef, VmExit> {
    if depth > MAX_INFLATE_DEPTH {
        return Err(inflate_error(format!(
            "AXML aninhado além de {MAX_INFLATE_DEPTH} níveis — estrutura hostil (cap do LayoutInflater)"
        )));
    }
    *count += 1;
    if *count > MAX_INFLATE_VIEWS {
        return Err(inflate_error(format!(
            "árvore de views excede {MAX_INFLATE_VIEWS} views — estrutura hostil (cap do LayoutInflater)"
        )));
    }
    let desc = view_desc_of(&el.name)?;
    if !vm.cp.is_subtype(&desc, framework::VIEW) {
        return Err(inflate_error(format!(
            "{}: classe {desc} não resolve para android.view.View na hierarquia DEX/plataforma",
            el.name
        )));
    }
    let r = vm
        .heap
        .alloc_instance(desc.clone(), Vec::new())
        .map_err(|e| {
            VmExit::Exception(crate::err::Throwable::new(
                "Ljava/lang/OutOfMemoryError;",
                format!(
                    "inflação de {desc}: {} bytes excedem o heap de {} bytes",
                    e.requested, e.budget
                ),
            ))
        })?;
    vm.attach_host_state(&desc, r)?;
    construct_inflated_view(vm, &desc, r, context)?;
    apply_attrs(vm, r, el)?;
    // issue #45: attach SEM re-layout — o setContentView leita a raiz
    // UMA vez (O(N)); re-layout por filho era O(N²). `<include>` é expandido
    // aqui porque pode carregar outro layout ou um root `<merge>`.
    inflate_children(vm, r, &el.children, depth + 1, count, context)?;
    Ok(r)
}

/// DEX custom Views need their Android constructor to initialize host-visible
/// and library state (for example AppCompat's decor-padding Rect). Constructors
/// receive the selected Context; attrs are null until the AXML-backed
/// AttributeSet adapter is available.
fn construct_inflated_view(
    vm: &mut Engine,
    desc: &str,
    view: ObjRef,
    context: Option<ObjRef>,
) -> Result<(), VmExit> {
    if desc.starts_with("Landroid/") {
        return Ok(()); // platform Views are initialized by host state
    }
    let Some((dex_idx, class_def)) = vm.cp.find_class(desc) else {
        return Ok(()); // no DEX constructor to invoke
    };
    let class_identity = (dex_idx, class_def.class_idx);
    let Some(context) = context else {
        return Err(inflate_error(format!(
            "View {desc} requires a LayoutInflater Context for its DEX constructor"
        )));
    };
    let candidates = [
        (
            "(Landroid/content/Context;Landroid/util/AttributeSet;I)V",
            vec![Value::Obj(context), Value::Null, Value::Int(0)],
        ),
        (
            "(Landroid/content/Context;Landroid/util/AttributeSet;)V",
            vec![Value::Obj(context), Value::Null],
        ),
        ("(Landroid/content/Context;)V", vec![Value::Obj(context)]),
        ("()V", Vec::new()),
    ];
    for (sig, args) in candidates {
        let declared = vm
            .cp
            .resolve_method(desc, "<init>", sig)
            .is_some_and(|(owner_dex, owner, _)| (owner_dex, owner.class_idx) == class_identity);
        if declared {
            vm.invoke_instance(view, "<init>", sig, &args)?;
            return Ok(());
        }
    }
    Err(inflate_error(format!(
        "View {desc} has no supported DEX constructor (Context, AttributeSet[, defStyleAttr])"
    )))
}

fn inflate_children(
    vm: &mut Engine,
    parent: ObjRef,
    children: &[XmlElement],
    depth: usize,
    count: &mut usize,
    context: Option<ObjRef>,
) -> Result<(), VmExit> {
    for child in children {
        if child.name == "include" {
            if depth > MAX_INFLATE_DEPTH {
                return Err(inflate_error(format!(
                    "AXML aninhado além de {MAX_INFLATE_DEPTH} níveis — estrutura hostil (cap do LayoutInflater)"
                )));
            }
            *count += 1;
            if *count > MAX_INFLATE_VIEWS {
                return Err(inflate_error(format!(
                    "árvore de views excede {MAX_INFLATE_VIEWS} views — estrutura hostil (cap do LayoutInflater)"
                )));
            }
            let layout = child
                .attr("layout")
                .and_then(resid_of)
                .ok_or_else(|| inflate_error("<include> sem android:layout resource"))?;
            let included = load_layout_document(vm, layout)?;
            if included.root.name == "merge" {
                inflate_children(
                    vm,
                    parent,
                    &included.root.children,
                    depth + 1,
                    count,
                    context,
                )?;
            } else {
                let view = inflate_element_depth(vm, &included.root, depth + 1, count, context)?;
                // Os atributos android:* do include sobrescrevem atributos
                // correspondentes da view raiz incluída (notavelmente id).
                apply_attrs(vm, view, child)?;
                assign_layout_params(vm, parent, view, &[child, &included.root])?;
                framework::attach_child(vm, parent, view)?;
            }
        } else {
            let view = inflate_element_depth(vm, child, depth, count, context)?;
            assign_layout_params(vm, parent, view, &[child])?;
            framework::attach_child(vm, parent, view)?;
        }
    }
    Ok(())
}

/// Nome de elemento AXML → descritor de classe host.
/// Shorthand do Android (`<LinearLayout>`) OU caminho completo
/// (`<android.widget.LinearLayout>`).
fn view_desc_of(name: &str) -> Result<String, VmExit> {
    let known = match name {
        "View" => framework::VIEW,
        "TextView" => framework::TEXTVIEW,
        "ImageView" => "Landroid/widget/ImageView;",
        "EditText" => "Landroid/widget/EditText;",
        "Button" => framework::BUTTON,
        "LinearLayout" => framework::LINEARLAYOUT,
        "androidx.recyclerview.widget.RecyclerView" => crate::m8::RECYCLER,
        "RecyclerView" => crate::m8::RECYCLER,
        "FrameLayout" => framework::FRAMELAYOUT,
        "ProgressBar" => "Landroid/widget/ProgressBar;",
        "RadioButton" => "Landroid/widget/RadioButton;",
        "RadioGroup" => "Landroid/widget/RadioGroup;",
        "ScrollView" => "Landroid/widget/ScrollView;",
        other if other.contains('.') => {
            // FQCN — a checagem is_subtype(VIEW) no caller filtra o que o
            // framework host não implementa (ex.: ImageView/WebView)
            return Ok(format!("L{};", other.replace('.', "/")));
        }
        other => {
            return Err(inflate_error(format!(
                "tag <{other}> sem mapeamento no framework host M3"
            )));
        }
    };
    Ok(known.to_string())
}

/// Aplica os atributos android:* de um elemento à view host recém-inflada.
fn apply_attrs(vm: &mut Engine, r: ObjRef, el: &XmlElement) -> Result<(), VmExit> {
    for a in &el.attrs {
        if !a.is_android() {
            continue; // app:/tools:/custom — fora do escopo M3
        }
        match a.name.as_str() {
            "id" => {
                if let Some(id) = resid_of(a) {
                    if let Some(framework::HostObj::View { id: slot, .. }) =
                        vm.fw.objects.get_mut(&r)
                    {
                        *slot = id as i32;
                    }
                }
            }
            "text" => {
                let s = attr_text(vm, a)?;
                if let Some(framework::HostObj::View { text, .. }) = vm.fw.objects.get_mut(&r) {
                    *text = s;
                }
            }
            "orientation" => {
                let o = match &a.value {
                    AttrValue::Int(i) => (*i != 0) as i32, // 0=h, 1=v
                    AttrValue::String(s) => match s.as_str() {
                        "vertical" => 1,
                        _ => 0,
                    },
                    _ => 0,
                };
                if let Some(framework::HostObj::View { orientation, .. }) =
                    vm.fw.objects.get_mut(&r)
                {
                    *orientation = o;
                }
            }
            "visibility" => {
                // issue #46: 3 estados reais — 0=VISIBLE, 1=INVISIBLE, 2=GONE
                // (antes: colapsava em bool e GONE consumia espaço do layout)
                let vis_state = match &a.value {
                    AttrValue::Int(i) => framework::Vis::from_java(*i),
                    AttrValue::String(s) => match s.as_str() {
                        "invisible" => framework::Vis::Invisible,
                        "gone" => framework::Vis::Gone,
                        _ => framework::Vis::Visible,
                    },
                    _ => framework::Vis::Visible,
                };
                if let Some(framework::HostObj::View { vis, .. }) = vm.fw.objects.get_mut(&r) {
                    *vis = vis_state;
                }
            }
            "enabled" => {
                // issue #61: referência @bool/@string RESOLVE no arsc (o deferral
                // "M4" da #46 quebrou — nunca mais default silencioso)
                let en = match &a.value {
                    AttrValue::Reference(res) => ref_bool(vm, "enabled", *res)?,
                    v => v.as_bool().unwrap_or(true),
                };
                if let Some(framework::HostObj::View { enabled, .. }) = vm.fw.objects.get_mut(&r) {
                    *enabled = en;
                }
            }
            "onClick" => {
                let m = a
                    .raw
                    .clone()
                    .or_else(|| a.value.as_string().map(str::to_string))
                    .ok_or_else(|| {
                        inflate_error(format!(
                            "android:onClick sem nome de método em <{}>",
                            el.name
                        ))
                    })?;
                if let Some(framework::HostObj::View { click_method, .. }) =
                    vm.fw.objects.get_mut(&r)
                {
                    *click_method = Some(m);
                }
            }
            "layout_height" if el.children.is_empty() => {
                // FOLHA: dimensão explícita → px; match_parent/wrap_content
                // mantêm o default do modelo (touch target). Contêineres têm
                // a altura COMPUTADA pelo passe de layout (soma/max).
                // issue #61: @dimen/x por referência RESOLVE no arsc (deferral
                // da #46 quebrou — bounds errados sem pista não volta).
                let px = match &a.value {
                    AttrValue::Reference(res) => Some(ref_dimension_px(
                        vm,
                        "layout_height",
                        *res,
                        framework::WINDOW_H,
                    )?),
                    _ => dimension_px(a, framework::WINDOW_H, "altura")?,
                };
                if let Some(px) = px {
                    if let Some(framework::HostObj::View { h, .. }) = vm.fw.objects.get_mut(&r) {
                        *h = px;
                    }
                }
            }
            "layout_width" => {
                // issue #59/#61: largura explícita → w_explicit (vence o slot
                // distribuído no layout); validada contra WINDOW_W (largura).
                // match_parent/wrap_content/negativo → None (slot do pai).
                let px = match &a.value {
                    AttrValue::Reference(res) => Some(ref_dimension_px(
                        vm,
                        "layout_width",
                        *res,
                        framework::WINDOW_W,
                    )?),
                    _ => dimension_px(a, framework::WINDOW_W, "largura")?,
                };
                if let Some(px) = px {
                    if let Some(framework::HostObj::View { w_explicit, .. }) =
                        vm.fw.objects.get_mut(&r)
                    {
                        *w_explicit = Some(px);
                    }
                }
            }
            other => {
                if std::env::var("RD_FW_DEBUG").is_ok() {
                    eprintln!("[fw] inflate: attr android:{other} ignorado no modelo headless M3");
                }
            }
        }
    }
    Ok(())
}

/// resid de `android:id` — `@+id/x`/`@id/x` chegam como Reference tipada do
/// AXML compilado; fallback hex/decimal no raw (AXML sem resource map).
fn resid_of(a: &XmlAttribute) -> Option<u32> {
    match &a.value {
        AttrValue::Reference(res) => Some(*res),
        _ => a.as_u32(),
    }
}

/// Texto de atributo: string crua OU referência resolvida no arsc (fallback
/// nome qualificado — mesmo padrão do manifest.resolve_label).
fn attr_text(vm: &mut Engine, a: &XmlAttribute) -> Result<String, VmExit> {
    match &a.value {
        AttrValue::String(s) => Ok(s.clone()),
        AttrValue::Reference(res) => vm.resolve_string(*res),
        AttrValue::Null => Ok(String::new()),
        _ => Ok(a.text()),
    }
}

/// `layout_height`/`layout_width` com dimensão → px do modelo headless
/// (DENSITY 2.0). `None` = match_parent/wrap_content/int negativo (usa
/// default do modelo).
/// issue #45: dimensão que excede o viewport é ERRO TIPADO (antes: i32::MAX
/// cru passava e os consumidores estouravam — panic em debug, wrap silencioso
/// em release → coordenadas negativas → dump/hit-test corruptos).
/// issue #59: o teto é POR EIXO — largura ≤ WINDOW_W (720), altura ≤
/// WINDOW_H (1440); usar WINDOW_W nos dois rejeitava layout_height legal
/// de 361–1440px (falso-positivo INFLATE em apps reais comuns).
fn dimension_px(a: &XmlAttribute, cap: i32, eixo: &str) -> Result<Option<i32>, VmExit> {
    let px = match &a.value {
        AttrValue::Dimension(v, u) => {
            let scale = match u {
                ComplexUnit::Px => 1.0,
                _ => framework::DENSITY, // dip/sp/pt/in/mm → escala do viewport
            };
            Some((*v * scale).round())
        }
        AttrValue::Int(i) if *i >= 0 => Some(*i as f32), // px puro
        _ => None,
    };
    match px {
        None => Ok(None),
        Some(f) if f >= 0.0 && f <= cap as f32 => Ok(Some(f as i32)),
        Some(f) => Err(inflate_error(format!(
            "dimensão de layout {f}px excede o viewport de {cap}px no eixo {eixo} (hostil ou bug do app)"
        ))),
    }
}

/// issue #61: `@dimen/x` por referência → px, validado contra o teto do eixo.
/// Segue cadeias de Reference (alias) até 8 saltos. NÃO resolve (ou resolve
/// para tipo errado) → INFLATE tipado — nunca default silencioso.
fn ref_dimension_px(vm: &Engine, attr: &str, resid: u32, cap: i32) -> Result<i32, VmExit> {
    let mut cur = resid;
    for _ in 0..8 {
        match vm.resolve_resource_value(cur).map_err(|e| {
            inflate_error(format!(
                "android:{attr}=@0x{resid:08x}: referência não resolvida ({})",
                match &e {
                    VmExit::Error(rd) => rd.cause.clone(),
                    other => other.to_string(),
                }
            ))
        })? {
            ResolvedValue::Reference(next) => {
                cur = next;
            }
            ResolvedValue::Dimension { value, unit } => {
                let scale = match unit {
                    ComplexUnit::Px => 1.0,
                    _ => framework::DENSITY, // dip/sp/pt/in/mm → escala do viewport
                };
                let f = (value * scale).round();
                if f < 0.0 || f > cap as f32 {
                    return Err(inflate_error(format!(
                        "android:{attr}=@0x{resid:08x}: dimensão {f}px excede o viewport de {cap}px no eixo (hostil ou bug do app)"
                    )));
                }
                return Ok(f as i32);
            }
            ResolvedValue::Integer(i) if i >= 0 => return Ok(i), // px puro
            other => {
                return Err(inflate_error(format!(
                    "android:{attr}=@0x{resid:08x}: resolve para {other:?} — esperado dimensão (@dimen)"
                )))
            }
        }
    }
    Err(inflate_error(format!(
        "android:{attr}=@0x{resid:08x}: cadeia de referências além de 8 saltos (ciclo no arsc)"
    )))
}

/// issue #61: `@bool/x` por referência → boolean (cadeias de Reference idem).
/// NÃO resolve ou tipo errado → INFLATE tipado — nunca default silencioso.
fn ref_bool(vm: &Engine, attr: &str, resid: u32) -> Result<bool, VmExit> {
    let mut cur = resid;
    for _ in 0..8 {
        match vm.resolve_resource_value(cur).map_err(|e| {
            inflate_error(format!(
                "android:{attr}=@0x{resid:08x}: referência não resolvida ({})",
                match &e {
                    VmExit::Error(rd) => rd.cause.clone(),
                    other => other.to_string(),
                }
            ))
        })? {
            ResolvedValue::Reference(next) => {
                cur = next;
            }
            ResolvedValue::Boolean(b) => return Ok(b),
            ResolvedValue::Integer(i) => return Ok(i != 0),
            other => {
                return Err(inflate_error(format!(
                "android:{attr}=@0x{resid:08x}: resolve para {other:?} — esperado boolean (@bool)"
            )))
            }
        }
    }
    Err(inflate_error(format!(
        "android:{attr}=@0x{resid:08x}: cadeia de referências além de 8 saltos (ciclo no arsc)"
    )))
}

fn inflate_error(cause: impl Into<String>) -> VmExit {
    crate::err::with_suggestion(
        crate::err::vm_error("INFLATE", cause),
        "verifique res/layout/*.xml — tags/attrs fora do escopo M3 falham tipadas (nunca silenciosas)",
    )
    .into()
}
