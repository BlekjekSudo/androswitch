//! Regression tests for issues #253 (`axml` chunk/attribute/depth hardening +
//! iterative `XmlElement::clone`) and #254 (`manifest` `glEsVersion` decoding +
//! multi-`<queries>` accumulation).

use rd_apk::axml::{self, AttrValue, AxmlDocument, XmlAttribute, XmlElement, NS_ANDROID};
use rd_apk::manifest;

const RES_XML_TYPE: u16 = 0x0003;
const RES_STRING_POOL_TYPE: u16 = 0x0001;
const RES_XML_START_ELEMENT_TYPE: u16 = 0x0102;
const RES_XML_END_ELEMENT_TYPE: u16 = 0x0103;
const NO_INDEX: u32 = 0xFFFF_FFFF;
const TYPE_STRING: u8 = 0x03;

fn elem(name: &str, attrs: Vec<XmlAttribute>, children: Vec<XmlElement>) -> XmlElement {
    XmlElement {
        name: name.to_string(),
        attrs,
        children,
        text: None,
    }
}

fn build_utf16_pool(strs: &[&str]) -> Vec<u8> {
    let mut offs = Vec::with_capacity(strs.len());
    let mut body = Vec::new();
    for s in strs {
        offs.push(body.len() as u32);
        let units: Vec<u16> = s.encode_utf16().collect();
        body.extend_from_slice(&(units.len() as u16).to_le_bytes());
        for u in units {
            body.extend_from_slice(&u.to_le_bytes());
        }
        body.extend_from_slice(&0u16.to_le_bytes());
    }
    while body.len() % 4 != 0 {
        body.push(0);
    }
    let strings_start = 28 + strs.len() * 4;
    let total = strings_start + body.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&RES_STRING_POOL_TYPE.to_le_bytes());
    out.extend_from_slice(&28u16.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(strs.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(strings_start as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for o in offs {
        out.extend_from_slice(&o.to_le_bytes());
    }
    out.extend_from_slice(&body);
    out
}

fn start_element_chunk(
    name_idx: u32,
    attribute_start: u16,
    attribute_size: u16,
    attribute_count: u16,
    raw_attr_bytes: &[u8],
) -> Vec<u8> {
    let csize = 36 + raw_attr_bytes.len();
    let mut el = Vec::with_capacity(csize);
    el.extend_from_slice(&RES_XML_START_ELEMENT_TYPE.to_le_bytes());
    el.extend_from_slice(&16u16.to_le_bytes());
    el.extend_from_slice(&(csize as u32).to_le_bytes());
    el.extend_from_slice(&1u32.to_le_bytes()); // line
    el.extend_from_slice(&NO_INDEX.to_le_bytes()); // comment
    el.extend_from_slice(&NO_INDEX.to_le_bytes()); // ns
    el.extend_from_slice(&name_idx.to_le_bytes()); // name
    el.extend_from_slice(&attribute_start.to_le_bytes());
    el.extend_from_slice(&attribute_size.to_le_bytes());
    el.extend_from_slice(&attribute_count.to_le_bytes());
    el.extend_from_slice(&0u16.to_le_bytes()); // idIndex
    el.extend_from_slice(&0u16.to_le_bytes()); // classIndex
    el.extend_from_slice(&0u16.to_le_bytes()); // styleIndex
    el.extend_from_slice(raw_attr_bytes);
    el
}

fn end_element_chunk(name_idx: u32) -> Vec<u8> {
    let mut end = Vec::with_capacity(24);
    end.extend_from_slice(&RES_XML_END_ELEMENT_TYPE.to_le_bytes());
    end.extend_from_slice(&16u16.to_le_bytes());
    end.extend_from_slice(&24u32.to_le_bytes());
    end.extend_from_slice(&1u32.to_le_bytes());
    end.extend_from_slice(&NO_INDEX.to_le_bytes());
    end.extend_from_slice(&NO_INDEX.to_le_bytes());
    end.extend_from_slice(&name_idx.to_le_bytes());
    end
}

fn wrap_axml(chunks: &[&[u8]]) -> Vec<u8> {
    let body_len: usize = chunks.iter().map(|c| c.len()).sum();
    let total = (8 + body_len) as u32;
    let mut out = Vec::with_capacity(8 + body_len);
    out.extend_from_slice(&RES_XML_TYPE.to_le_bytes());
    out.extend_from_slice(&8u16.to_le_bytes());
    out.extend_from_slice(&total.to_le_bytes());
    for c in chunks {
        out.extend_from_slice(c);
    }
    out
}

#[test]
fn issue_253_rejects_zero_attribute_size_amplification_without_oom() {
    let big = "A".repeat(32 * 1024);
    let pool = build_utf16_pool(&["manifest", &big]);
    let mut attr = Vec::with_capacity(20);
    attr.extend_from_slice(&NO_INDEX.to_le_bytes());
    attr.extend_from_slice(&1u32.to_le_bytes());
    attr.extend_from_slice(&1u32.to_le_bytes());
    attr.extend_from_slice(&8u16.to_le_bytes());
    attr.push(0);
    attr.push(TYPE_STRING);
    attr.extend_from_slice(&1u32.to_le_bytes());

    let start = start_element_chunk(0, 20, 0, u16::MAX, &attr);
    let end = end_element_chunk(0);
    let doc = wrap_axml(&[&pool, &start, &end]);

    let err = axml::parse(&doc).expect_err("attribute_size=0 must be rejected immediately");
    assert!(
        err.cause.contains("attributeSize"),
        "unexpected error cause: {}",
        err.cause
    );
}

#[test]
fn issue_253_rejects_attribute_table_reading_past_start_element_chunk() {
    let pool = build_utf16_pool(&["manifest", "package"]);
    let start = start_element_chunk(0, 20, 20, 1, &[]);
    let end = end_element_chunk(0);
    let extra = end_element_chunk(0);
    let doc = wrap_axml(&[&pool, &start, &end, &extra]);

    let err = axml::parse(&doc).expect_err("attribute table exceeding csize must be rejected");
    assert!(
        err.cause.contains("beyond chunk end"),
        "unexpected error cause: {}",
        err.cause
    );
}

#[test]
fn issue_253_rejects_unclosed_element_opened_after_root_closes_and_excess_depth() {
    let pool = build_utf16_pool(&["manifest", "extra"]);
    let root_start = start_element_chunk(0, 20, 20, 0, &[]);
    let root_end = end_element_chunk(0);
    let dangling_after_root = start_element_chunk(1, 20, 20, 0, &[]);
    let doc = wrap_axml(&[&pool, &root_start, &root_end, &dangling_after_root]);
    assert!(
        axml::parse(&doc).is_err(),
        "unclosed element opened after root closed must be rejected"
    );

    let unclosed_child = wrap_axml(&[&pool, &root_start, &dangling_after_root]);
    assert!(
        axml::parse(&unclosed_child).is_err(),
        "unclosed elements at EOF must be rejected"
    );

    let mut deep_chunks: Vec<Vec<u8>> = vec![pool];
    for _ in 0..1030 {
        deep_chunks.push(start_element_chunk(0, 20, 20, 0, &[]));
    }
    let refs: Vec<&[u8]> = deep_chunks.iter().map(Vec::as_slice).collect();
    let deep_doc = wrap_axml(&refs);
    let err = axml::parse(&deep_doc).expect_err("excessive nesting must be rejected");
    assert!(
        err.cause.contains("maximum depth"),
        "unexpected error cause: {}",
        err.cause
    );
}

#[test]
fn issue_253_iterative_clone_preserves_order_and_handles_200k_levels() {
    let mut tree = elem(
        "manifest",
        Vec::new(),
        vec![
            elem(
                "uses-permission",
                Vec::new(),
                vec![
                    elem("a1", Vec::new(), Vec::new()),
                    elem("a2", Vec::new(), Vec::new()),
                ],
            ),
            elem(
                "application",
                Vec::new(),
                vec![
                    elem("b1", Vec::new(), Vec::new()),
                    elem("b2", Vec::new(), Vec::new()),
                    elem("b3", Vec::new(), Vec::new()),
                ],
            ),
        ],
    );
    tree.text = Some("root-text".into());

    let cloned = tree.clone();
    assert_eq!(cloned.name, "manifest");
    assert_eq!(cloned.text.as_deref(), Some("root-text"));
    assert_eq!(cloned.children.len(), 2);
    assert_eq!(cloned.children[0].name, "uses-permission");
    assert_eq!(
        cloned.children[0]
            .children
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        vec!["a1", "a2"]
    );
    assert_eq!(cloned.children[1].name, "application");
    assert_eq!(
        cloned.children[1]
            .children
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        vec!["b1", "b2", "b3"]
    );

    let mut cur = elem("leaf", Vec::new(), Vec::new());
    for _ in 0..200_000 {
        cur = elem("n", Vec::new(), vec![cur]);
    }
    let deep_clone = cur.clone();
    assert_eq!(deep_clone.name, "n");
    assert_eq!(deep_clone.children.len(), 1);
    drop(deep_clone);
    drop(cur);
}

fn android_attr(name: &str, value: AttrValue, raw: Option<&str>) -> XmlAttribute {
    XmlAttribute {
        ns: Some(NS_ANDROID.to_string()),
        name: name.to_string(),
        raw: raw.map(str::to_owned),
        value,
        res_id: None,
    }
}

fn plain_attr(name: &str, s: &str) -> XmlAttribute {
    XmlAttribute {
        ns: None,
        name: name.to_string(),
        raw: Some(s.to_string()),
        value: AttrValue::String(s.to_string()),
        res_id: None,
    }
}

#[test]
fn issue_254_gles_version_decodes_major_minor_and_merges_multiple_queries_blocks() {
    let root = elem(
        "manifest",
        vec![plain_attr("package", "com.example.game")],
        vec![
            elem(
                "uses-feature",
                vec![android_attr(
                    "glEsVersion",
                    AttrValue::Int(0x0003_0000),
                    None,
                )],
                Vec::new(),
            ),
            elem(
                "uses-feature",
                vec![
                    android_attr(
                        "glEsVersion",
                        AttrValue::Int(0x0003_0002),
                        Some("0x00030002"),
                    ),
                    android_attr("required", AttrValue::Bool(false), None),
                ],
                Vec::new(),
            ),
            elem(
                "uses-feature",
                vec![android_attr(
                    "name",
                    AttrValue::String("android.hardware.vulkan.level".into()),
                    Some("android.hardware.vulkan.level"),
                )],
                Vec::new(),
            ),
            elem(
                "queries",
                Vec::new(),
                vec![
                    elem(
                        "package",
                        vec![android_attr(
                            "name",
                            AttrValue::String("org.fdroid.fdroid".into()),
                            Some("org.fdroid.fdroid"),
                        )],
                        Vec::new(),
                    ),
                    elem(
                        "intent",
                        Vec::new(),
                        vec![elem(
                            "action",
                            vec![android_attr(
                                "name",
                                AttrValue::String("android.intent.action.VIEW".into()),
                                Some("android.intent.action.VIEW"),
                            )],
                            Vec::new(),
                        )],
                    ),
                    elem("signature", Vec::new(), Vec::new()),
                ],
            ),
            elem(
                "queries",
                Vec::new(),
                vec![
                    elem(
                        "package",
                        vec![android_attr(
                            "name",
                            AttrValue::String("com.android.vending".into()),
                            Some("com.android.vending"),
                        )],
                        Vec::new(),
                    ),
                    elem(
                        "provider",
                        vec![android_attr(
                            "authorities",
                            AttrValue::String("com.example.billing.provider".into()),
                            Some("com.example.billing.provider"),
                        )],
                        Vec::new(),
                    ),
                    elem(
                        "intent",
                        Vec::new(),
                        vec![elem(
                            "action",
                            vec![android_attr(
                                "name",
                                AttrValue::String(
                                    "android.support.customtabs.action.CustomTabsService".into(),
                                ),
                                Some("android.support.customtabs.action.CustomTabsService"),
                            )],
                            Vec::new(),
                        )],
                    ),
                    elem("signature", Vec::new(), Vec::new()),
                ],
            ),
        ],
    );

    let doc = AxmlDocument {
        root,
        ..Default::default()
    };
    let mf = manifest::build(&doc, None).expect("valid manifest");

    assert_eq!(mf.uses_features.len(), 3);
    assert_eq!(mf.uses_features[0].gl_es_version.as_deref(), Some("3.0"));
    assert!(mf.uses_features[0].required);
    assert_eq!(mf.uses_features[1].gl_es_version.as_deref(), Some("3.2"));
    assert!(!mf.uses_features[1].required);
    assert_eq!(
        mf.uses_features[2].name.as_deref(),
        Some("android.hardware.vulkan.level")
    );

    assert_eq!(
        mf.queries.packages,
        vec!["org.fdroid.fdroid", "com.android.vending"]
    );
    assert_eq!(mf.queries.providers, vec!["com.example.billing.provider"]);
    assert_eq!(mf.queries.intents.len(), 2);
    assert_eq!(
        mf.queries.intents[0].actions,
        vec!["android.intent.action.VIEW"]
    );
    assert_eq!(
        mf.queries.intents[1].actions,
        vec!["android.support.customtabs.action.CustomTabsService"]
    );
    assert_eq!(mf.queries.signature_count, 2);
}
