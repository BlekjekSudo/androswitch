//! Fixtures de APK sintético para os testes do M3.2 (LayoutInflater):
//! ZIP (STORED) + string pool UTF-16 + AXML + ARSC mínimos-corretos,
//! gerados byte a byte — mesma política do `DexBuilder` (nenhum fixture
//! binário commitado, tudo determinístico e auditável).

// ────────────────────────────────────────────────────────────────────────────
// CRC-32 (IEEE, refletido) — o Zip do rd-apk verifica CRC em toda leitura.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

pub fn pad4(out: &mut Vec<u8>) {
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

// ────────────────────────────────────────────────────────────────────────────
// ResStringPool UTF-16 (flags = 0) — formato confirmado contra o parser
// (`pool.rs`) e o builder de teste do axml.rs.
pub fn string_pool_utf16(strs: &[&str]) -> Vec<u8> {
    fn utf16_unit(s: &str) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend((s.encode_utf16().count() as u16).to_le_bytes());
        for u in s.encode_utf16() {
            v.extend(u.to_le_bytes());
        }
        v.extend(0u16.to_le_bytes());
        v
    }
    let mut pool = Vec::new();
    pool.extend(0x0001u16.to_le_bytes()); // RES_STRING_POOL_TYPE
    pool.extend(28u16.to_le_bytes()); // headerSize
    pool.extend(0u32.to_le_bytes()); // size (patched)
    pool.extend((strs.len() as u32).to_le_bytes());
    pool.extend(0u32.to_le_bytes()); // styleCount
    pool.extend(0u32.to_le_bytes()); // flags (UTF-16)
    let strings_start = 28 + strs.len() * 4;
    pool.extend((strings_start as u32).to_le_bytes());
    pool.extend(0u32.to_le_bytes()); // stylesStart
    let mut offs = Vec::new();
    let mut body = Vec::new();
    for s in strs {
        offs.push(body.len() as u32);
        body.extend(utf16_unit(s));
    }
    pool.extend(offs.iter().flat_map(|o| o.to_le_bytes()));
    pool.extend(&body);
    pad4(&mut pool);
    let size = pool.len() as u32;
    pool[4..8].copy_from_slice(&size.to_le_bytes());
    pool
}

// ────────────────────────────────────────────────────────────────────────────
// AXML (XML binário) — subset de emissão usado pelos layouts/manifest de teste.

pub const NO_INDEX: u32 = 0xFFFF_FFFF;
pub const NS_ANDROID: &str = "http://schemas.android.com/apk/res/android";

pub const TYPE_REFERENCE: u8 = 0x01;
pub const TYPE_STRING: u8 = 0x03;
pub const TYPE_DIMENSION: u8 = 0x05;
pub const TYPE_FIRST_INT: u8 = 0x10;
pub const TYPE_BOOLEAN: u8 = 0x12;

pub enum AxVal {
    /// string com raw no pool (tipo STRING)
    Str(String),
    /// referência de recurso (ex.: @+id/tv → 0x7f030000)
    Ref(u32),
    Int(i32),
    Bool(bool),
    /// dimensão crua Res_value (mantissa<<8 | unit) — ex.: (80<<8)|1 = 80dp
    Dim {
        raw: u32,
    },
}

pub struct AxAttr {
    pub android: bool,
    pub name: String,
    pub val: AxVal,
}

pub fn a(android: bool, name: &str, val: AxVal) -> AxAttr {
    AxAttr {
        android,
        name: name.to_string(),
        val,
    }
}

#[derive(Default)]
pub struct AxNode {
    pub name: String,
    pub attrs: Vec<AxAttr>,
    pub children: Vec<AxNode>,
}

impl AxNode {
    pub fn new(name: &str, attrs: Vec<AxAttr>, children: Vec<AxNode>) -> Self {
        AxNode {
            name: name.to_string(),
            attrs,
            children,
        }
    }
}

fn idx_of(strs: &mut Vec<String>, s: &str) -> u32 {
    if let Some(i) = strs.iter().position(|x| x == s) {
        return i as u32;
    }
    strs.push(s.to_string());
    (strs.len() - 1) as u32
}

fn collect_strings(node: &AxNode, strs: &mut Vec<String>) {
    idx_of(strs, &node.name);
    for at in &node.attrs {
        if at.android {
            // uri + prefix (a URI fica no slot 0 — os attrs android apontam p/ ela)
            if !strs.contains(&NS_ANDROID.to_string()) {
                strs.insert(0, NS_ANDROID.to_string());
                strs.insert(1, "android".to_string());
            }
        }
        idx_of(strs, &at.name);
        if let AxVal::Str(s) = &at.val {
            idx_of(strs, s);
        }
    }
    for c in &node.children {
        collect_strings(c, strs);
    }
}

