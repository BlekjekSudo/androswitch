//! UI dump no formato uiautomator (M4 DoD: "get_ui_tree com ids certos").
//!
//! Fonte de verdade: `rd_vm::Engine::view_tree()` (árvore host estruturada).
//! O formato espelha o `uiautomator dump` do Android real: `<hierarchy>` com
//! `<node>` campo a campo (index/text/resource-id/class/package/bounds/…),
//! bounds "[x1,y1][x2,y2]" em px do viewport. Views GONE não aparecem
//! (comportamento do uiautomator real); INVISIBLE aparece com bounds (ocupa
//! espaço, não desenha — issue #46).
//!
//! Contrato de fidelidade (issues #53/#54):
//! - TODOS os valores interpolados passam por `xml_escape` (package e
//!   resource-id incluídos — são controlados pelo APK hostil);
//! - nenhum caractere de controle cru: `\t\n\r` viram referências de
//!   caractere XML, os demais < 0x20 viram U+FFFD (XML 1.0 os proíbe — o
//!   dump do Android real sai saneado por getText);
//! - `index` é POR-IRMÃOS (reinicia 0 em cada pai), como no uiautomator real;
//! - `focusable` é propriedade própria (não fudge de `clickable`).
//!
//! Atributos NCK (scrollable/checkable/checked/password/selected/long-clickable/
//! focused/content-desc) são `false`/`""` constantes hoje: o modelo headless
//! ainda não rastreia esses estados — compat-gap DOCUMENTADO no README (issue
//! #54.3); `focusable` já é campo honesto do UiNode.

use crate::RdError;
use rd_vm::framework::{UiNode, Vis};

/// issue #62: budget de tamanho do dump — um `text` de 8 MB com `&&&&` multiplica
/// a saída 5×; acima disso é amplificação hostil → erro tipado (nunca silêncio).
pub const MAX_DUMP_BYTES: usize = 64 * 1024 * 1024;
const MAX_DUMP_DEPTH: usize = 512;

fn dump_too_large() -> RdError {
    RdError {
        code: "INVALID_FORMAT".into(),
        cause: format!(
            "dump uiautomator excede {} bytes — amplificação de saída hostil (issue #62)",
            MAX_DUMP_BYTES
        ),
        suggestion: Some("árvore de views com text hostil; cap no framework do app".into()),
        module_id: crate::MODULE_ID.into(),
    }
}

fn push_bounded(out: &mut String, part: &str) -> Result<(), RdError> {
    if out
        .len()
        .checked_add(part.len())
        .filter(|len| *len <= MAX_DUMP_BYTES)
        .is_none()
    {
        return Err(dump_too_large());
    }
    out.push_str(part);
    Ok(())
}

fn push_indent(out: &mut String, depth: usize) -> Result<(), RdError> {
    for _ in 0..depth {
        push_bounded(out, "  ")?;
    }
    Ok(())
}

/// Serializa a árvore no XML do uiautomator. Erro tipado se o dump excede
/// `MAX_DUMP_BYTES` (amplificação hostil — issue #62).
pub fn uiautomator_xml(root: &UiNode, package: &str) -> Result<String, RdError> {
    let mut out = String::new();
    push_bounded(
        &mut out,
        "<?xml version='1.0' encoding='UTF-8' standalone='yes' ?>\n<hierarchy rotation=\"0\">\n",
    )?;
    // Escape the package once, under the same byte budget; the escaped value
    // is then streamed into each node without another unbounded allocation.
    let mut pkg_esc = String::new();
    push_xml_escaped(&mut pkg_esc, package)?;
    // issue #54: index POR-IRMÃOS — o contador global DFS fazia o primeiro
    // filho do root sair com index=1 (real: 0) e qualquer seletor
    // node[@index='N'] gravado em dispositivo real divergir.
    emit_node(root, &pkg_esc, 1, 0, &mut out)?;
    push_bounded(&mut out, "</hierarchy>\n")?;
    Ok(out)
}

