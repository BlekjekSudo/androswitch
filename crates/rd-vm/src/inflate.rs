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
//! - attrs não modelados (textSize/padding/gravity/background/weight/…) são
//!   IGNORADOS com log sob `RD_FW_DEBUG` — Android também não falha por attr
//!   desconhecido; classes de view sem implementação host falham TIPADAS.

use crate::engine::Engine;
use crate::err::VmExit;
use crate::framework;
use crate::heap::ObjRef;

use rd_apk::arsc::ResolvedValue;
use rd_apk::axml::{AttrValue, ComplexUnit, XmlAttribute, XmlElement};

/// Infla `layout_resid` (R.layout.x) do APK injetado e devolve a raiz da
/// árvore de views host. Quem anexa na window é o chamador (dispatch de
/// `Activity.setContentView(I)` → `framework::set_content_view`). O contexto
/// de `android:onClick` é a ACTIVITY corrente, resolvida no toque.
pub fn inflate_resource(vm: &mut Engine, layout_resid: u32) -> Result<ObjRef, VmExit> {
    // 1–3: localizar e decodificar o layout (leitura imutável do Resources;
    // o borrow de `vm` termina antes da fase mutável de inflação)
    let doc = {
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
        let entry = layout_entry(apk, key)?;
        let bytes = apk
            .container()
            .read(&entry)
            .map_err(|e| inflate_error(format!("{entry}: {e}")))?;
        rd_apk::axml::parse(&bytes).map_err(|e| inflate_error(format!("{entry}: {e}")))?
    };

    // 4: inflar a árvore
    inflate_element(vm, &doc.root)
}

/// Entrada ZIP do layout: caminho canônico primeiro; fallback varre configs
/// (`res/layout-land/…`, `res/layout-night/…`).
/// issue #61: a escolha NÃO é mais cega à ordem do ZIP — pontua qualificadores
/// conhecidos contra o device virtual (retrato, tema dia): qualificador que
/// CONTRADIZ o device perde; empate → ordem lexicográfica (determinística).
/// FALHA TIPADA se nenhuma existir.
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
/// profundidade é controlada pelo atacante) estourava a stack Rust e abortava
/// o processo (não é VmExit, viola a Lei 1). 512 níveis ≈ Android real
/// (StackOverflowError do LayoutInflater); frame Rust ~400 B → ~200 KB de stack.
const MAX_INFLATE_DEPTH: usize = 512;

/// issue #45: cap de CONTAGEM de views — proteção de CPU/memória adicional
/// (layout O(N) pós-inflação; 10k views ≈ apps reais; acima disso é hostil).
const MAX_INFLATE_VIEWS: usize = 10_000;

/// Infla um elemento (e descendentes) em view host.
fn inflate_element(vm: &mut Engine, el: &XmlElement) -> Result<ObjRef, VmExit> {
    let mut count = 0usize;
    inflate_element_depth(vm, el, 0, &mut count)
}

fn inflate_element_depth(
    vm: &mut Engine,
    el: &XmlElement,
    depth: usize,
    count: &mut usize,
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
            "{}: classe {desc} não é View suportada pelo framework host M3 (só View/TextView/Button/LinearLayout/FrameLayout)",
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
    apply_attrs(vm, r, el)?;
    for child in &el.children {
        // issue #45: attach SEM re-layout — o setContentView leita a raiz
        // UMA vez (O(N)); re-layout por filho era O(N²)
        let c = inflate_element_depth(vm, child, depth + 1, count)?;
        framework::attach_child(vm, r, c)?;
    }
    Ok(r)
}

/// Nome de elemento AXML → descritor de classe host.
/// Shorthand do Android (`<LinearLayout>`) OU caminho completo
/// (`<android.widget.LinearLayout>`).
fn view_desc_of(name: &str) -> Result<String, VmExit> {
    let known = match name {
        "View" => framework::VIEW,
        "TextView" => framework::TEXTVIEW,
        "EditText" => "Landroid/widget/EditText;",
        "Button" => framework::BUTTON,
        "LinearLayout" => framework::LINEARLAYOUT,
        "androidx.recyclerview.widget.RecyclerView" => crate::m8::RECYCLER,
        "RecyclerView" => crate::m8::RECYCLER,
        "FrameLayout" => framework::FRAMELAYOUT,
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