fn emit_node(node: &AxNode, strs: &[String], out: &mut Vec<u8>) {
    // todas as strings já foram internadas por collect_strings
    let ix =
        |s: &str| -> u32 { strs.iter().position(|x| x == s).expect("string internada") as u32 };
    let ns_idx = |android: bool| -> u32 {
        if android {
            ix(NS_ANDROID)
        } else {
            NO_INDEX
        }
    };
    let mut el = Vec::new();
    el.extend(0x0102u16.to_le_bytes()); // RES_XML_START_ELEMENT_TYPE
    el.extend(16u16.to_le_bytes()); // headerSize (8 + line + comment)
    el.extend(0u32.to_le_bytes()); // size (patched)
    el.extend(1u32.to_le_bytes()); // line
    el.extend(NO_INDEX.to_le_bytes()); // comment
    el.extend(NO_INDEX.to_le_bytes()); // ns do elemento
    el.extend(ix(&node.name).to_le_bytes());
    el.extend(20u16.to_le_bytes()); // attributeStart (a partir do ns)
    el.extend(20u16.to_le_bytes()); // attributeSize
    el.extend((node.attrs.len() as u16).to_le_bytes());
    el.extend(0u16.to_le_bytes()); // idIndex
    el.extend(0u16.to_le_bytes()); // classIndex
    el.extend(0u16.to_le_bytes()); // styleIndex
    for at in &node.attrs {
        el.extend(ns_idx(at.android).to_le_bytes());
        el.extend(ix(&at.name).to_le_bytes());
        match &at.val {
            AxVal::Str(s) => {
                let i = ix(s);
                el.extend(i.to_le_bytes()); // raw
                el.extend(8u16.to_le_bytes()); // Res_value.size
                el.push(0u8); // res0
                el.push(TYPE_STRING);
                el.extend(i.to_le_bytes()); // data
            }
            AxVal::Ref(r) => {
                el.extend(NO_INDEX.to_le_bytes());
                el.extend(8u16.to_le_bytes());
                el.push(0u8);
                el.push(TYPE_REFERENCE);
                el.extend(r.to_le_bytes());
            }
            AxVal::Int(i) => {
                el.extend(NO_INDEX.to_le_bytes());
                el.extend(8u16.to_le_bytes());
                el.push(0u8);
                el.push(TYPE_FIRST_INT);
                el.extend((*i as u32).to_le_bytes());
            }
            AxVal::Bool(b) => {
                el.extend(NO_INDEX.to_le_bytes());
                el.extend(8u16.to_le_bytes());
                el.push(0u8);
                el.push(TYPE_BOOLEAN);
                el.extend((*b as u32).to_le_bytes());
            }
            AxVal::Dim { raw } => {
                el.extend(NO_INDEX.to_le_bytes());
                el.extend(8u16.to_le_bytes());
                el.push(0u8);
                el.push(TYPE_DIMENSION);
                el.extend(raw.to_le_bytes());
            }
        }
    }
    pad4(&mut el);
    let el_size = el.len() as u32;
    el[4..8].copy_from_slice(&el_size.to_le_bytes());
    out.extend(&el);

    for c in &node.children {
        emit_node(c, strs, out);
    }

    let mut end = Vec::new();
    end.extend(0x0103u16.to_le_bytes()); // RES_XML_END_ELEMENT_TYPE
    end.extend(16u16.to_le_bytes());
    end.extend(24u32.to_le_bytes());
    end.extend(1u32.to_le_bytes()); // line
    end.extend(NO_INDEX.to_le_bytes()); // comment
    end.extend(NO_INDEX.to_le_bytes()); // ns
    end.extend(ix(&node.name).to_le_bytes());
    out.extend(&end);
}