fn emit_node(
    root: &UiNode,
    pkg_esc: &str,
    depth: usize,
    sibling_index: usize,
    out: &mut String,
) -> Result<(), RdError> {
    enum Event<'a> {
        Enter(&'a UiNode, usize, usize),
        Exit(usize),
    }

    // The public UiNode tree is untrusted, so traversal is iterative: a
    // depth cap alone still leaves hundreds of recursive native frames.
    let mut stack = vec![Event::Enter(root, depth, sibling_index)];
    while let Some(event) = stack.pop() {
        let (n, depth, sibling_index) = match event {
            Event::Exit(depth) => {
                push_indent(out, depth)?;
                push_bounded(out, "</node>\n")?;
                continue;
            }
            Event::Enter(n, depth, sibling_index) => (n, depth, sibling_index),
        };
        if n.visibility == Vis::Gone {
            continue; // uiautomator real: GONE não aparece no dump
        }
        if depth > MAX_DUMP_DEPTH {
            push_indent(out, depth)?;
            push_bounded(
                out,
                "<!-- truncated: subárvore além de 512 níveis (cap hostil) -->\n",
            )?;
            continue;
        }
        let (x, y, w, h) = n.bounds;
        let clickable = if n.clickable { "true" } else { "false" };
        let enabled = if n.enabled { "true" } else { "false" };
        let focusable = if n.focusable { "true" } else { "false" };
        // issue #53: filhos GONE não são emitidos — se não sobra NENHUM, o nó é
        // self-closing (não <node>…</node> vazio).
        let visible_children = n
            .children
            .iter()
            .filter(|c| c.visibility != Vis::Gone)
            .count();
        let empty = visible_children == 0;
        // Bounds hostis não podem panicar nem inverter o retângulo: tamanhos
        // negativos colapsam em zero e as somas saturam.
        let x2 = x.saturating_add(w.max(0));
        let y2 = y.saturating_add(h.max(0));

        push_indent(out, depth)?;
        push_bounded(out, "<node index=\"")?;
        push_bounded(out, &sibling_index.to_string())?;
        push_bounded(out, "\" text=\"")?;
        push_xml_escaped(out, &n.text)?;
        push_bounded(out, "\" resource-id=\"")?;
        push_xml_escaped(out, n.resource_id.as_deref().unwrap_or_default())?;
        push_bounded(out, "\" class=\"")?;
        push_xml_escaped(out, &n.class)?;
        push_bounded(out, "\" package=\"")?;
        push_bounded(out, pkg_esc)?;
        push_bounded(
            out,
            "\" content-desc=\"\" checkable=\"false\" checked=\"false\" clickable=\"",
        )?;
        push_bounded(out, clickable)?;
        push_bounded(out, "\" enabled=\"")?;
        push_bounded(out, enabled)?;
        push_bounded(out, "\" focusable=\"")?;
        push_bounded(out, focusable)?;
        push_bounded(
            out,
            "\" focused=\"false\" scrollable=\"false\" long-clickable=\"false\" \
password=\"false\" selected=\"false\" bounds=\"[",
        )?;
        push_bounded(out, &x.to_string())?;
        push_bounded(out, ",")?;
        push_bounded(out, &y.to_string())?;
        push_bounded(out, "][")?;
        push_bounded(out, &x2.to_string())?;
        push_bounded(out, ",")?;
        push_bounded(out, &y2.to_string())?;
        push_bounded(out, "]\"")?;
        if empty {
            push_bounded(out, "/")?;
        }
        push_bounded(out, ">\n")?;

        // issue #62: nó com subárvore cortada ganha marcador EXPLÍCITO — o agente
        // não pode ler a árvore como completa.
        if n.truncated {
            push_indent(out, depth)?;
            push_bounded(
                out,
                "<!-- truncated: subárvore além de 512 níveis (cap hostil) -->\n",
            )?;
        }
        if !empty {
            stack.push(Event::Exit(depth));
            for (reverse_index, c) in n
                .children
                .iter()
                .filter(|c| c.visibility != Vis::Gone)
                .rev()
                .enumerate()
            {
                let i = visible_children - 1 - reverse_index;
                stack.push(Event::Enter(c, depth + 1, i));
            }
        }
    }
    Ok(())
}