/// Monta um documento AXML binário completo (doc header + pool + chunks de
/// namespace + resource map + elementos) — issue #47: antes sem START/END
/// namespace e sem resource map (0x0180), o attr.res_id era sempre None e
/// caminhos indexados por res_id ficavam sem teste.
pub fn build_axml(root: &AxNode) -> Vec<u8> {
    let mut strs: Vec<String> = vec![NS_ANDROID.to_string(), "android".to_string()];
    collect_strings(root, &mut strs);

    // nomes de attrs android (recebem id no resource map — determinístico)
    let mut android_attr_names: Vec<String> = Vec::new();
    collect_android_attr_names(root, &mut android_attr_names);

    let mut out = Vec::new();
    out.extend(0x0003u16.to_le_bytes()); // RES_XML_TYPE
    out.extend(8u16.to_le_bytes());
    out.extend(0u32.to_le_bytes()); // total size (patched)
    out.extend(string_pool_utf16(
        &strs.iter().map(String::as_str).collect::<Vec<_>>(),
    ));

    // START_NAMESPACE — o aapt abre antes do root (uri slot 0, prefix slot 1)
    out.extend(0x0100u16.to_le_bytes()); // RES_XML_START_NAMESPACE_TYPE
    out.extend(16u16.to_le_bytes()); // headerSize
    out.extend(24u32.to_le_bytes()); // size
    out.extend(1u32.to_le_bytes()); // line
    out.extend(NO_INDEX.to_le_bytes()); // comment
    out.extend(1u32.to_le_bytes()); // prefix idx ("android")
    out.extend(0u32.to_le_bytes()); // uri idx (NS_ANDROID)

    // resource map (0x0180) — um u32 por string do pool; id plausível
    // (0x0101_0000+n) para nomes de attrs android, 0 para os demais
    out.extend(0x0180u16.to_le_bytes()); // RES_XML_RESOURCE_MAP_TYPE
    out.extend(8u16.to_le_bytes()); // headerSize
    out.extend((8 + strs.len() as u32 * 4).to_le_bytes()); // size
    let mut attr_n = 0u32;
    for s in &strs {
        if android_attr_names.contains(s) {
            attr_n += 1;
            out.extend((0x0101_0000u32 | attr_n).to_le_bytes());
        } else {
            out.extend(0u32.to_le_bytes());
        }
    }

    emit_node(root, &strs, &mut out);

    // END_NAMESPACE — o aapt fecha depois do root
    out.extend(0x0101u16.to_le_bytes()); // RES_XML_END_NAMESPACE_TYPE
    out.extend(16u16.to_le_bytes());
    out.extend(24u32.to_le_bytes());
    out.extend(1u32.to_le_bytes());
    out.extend(NO_INDEX.to_le_bytes());
    out.extend(1u32.to_le_bytes()); // prefix
    out.extend(0u32.to_le_bytes()); // uri

    let total = out.len() as u32;
    out[4..8].copy_from_slice(&total.to_le_bytes());
    out
}

fn collect_android_attr_names(node: &AxNode, out: &mut Vec<String>) {
    for at in &node.attrs {
        if at.android && !out.contains(&at.name.to_string()) {
            out.push(at.name.to_string());
        }
    }
    for c in &node.children {
        collect_android_attr_names(c, out);
    }
}

/// Manifest mínimo válido (root `<manifest package=…>` com application,
/// uses-sdk e intent-filter MAIN/LAUNCHER — issue #47: fidelidade ao aapt;
/// min/target=1 preserva o comportamento dos testes que assumem o default).
pub fn build_manifest_axml(package: &str, activity_desc: &str) -> Vec<u8> {
    build_manifest_axml_sdk(package, activity_desc, 1, 1)
}

/// Variante com SDKs explícitos (MCP/agent: fixtures acima do piso 26).
pub fn build_manifest_axml_sdk(
    package: &str,
    activity_desc: &str,
    min_sdk: i32,
    target_sdk: i32,
) -> Vec<u8> {
    let root = AxNode::new(
        "manifest",
        vec![
            a(false, "package", AxVal::Str(package.to_string())),
            a(true, "versionCode", AxVal::Int(1)),
        ],
        vec![
            AxNode::new(
                "uses-sdk",
                vec![
                    a(true, "minSdkVersion", AxVal::Int(min_sdk)),
                    a(true, "targetSdkVersion", AxVal::Int(target_sdk)),
                ],
                vec![],
            ),
            AxNode::new(
                "application",
                vec![],
                vec![AxNode::new(
                    "activity",
                    vec![a(true, "name", AxVal::Str(activity_desc.to_string()))],
                    vec![AxNode::new(
                        "intent-filter",
                        vec![],
                        vec![
                            AxNode::new(
                                "action",
                                vec![a(
                                    true,
                                    "name",
                                    AxVal::Str("android.intent.action.MAIN".to_string()),
                                )],
                                vec![],
                            ),
                            AxNode::new(
                                "category",
                                vec![a(
                                    true,
                                    "name",
                                    AxVal::Str("android.intent.category.LAUNCHER".to_string()),
                                )],
                                vec![],
                            ),
                        ],
                    )],
                )],
            ),
        ],
    );
    build_axml(&root)
}

/// Variante com `<activity-alias>` (issue #109): a `<activity>` fica sem filtro
/// e o alias carrega o intent-filter MAIN/LAUNCHER — padrão comum do Android
/// para trocar ícone/rótulo sem renomear a activity. Passe um `target_desc`
/// ausente do DEX para exercitar o caminho de erro estruturado.
pub fn build_manifest_axml_sdk_alias(
    package: &str,
    activity_desc: &str,
    alias_desc: &str,
    target_desc: &str,
    min_sdk: i32,
    target_sdk: i32,
) -> Vec<u8> {
    let root = AxNode::new(
        "manifest",
        vec![
            a(false, "package", AxVal::Str(package.to_string())),
            a(true, "versionCode", AxVal::Int(1)),
        ],
        vec![
            AxNode::new(
                "uses-sdk",
                vec![
                    a(true, "minSdkVersion", AxVal::Int(min_sdk)),
                    a(true, "targetSdkVersion", AxVal::Int(target_sdk)),
                ],
                vec![],
            ),
            AxNode::new(
                "application",
                vec![],
                vec![
                    AxNode::new(
                        "activity",
                        vec![a(true, "name", AxVal::Str(activity_desc.to_string()))],
                        vec![],
                    ),
                    AxNode::new(
                        "activity-alias",
                        vec![
                            a(true, "name", AxVal::Str(alias_desc.to_string())),
                            a(true, "targetActivity", AxVal::Str(target_desc.to_string())),
                        ],
                        vec![AxNode::new(
                            "intent-filter",
                            vec![],
                            vec![
                                AxNode::new(
                                    "action",
                                    vec![a(
                                        true,
                                        "name",
                                        AxVal::Str("android.intent.action.MAIN".to_string()),
                                    )],
                                    vec![],
                                ),
                                AxNode::new(
                                    "category",
                                    vec![a(
                                        true,
                                        "name",
                                        AxVal::Str("android.intent.category.LAUNCHER".to_string()),
                                    )],
                                    vec![],
                                ),
                            ],
                        )],
                    ),
                ],
            ),
        ],
    );
    build_axml(&root)
}

/// Variante do manifest com `<uses-permission>` declarados (M5).
pub fn build_manifest_axml_sdk_perms(
    package: &str,
    activity_desc: &str,
    min_sdk: i32,
    target_sdk: i32,
    uses_permissions: &[&str],
) -> Vec<u8> {
    let mut top = vec![AxNode::new(
        "uses-sdk",
        vec![
            a(true, "minSdkVersion", AxVal::Int(min_sdk)),
            a(true, "targetSdkVersion", AxVal::Int(target_sdk)),
        ],
        vec![],
    )];
    for p in uses_permissions {
        top.push(AxNode::new(
            "uses-permission",
            vec![a(true, "name", AxVal::Str((*p).to_string()))],
            vec![],
        ));
    }
    let application = AxNode::new(
        "application",
        vec![],
        vec![AxNode::new(
            "activity",
            vec![a(true, "name", AxVal::Str(activity_desc.to_string()))],
            vec![AxNode::new(
                "intent-filter",
                vec![],
                vec![
                    AxNode::new(
                        "action",
                        vec![a(
                            true,
                            "name",
                            AxVal::Str("android.intent.action.MAIN".to_string()),
                        )],
                        vec![],
                    ),
                    AxNode::new(
                        "category",
                        vec![a(
                            true,
                            "name",
                            AxVal::Str("android.intent.category.LAUNCHER".to_string()),
                        )],
                        vec![],
                    ),
                ],
            )],
        )],
    );
    let mut children = top;
    children.push(application);
    let root = AxNode::new(
        "manifest",
        vec![
            a(false, "package", AxVal::Str(package.to_string())),
            a(true, "versionCode", AxVal::Int(1)),
        ],
        children,
    );
    build_axml(&root)
}

// ────────────────────────────────────────────────────────────────────────────
// ZIP (STORED) — local headers + central directory + EOCD, CRC verificado.

pub struct ZipFix {
    entries: Vec<(String, Vec<u8>)>,
}

impl Default for ZipFix {
    fn default() -> Self {
        Self::new()
    }
}

impl ZipFix {
    pub fn new() -> Self {
        ZipFix {
            entries: Vec::new(),
        }
    }

    pub fn add(&mut self, name: &str, data: Vec<u8>) {
        self.entries.push((name.to_string(), data));
    }