/// Escape mínimo de XML + saneamento de caracteres de controle (issue #53):
/// - os 5 reservados (`& < > " '`) viram entidades — uiautomator real idem;
/// - `\t\n\r` legais em XML 1.0 mas normalizados por parsers quando crus —
///   viram referências `&#x9;`/`&#xA;`/`&#xD;` (o serializer do Android emite
///   `&#10;` para texto multilinha);
/// - os proibidos (U+0000–U+0008/U+000B/U+000C/U+000E–U+001F) e os
///   não-caracteres U+FFFE/U+FFFF viram U+FFFD — mesmo saneamento do
///   getText do Android; o dump permanece XML válido (expat:
///   "not well-formed" sem isto).
///
/// The escaped bytes are streamed into `out` under `MAX_DUMP_BYTES`, so a
/// hostile field cannot first materialize a multi-gigabyte temporary.
fn push_xml_escaped(out: &mut String, s: &str) -> Result<(), RdError> {
    for c in s.chars() {
        match c {
            '&' => push_bounded(out, "&amp;")?,
            '<' => push_bounded(out, "&lt;")?,
            '>' => push_bounded(out, "&gt;")?,
            '"' => push_bounded(out, "&quot;")?,
            '\'' => push_bounded(out, "&apos;")?,
            '\t' => push_bounded(out, "&#x9;")?,
            '\n' => push_bounded(out, "&#xA;")?,
            '\r' => push_bounded(out, "&#xD;")?,
            '\u{FFFE}' | '\u{FFFF}' => push_bounded(out, "\u{FFFD}")?,
            c if (c as u32) < 0x20 => push_bounded(out, "\u{FFFD}")?,
            _ => {
                let mut buf = [0; 4];
                push_bounded(out, c.encode_utf8(&mut buf))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(class: &str, rid: Option<&str>, text: &str, bounds: (i32, i32, i32, i32)) -> UiNode {
        UiNode {
            class: class.to_string(),
            resource_id: rid.map(str::to_string),
            text: text.to_string(),
            bounds,
            visibility: Vis::Visible,
            enabled: true,
            clickable: false,
            focusable: false,
            truncated: false,
            children: Vec::new(),
        }
    }

    #[test]
    fn dump_has_bounds_class_and_resource_id() {
        let mut root = node(
            "android.widget.LinearLayout",
            Some("com.test:id/root"),
            "",
            (0, 0, 720, 96),
        );
        root.children.push(node(
            "android.widget.TextView",
            Some("com.test:id/tv"),
            "Olá",
            (0, 0, 720, 48),
        ));
        let xml = uiautomator_xml(&root, "com.test").unwrap();
        assert!(
            xml.contains("resource-id=\"com.test:id/tv\""),
            "ids certos: {xml}"
        );
        assert!(xml.contains("class=\"android.widget.TextView\""));
        assert!(xml.contains("bounds=\"[0,0][720,48]\""));
        assert!(xml.contains("text=\"Olá\""));
        assert!(xml.starts_with("<?xml"));
        assert!(xml.contains("<hierarchy rotation=\"0\">"));
    }

    #[test]
    fn gone_is_excluded_invisible_is_kept() {
        let mut root = node("android.widget.LinearLayout", None, "", (0, 0, 720, 144));
        let mut gone = node(
            "android.widget.TextView",
            Some("com.test:id/gone"),
            "",
            (0, 0, 0, 0),
        );
        gone.visibility = Vis::Gone;
        let mut inv = node(
            "android.widget.Button",
            Some("com.test:id/inv"),
            "",
            (0, 48, 720, 48),
        );
        inv.visibility = Vis::Invisible;
        root.children.push(gone);
        root.children.push(inv);
        let xml = uiautomator_xml(&root, "com.test").unwrap();
        assert!(!xml.contains("com.test:id/gone"), "GONE não aparece: {xml}");
        assert!(
            xml.contains("com.test:id/inv"),
            "INVISIBLE aparece (ocupa espaço)"
        );
        assert!(xml.contains("bounds=\"[0,48][720,96]\""));
    }

    #[test]
    fn xml_escapes_reserved_chars() {
        let root = node(
            "android.widget.TextView",
            None,
            "a<b>&\"c\"",
            (0, 0, 10, 10),
        );
        let xml = uiautomator_xml(&root, "p").unwrap();
        assert!(xml.contains("text=\"a&lt;b&gt;&amp;&quot;c&quot;"), "{xml}");
    }

    /// issue #53 (PoC do audit): resource-id/hostil com `"` e `<` NÃO injeta
    /// nós falsos — sai escapado; package idem.
    #[test]
    fn hostile_resource_id_and_package_cannot_inject_nodes() {
        let root = node(
            "android.widget.TextView",
            Some("com.p\"/><node text=\"INJETADO"),
            "",
            (0, 0, 10, 10),
        );
        let xml = uiautomator_xml(&root, "com.p\" inj=\"1").unwrap();
        // nenhum nó falso: o `"` do id/package sai como &quot;
        assert!(!xml.contains("inj=\"1\""), "injeção via package: {xml}");
        assert!(
            !xml.contains("<node text=\"INJETADO\""),
            "injeção via resource-id: {xml}"
        );
        assert!(xml.contains("resource-id=\"com.p&quot;/&gt;&lt;node text=&quot;INJETADO\""));
        assert!(xml.contains("package=\"com.p&quot; inj=&quot;1\""));
        // estrutura: exatamente 1 <node> no dump
        assert_eq!(xml.matches("<node ").count(), 1);
    }

    /// issue #53: chars de controle não saem crus — \n vira referência
    /// (texto multilinha sobrevive a parsers XML); U+0001 vira U+FFFD.
    #[test]
    fn control_chars_are_sanitized_not_raw() {
        let root = node(
            "android.widget.TextView",
            None,
            "linha1\nlinha2\u{1}fim",
            (0, 0, 10, 10),
        );
        let xml = uiautomator_xml(&root, "p").unwrap();
        assert!(
            xml.contains("linha1&#xA;linha2"),
            "newline referenciado: {xml}"
        );
        assert!(xml.contains('\u{FFFD}'), "U+0001 → U+FFFD: {xml}");
        assert!(!xml.contains('\u{1}'), "nenhum controle cru: {xml}");
        // atributo com \t também referenciado (normalização de parser corrompia)
        let tab = node("C", Some("a\tb"), "", (0, 0, 1, 1));
        let xml = uiautomator_xml(&tab, "p").unwrap();
        assert!(xml.contains("resource-id=\"a&#x9;b\""), "{xml}");
    }

    /// issue #54.1: index por-irmãos — root=0, primeiro filho=0, segundo=1;
    /// neto reinicia.
    #[test]
    fn index_is_per_sibling_not_global_dfs() {
        let mut root = node("Root", None, "", (0, 0, 10, 10));
        let mut a = node("A", None, "", (0, 0, 5, 5));
        a.children.push(node("A1", None, "", (0, 0, 1, 1)));
        a.children.push(node("A2", None, "", (0, 0, 1, 1)));
        root.children.push(a);
        root.children.push(node("B", None, "", (0, 5, 5, 5)));
        let xml = uiautomator_xml(&root, "p").unwrap();
        let root_idx = xml
            .split("<node ")
            .nth(1)
            .and_then(|s| s.split("index=\"").nth(1).and_then(|t| t.split('"').next()));
        assert_eq!(root_idx, Some("0"));
        assert!(
            xml.contains("<node index=\"0\" text=\"\" resource-id=\"\" class=\"A\""),
            "{xml}"
        );
        assert!(
            xml.contains("<node index=\"1\" text=\"\" resource-id=\"\" class=\"B\""),
            "{xml}"
        );
        assert!(
            xml.contains("<node index=\"0\" text=\"\" resource-id=\"\" class=\"A1\""),
            "{xml}"
        );
        assert!(
            xml.contains("<node index=\"1\" text=\"\" resource-id=\"\" class=\"A2\""),
            "{xml}"
        );
    }

    /// issue #54.2: focusable é campo próprio — false honesto mesmo com
    /// clickable=true (TextView clicável NÃO é focável no Android real).
    #[test]
    fn focusable_is_not_fudged_from_clickable() {
        let mut c = node("android.widget.Button", None, "ok", (0, 0, 10, 10));
        c.clickable = true;
        let xml = uiautomator_xml(&c, "p").unwrap();
        assert!(xml.contains("clickable=\"true\""), "{xml}");
        assert!(xml.contains("focusable=\"false\""), "{xml}");
    }

    /// issue #62: bounds hostis (i32::MAX) não panican — saturating.
    #[test]
    fn hostile_bounds_do_not_overflow() {
        let n = node("C", None, "", (i32::MAX, 0, 1, 1));
        let xml = uiautomator_xml(&n, "p").unwrap();
        assert!(
            xml.contains("bounds=\"[2147483647,0][2147483647,1]\""),
            "{xml}"
        );
        let n = node("C", None, "", (0, i32::MIN, 2, i32::MAX));
        let xml = uiautomator_xml(&n, "p").unwrap();
        // MIN.saturating_add(MAX) = -1 — saturação honesta, sem panic nem wrap
        assert!(xml.contains("bounds=\"[0,-2147483648][2,-1]\""), "{xml}");
    }

    /// issue #62.2: nó truncado ganha marcador no XML.
    #[test]
    fn truncated_node_is_marked_in_xml() {
        let mut n = node("C", None, "", (0, 0, 1, 1));
        n.truncated = true;
        let xml = uiautomator_xml(&n, "p").unwrap();
        assert!(xml.contains("<!-- truncated:"), "{xml}");
    }

    #[test]
    fn deep_public_tree_is_capped_and_marked() {
        let mut root = node("L599", None, "", (0, 0, 1, 1));
        for i in (0..599).rev() {
            let mut parent = node(&format!("L{i}"), None, "", (0, 0, 1, 1));
            parent.children.push(root);
            root = parent;
        }
        let xml = uiautomator_xml(&root, "p").unwrap();
        assert_eq!(xml.matches("<node ").count(), 512);
        assert!(xml.contains("<!-- truncated:"), "cap is explicit");
    }

    #[test]
    fn single_node_cannot_bypass_dump_budget() {
        let mut root = node("C", None, "", (0, 0, 1, 1));
        // 13.5M ampersands expand to 67.5 MB, just over MAX_DUMP_BYTES.
        root.text = "&".repeat(13_500_000);
        match uiautomator_xml(&root, "p") {
            Err(err) => assert_eq!(err.code, "INVALID_FORMAT"),
            Ok(_) => panic!("expected dump budget error"),
        }
    }

    #[test]
    fn xml_noncharacters_are_sanitized_and_negative_size_does_not_invert_bounds() {
        let root = node("C", Some("id\u{FFFE}"), "a\u{FFFF}b", (10, 20, -5, -7));
        let xml = uiautomator_xml(&root, "p").unwrap();
        assert!(!xml.contains('\u{FFFE}'), "{xml}");
        assert!(!xml.contains('\u{FFFF}'), "{xml}");
        assert_eq!(xml.matches('\u{FFFD}').count(), 2, "{xml}");
        assert!(xml.contains("bounds=\"[10,20][10,20]\""), "{xml}");
    }
}