    /// ZIP64 EOCD count fixture (small offsets/sizes only, not >4 GiB payloads).
    pub fn build_zip64(self) -> Vec<u8> {
        let count = self.entries.len() as u64;
        let mut bytes = self.build();
        let eocd_offset = bytes.len() - 22;
        let cd_size = u32::from_le_bytes(
            bytes[eocd_offset + 12..eocd_offset + 16]
                .try_into()
                .unwrap(),
        ) as u64;
        let cd_offset = u32::from_le_bytes(
            bytes[eocd_offset + 16..eocd_offset + 20]
                .try_into()
                .unwrap(),
        ) as u64;
        let mut eocd = bytes.split_off(eocd_offset);
        let zip64_offset = bytes.len() as u64;
        bytes.extend_from_slice(&0x0606_4b50u32.to_le_bytes());
        bytes.extend_from_slice(&44u64.to_le_bytes());
        bytes.extend_from_slice(&[45, 0, 45, 0]);
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(&cd_size.to_le_bytes());
        bytes.extend_from_slice(&cd_offset.to_le_bytes());
        bytes.extend_from_slice(&0x0706_4b50u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&zip64_offset.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        eocd[8..12].copy_from_slice(&[0xff; 4]);
        bytes.extend_from_slice(&eocd);
        bytes
    }

    pub fn build(self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in &self.entries {
            let off = out.len() as u32;
            let crc = crc32(data);
            out.extend(0x0403_4B50u32.to_le_bytes()); // local file header
            out.extend(20u16.to_le_bytes()); // version needed
            out.extend(0u16.to_le_bytes()); // flags
            out.extend(0u16.to_le_bytes()); // method = STORED
                                            // 0x9F00 = 19:56:00 (hora 19 = 10011b, min 56 = 111000b, seg 0) — issue #47
            out.extend(0x9F00u16.to_le_bytes()); // mod time
            out.extend(0x5D41u16.to_le_bytes()); // mod date (2026-10-01)
            out.extend(crc.to_le_bytes());
            out.extend((data.len() as u32).to_le_bytes()); // compressed
            out.extend((data.len() as u32).to_le_bytes()); // uncompressed
            out.extend((name.len() as u16).to_le_bytes());
            out.extend(0u16.to_le_bytes()); // extra len
            out.extend(name.as_bytes());
            out.extend(data);

            central.extend(0x0201_4B50u32.to_le_bytes());
            central.extend(20u16.to_le_bytes()); // version made by
            central.extend(20u16.to_le_bytes()); // version needed
            central.extend(0u16.to_le_bytes()); // flags
            central.extend(0u16.to_le_bytes()); // method
            central.extend(0x9F00u16.to_le_bytes()); // time
            central.extend(0x5D41u16.to_le_bytes()); // date
            central.extend(crc.to_le_bytes());
            central.extend((data.len() as u32).to_le_bytes());
            central.extend((data.len() as u32).to_le_bytes());
            central.extend((name.len() as u16).to_le_bytes());
            central.extend(0u16.to_le_bytes()); // extra
            central.extend(0u16.to_le_bytes()); // comment
            central.extend(0u16.to_le_bytes()); // disk start
            central.extend(0u16.to_le_bytes()); // internal attrs
            central.extend(0u32.to_le_bytes()); // external attrs
            central.extend(off.to_le_bytes());
            central.extend(name.as_bytes());
        }
        let cd_off = out.len() as u32;
        let cd_size = central.len() as u32;
        out.extend(&central);
        out.extend(0x0605_4B50u32.to_le_bytes()); // EOCD
        out.extend(0u16.to_le_bytes()); // disk
        out.extend(0u16.to_le_bytes()); // cd disk
        out.extend((self.entries.len() as u16).to_le_bytes());
        out.extend((self.entries.len() as u16).to_le_bytes());
        out.extend(cd_size.to_le_bytes());
        out.extend(cd_off.to_le_bytes());
        out.extend(0u16.to_le_bytes()); // comment len
        out
    }
}

// ────────────────────────────────────────────────────────────────────────────
// resources.arsc — tabela com 1 pacote (id 0x7f) e N tipos, config default.

pub enum ArscVal {
    /// valor string no pool global do pacote
    Str(String),
    Int(i32),
    Ref(u32),
    /// issue #61: dimensão crua Res_value (mantissa<<8 | unit) — ex. (80<<8)|1 = 80dp
    Dim(u32),
    /// issue #61: boolean (TYPE_BOOLEAN, data 0/1)
    Bool(bool),
}

pub struct ArscEntry {
    pub name: String,
    pub val: ArscVal,
    /// issue #47: config da entrada — None = default; Some("pt") = locale.
    pub lang: Option<String>,
}

#[derive(Default)]
pub struct ArscFix {
    values: Vec<String>,
    types: Vec<(&'static str, Vec<ArscEntry>)>,
}

impl ArscFix {
    pub fn new() -> Self {
        Default::default()
    }

    fn slot(&mut self, t: &'static str) -> &mut Vec<ArscEntry> {
        if self.types.iter().any(|(n, _)| *n == t) {
            let pos = self.types.iter().position(|(n, _)| *n == t).unwrap();
            return &mut self.types[pos].1;
        }
        self.types.push((t, Vec::new()));
        let last = self.types.len() - 1;
        &mut self.types[last].1
    }

    /// resid de um recurso já registrado (type_id = ordem de inserção, 1-based).
    pub fn resid_of(&self, t: &str, name: &str) -> u32 {
        let ti = self.types.iter().position(|(n, _)| *n == t).expect("tipo") as u32 + 1;
        let es = &self.types.iter().find(|(n, _)| *n == t).unwrap().1;
        let ei = es.iter().position(|e| e.name == name).expect("entrada") as u32;
        0x7F00_0000 | (ti << 16) | ei
    }

    pub fn add_layout(&mut self, name: &str) -> u32 {
        self.slot("layout").push(ArscEntry {
            name: name.to_string(),
            val: ArscVal::Int(0),
            lang: None,
        });
        self.resid_of("layout", name)
    }

    pub fn add_string(&mut self, name: &str, value: &str) -> u32 {
        if !self.values.iter().any(|v| v == value) {
            self.values.push(value.to_string());
        }
        self.slot("string").push(ArscEntry {
            name: name.to_string(),
            val: ArscVal::Str(value.to_string()),
            lang: None,
        });
        self.resid_of("string", name)
    }

    /// issue #47: string com config de locale — o mesmo res_id ganha 2
    /// valores (default + específica) em chunks ResTable_type distintos.
    pub fn add_string_lang(&mut self, name: &str, value: &str, lang: &str) -> u32 {
        if !self.values.iter().any(|v| v == value) {
            self.values.push(value.to_string());
        }
        self.slot("string").push(ArscEntry {
            name: name.to_string(),
            val: ArscVal::Str(value.to_string()),
            lang: Some(lang.to_string()),
        });
        self.resid_of("string", name)
    }

    pub fn add_id(&mut self, name: &str) -> u32 {
        self.slot("id").push(ArscEntry {
            name: name.to_string(),
            val: ArscVal::Int(0),
            lang: None,
        });
        self.resid_of("id", name)
    }

    /// issue #61: `@dimen/<name>` (dp) — o inflate resolve por referência.
    pub fn add_dimen_dp(&mut self, name: &str, dp: i32) -> u32 {
        self.slot("dimen").push(ArscEntry {
            name: name.to_string(),
            val: ArscVal::Dim(((dp as u32) << 8) | 1), // unit 1 = DIP
            lang: None,
        });
        self.resid_of("dimen", name)
    }

    /// issue #61: `@bool/<name>` — o inflate resolve por referência.
    pub fn add_bool(&mut self, name: &str, value: bool) -> u32 {
        self.slot("bool").push(ArscEntry {
            name: name.to_string(),
            val: ArscVal::Bool(value),
            lang: None,
        });
        self.resid_of("bool", name)
    }

    pub fn build(&self) -> Vec<u8> {
        // strings de tipo e chave (ordem de inserção → type_id/entry idx estáveis)
        let type_names: Vec<&str> = self.types.iter().map(|(n, _)| *n).collect();
        let mut keys: Vec<&str> = Vec::new();
        for (_, es) in &self.types {
            for e in es {
                if !keys.contains(&e.name.as_str()) {
                    keys.push(&e.name);
                }
            }
        }
        let type_pool = string_pool_utf16(&type_names);
        let key_pool = string_pool_utf16(&keys);
        let global_pool =
            string_pool_utf16(&self.values.iter().map(String::as_str).collect::<Vec<_>>());

        // chunks de tipo (um por type): typeSpec + variantes de config
        let mut type_chunks = Vec::new();
        for (i, (_, entries)) in self.types.iter().enumerate() {
            let type_id = (i + 1) as u8;
            // issue #47: entry index é por NAME ÚNICO — o mesmo name em 2
            // configs = o MESMO res_id com valores por config (como o aapt
            // emite); antes cada add_* criava index novo e a config pt virava
            // resid DIFERENTE em vez de segunda config do mesmo id
            let mut names: Vec<&str> = Vec::new();
            for e in entries {
                if !names.contains(&e.name.as_str()) {
                    names.push(&e.name);
                }
            }
            let mut config_default = vec![0u8; 28];
            config_default[0..4].copy_from_slice(&28u32.to_le_bytes()); // size
            let mut config_lang = vec![0u8; 28];
            config_lang[0..4].copy_from_slice(&28u32.to_le_bytes());
            config_lang[8..10].copy_from_slice(b"pt"); // language (ResTable_config@8)
            let cheader: u16 = 20 + 28; // header + config (28 bytes)
            let has_lang = entries.iter().any(|e| e.lang.is_some());

            // issue #47: typeSpec (0x0202) antes do type — o aapt real emite um
            // por type id (flags de entrada); o parser pula, mas o fixture
            // agora é fiel ao binário real
            let mut spec = Vec::new();
            spec.extend(0x0202u16.to_le_bytes()); // RES_TABLE_TYPE_SPEC
            spec.extend(16u16.to_le_bytes()); // headerSize
            spec.extend((16 + names.len() as u32 * 4).to_le_bytes()); // size
            spec.push(type_id);
            spec.push(0u8); // res0
            spec.extend(0u16.to_le_bytes()); // reserved
            spec.extend((names.len() as u32).to_le_bytes()); // entryCount
            for _ in &names {
                spec.extend(0u32.to_le_bytes()); // flags (simples)
            }
            type_chunks.push(spec);

            // variantes de config: default + locale (NO_ENTRY onde ausente)
            let mut variants: Vec<(&[u8], bool)> = vec![(&config_default, false)];
            if has_lang {
                variants.push((&config_lang, true));
            }
            for (config, want_lang) in variants {
                let mut offsets = Vec::new();
                let mut body = Vec::new();
                for n in &names {
                    let Some(e) = entries
                        .iter()
                        .find(|e| e.name == *n && e.lang.is_some() == want_lang)
                    else {
                        offsets.push(0xFFFF_FFFFu32); // NO_ENTRY
                        continue;
                    };
                    offsets.push(body.len() as u32);
                    let key_idx = keys.iter().position(|k| *k == e.name).unwrap() as u32;
                    body.extend(8u16.to_le_bytes()); // ResTable_entry.size
                    body.extend(0u16.to_le_bytes()); // flags (simples)
                    body.extend(key_idx.to_le_bytes());
                    let (dtype, data) = match &e.val {
                        ArscVal::Str(s) => (
                            TYPE_STRING,
                            self.values.iter().position(|v| v == s).unwrap() as u32,
                        ),
                        ArscVal::Int(i) => (TYPE_FIRST_INT, *i as u32),
                        ArscVal::Ref(r) => (TYPE_REFERENCE, *r),
                        ArscVal::Dim(raw) => (TYPE_DIMENSION, *raw),
                        ArscVal::Bool(b) => (TYPE_BOOLEAN, *b as u32),
                    };
                    body.extend(8u16.to_le_bytes()); // Res_value.size
                    body.push(0u8); // res0
                    body.push(dtype);
                    body.extend(data.to_le_bytes());
                }
                pad4(&mut body);
                let mut c = Vec::new();
                c.extend(0x0201u16.to_le_bytes()); // RES_TABLE_TYPE_CHUNK
                c.extend(cheader.to_le_bytes()); // headerSize
                c.extend(0u32.to_le_bytes()); // size (patched)
                c.push(type_id);
                c.push(0u8); // flags (não-sparse)
                c.extend(0u16.to_le_bytes()); // reserved
                c.extend((names.len() as u32).to_le_bytes()); // entryCount
                let entries_start = cheader as u32 + (offsets.len() as u32) * 4;
                c.extend(entries_start.to_le_bytes());
                c.extend_from_slice(config);
                c.extend(offsets.iter().flat_map(|o| o.to_le_bytes()));
                c.extend(&body);
                let csz = c.len() as u32;
                c[4..8].copy_from_slice(&csz.to_le_bytes());
                type_chunks.push(c);
            }
        }

        // pacote (header de 288 bytes + pools + tipos)
        let mut pkg = vec![0u8; 288];
        pkg[0..2].copy_from_slice(&0x0200u16.to_le_bytes());
        pkg[2..4].copy_from_slice(&288u16.to_le_bytes());
        // size patcheado no fim
        pkg[8..12].copy_from_slice(&0x7Fu32.to_le_bytes()); // package id
        let name = "com.test";
        for (i, u) in name.encode_utf16().chain(std::iter::once(0)).enumerate() {
            pkg[12 + i * 2..14 + i * 2].copy_from_slice(&u.to_le_bytes());
        }
        let type_strings_off = 288u32;
        let key_strings_off = 288u32 + type_pool.len() as u32;
        pkg[268..272].copy_from_slice(&type_strings_off.to_le_bytes());
        pkg[272..276].copy_from_slice(&0u32.to_le_bytes()); // lastPublicType
        pkg[276..280].copy_from_slice(&key_strings_off.to_le_bytes());
        pkg[280..284].copy_from_slice(&0u32.to_le_bytes()); // lastPublicKey
        pkg.extend(&type_pool);
        pkg.extend(&key_pool);
        for c in &type_chunks {
            pkg.extend(c);
        }
        pad4(&mut pkg);
        let pkg_size = pkg.len() as u32;
        pkg[4..8].copy_from_slice(&pkg_size.to_le_bytes());

        // tabela
        let mut out = Vec::new();
        out.extend(0x0002u16.to_le_bytes()); // RES_TABLE_TYPE
        out.extend(12u16.to_le_bytes()); // headerSize
        out.extend(0u32.to_le_bytes()); // total size (patched)
        out.extend(1u32.to_le_bytes()); // packageCount
        out.extend(&global_pool);
        out.extend(&pkg);
        let total = out.len() as u32;
        out[4..8].copy_from_slice(&total.to_le_bytes());
        // issue #47: alinhamento de 4 bytes do resources.arsc no ZIP (requisito
        // de APK real Android 11+) — o size do chunk NÃO inclui o padding
        pad4(&mut out);
        out
    }
}

/// APK sintético completo: manifest + arsc + entradas extras (layouts).
pub fn build_apk(
    package: &str,
    activity_desc: &str,
    arsc: Vec<u8>,
    extras: Vec<(&str, Vec<u8>)>,
) -> Vec<u8> {
    build_apk_sdk(package, activity_desc, arsc, extras, 1, 1)
}

/// Variante com min/target SDK explícitos (piso API 26 para o agente MCP).
pub fn build_apk_sdk(
    package: &str,
    activity_desc: &str,
    arsc: Vec<u8>,
    extras: Vec<(&str, Vec<u8>)>,
    min_sdk: i32,
    target_sdk: i32,
) -> Vec<u8> {
    build_apk_sdk_perms(
        package,
        activity_desc,
        arsc,
        extras,
        min_sdk,
        target_sdk,
        &[],
    )
}

/// Variante com `<uses-permission>` e `<permission>` declarados — issue #69:
/// permissões customizadas que o próprio APK define (o Android real as modela
/// a partir do manifesto, não de uma tabela externa).
pub fn build_manifest_axml_sdk_perms_declared(
    package: &str,
    activity_desc: &str,
    min_sdk: i32,
    target_sdk: i32,
    uses_permissions: &[&str],
    declared_permissions: &[(&str, &str)],
) -> Vec<u8> {
    let mut top = vec![AxNode::new(
        "uses-sdk",
        vec![
            a(true, "minSdkVersion", AxVal::Int(min_sdk)),
            a(true, "targetSdkVersion", AxVal::Int(target_sdk)),
        ],
        vec![],
    )];
    for p in uses_permissions {
        top.push(AxNode::new(
            "uses-permission",
            vec![a(true, "name", AxVal::Str((*p).to_string()))],
            vec![],
        ));
    }
    for (name, level) in declared_permissions {
        top.push(AxNode::new(
            "permission",
            vec![
                a(true, "name", AxVal::Str((*name).to_string())),
                a(true, "protectionLevel", AxVal::Str((*level).to_string())),
            ],
            vec![],
        ));
    }
    let application = AxNode::new(
        "application",
        vec![],
        vec![AxNode::new(
            "activity",
            vec![a(true, "name", AxVal::Str(activity_desc.to_string()))],
            vec![AxNode::new(
                "intent-filter",
                vec![],
                vec![AxNode::new(
                    "action",
                    vec![a(
                        true,
                        "name",
                        AxVal::Str("android.intent.action.MAIN".to_string()),
                    )],
                    vec![],
                )],
            )],
        )],
    );
    let mut children = top;
    children.push(application);
    let root = AxNode::new(
        "manifest",
        vec![
            a(false, "package", AxVal::Str(package.to_string())),
            a(true, "versionCode", AxVal::Int(1)),
        ],
        children,
    );
    build_axml(&root)
}

/// Variante com `uses-permission` declarados (MCP M5: ciclo de permissões
/// grant/clear no registro da sessão).
pub fn build_apk_sdk_perms(
    package: &str,
    activity_desc: &str,
    arsc: Vec<u8>,
    extras: Vec<(&str, Vec<u8>)>,
    min_sdk: i32,
    target_sdk: i32,
    uses_permissions: &[&str],
) -> Vec<u8> {
    let mut z = ZipFix::new();
    z.add(
        "AndroidManifest.xml",
        build_manifest_axml_sdk_perms(
            package,
            activity_desc,
            min_sdk,
            target_sdk,
            uses_permissions,
        ),
    );
    z.add("resources.arsc", arsc);
    for (name, data) in extras {
        z.add(name, data);
    }
    z.build()
}

/// issue #69: APK com `<permission>` declarados (permissões do próprio app).
pub fn build_apk_sdk_perms_declared(
    package: &str,
    activity_desc: &str,
    arsc: Vec<u8>,
    min_sdk: i32,
    target_sdk: i32,
    uses_permissions: &[&str],
    declared_permissions: &[(&str, &str)],
) -> Vec<u8> {
    let mut z = ZipFix::new();
    z.add(
        "AndroidManifest.xml",
        build_manifest_axml_sdk_perms_declared(
            package,
            activity_desc,
            min_sdk,
            target_sdk,
            uses_permissions,
            declared_permissions,
        ),
    );
    z.add("resources.arsc", arsc);
    z.build()
}

// issue #40: o AxNode do fixture tem o MESMO risco de drop recursivo que o
// XmlElement do parser (o teste de AXML profundo constrói a árvore com este
// tipo) — drop iterativo com stack explícita.
impl Drop for AxNode {
    fn drop(&mut self) {
        let mut stack = Vec::new();
        stack.extend(std::mem::take(&mut self.children));
        while let Some(mut node) = stack.pop() {
            stack.extend(std::mem::take(&mut node.children));
        }
    }
}

/// Static APK inventory, with no executable payload; used to exercise metadata
/// resource limits without loading a VM or committing a giant binary fixture.
pub fn build_large_inventory_apk(asset_count: usize) -> Vec<u8> {
    let mut zip = ZipFix::new();
    zip.add(
        "AndroidManifest.xml",
        build_manifest_axml_sdk(
            "com.example.inventory",
            "com.example.inventory.Main",
            26,
            36,
        ),
    );
    for i in 0..asset_count {
        zip.add(&format!("assets/{i}"), Vec::new());
    }
    zip.build_zip64()
}
