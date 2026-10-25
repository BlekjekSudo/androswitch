//! Testes de contrato, paridade e robustez para `LazyDex`, `Strings::parse_lazy`
//! e `RawClassIndex` (ADR 008 Stage 1b).

use std::sync::Arc;

use rd_dex::dex::{adler32, ENDIAN_CONSTANT_LE};
use rd_dex::strings::NO_INDEX;
use rd_dex::{mutf8, Dex, DexBacking, LazyDex, RdError, RdResult};

struct MiniDexBuilder {
    strings: Vec<String>,
    types: Vec<u32>,
    protos: Vec<(u32, u32, Vec<u16>)>,
    fields: Vec<(u16, u16, u32)>,
    methods: Vec<(u16, u16, u32)>,
    classes: Vec<MiniClass>,
}

struct MiniClass {
    class_idx: u32,
    access_flags: u32,
    superclass_idx: u32,
    interfaces: Vec<u16>,
    source_file_idx: u32,
    direct_methods: Vec<(u32, u32, Vec<u16>)>,
}

impl MiniDexBuilder {
    fn new() -> Self {
        Self {
            strings: Vec::new(),
            types: Vec::new(),
            protos: Vec::new(),
            fields: Vec::new(),
            methods: Vec::new(),
            classes: Vec::new(),
        }
    }

    fn intern_str(&mut self, s: &str) -> u32 {
        if let Some(pos) = self.strings.iter().position(|x| x == s) {
            return pos as u32;
        }
        let idx = self.strings.len() as u32;
        self.strings.push(s.to_owned());
        idx
    }

    fn intern_type(&mut self, desc: &str) -> u32 {
        let sidx = self.intern_str(desc);
        if let Some(pos) = self.types.iter().position(|&x| x == sidx) {
            return pos as u32;
        }
        let idx = self.types.len() as u32;
        self.types.push(sidx);
        idx
    }

    fn intern_proto(&mut self, shorty: &str, ret_desc: &str, params: &[&str]) -> u16 {
        let shorty_idx = self.intern_str(shorty);
        let ret_idx = self.intern_type(ret_desc);
        let param_tids: Vec<u16> = params.iter().map(|p| self.intern_type(p) as u16).collect();
        let idx = self.protos.len() as u16;
        self.protos.push((shorty_idx, ret_idx, param_tids));
        idx
    }

    fn intern_field(&mut self, owner: &str, name: &str, ty: &str) -> u32 {
        let c = self.intern_type(owner) as u16;
        let t = self.intern_type(ty) as u16;
        let n = self.intern_str(name);
        let idx = self.fields.len() as u32;
        self.fields.push((c, t, n));
        idx
    }

    fn intern_method(
        &mut self,
        owner: &str,
        name: &str,
        shorty: &str,
        ret: &str,
        params: &[&str],
    ) -> u32 {
        let c = self.intern_type(owner) as u16;
        let p = self.intern_proto(shorty, ret, params);
        let n = self.intern_str(name);
        let idx = self.methods.len() as u32;
        self.methods.push((c, p, n));
        idx
    }

    fn add_class(
        &mut self,
        desc: &str,
        access_flags: u32,
        super_desc: &str,
        interfaces: &[&str],
        source: Option<&str>,
        direct_methods: Vec<(u32, u32, Vec<u16>)>,
    ) {
        let class_idx = self.intern_type(desc);
        let superclass_idx = self.intern_type(super_desc);
        let ifaces = interfaces
            .iter()
            .map(|i| self.intern_type(i) as u16)
            .collect();
        let source_file_idx = match source {
            Some(s) => self.intern_str(s),
            None => NO_INDEX,
        };
        self.classes.push(MiniClass {
            class_idx,
            access_flags,
            superclass_idx,
            interfaces: ifaces,
            source_file_idx,
            direct_methods,
        });
    }

    fn push_uleb(mut v: u32, out: &mut Vec<u8>) {
        loop {
            let b = (v & 0x7F) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                break;
            }
            out.push(b | 0x80);
        }
    }

    fn align4(buf: &mut Vec<u8>) {
        while buf.len() % 4 != 0 {
            buf.push(0);
        }
    }

    fn finish(&self) -> Vec<u8> {
        let mut buf = vec![0u8; 112];
        let string_ids_off = if self.strings.is_empty() {
            0u32
        } else {
            buf.len() as u32
        };
        buf.resize(buf.len() + self.strings.len() * 4, 0);

        let type_ids_off = if self.types.is_empty() {
            0u32
        } else {
            buf.len() as u32
        };
        for &sidx in &self.types {
            buf.extend_from_slice(&sidx.to_le_bytes());
        }

        let proto_ids_off = if self.protos.is_empty() {
            0u32
        } else {
            buf.len() as u32
        };
        let proto_table_pos = buf.len();
        buf.resize(buf.len() + self.protos.len() * 12, 0);

        let field_ids_off = if self.fields.is_empty() {
            0u32
        } else {
            buf.len() as u32
        };
        for &(c, t, n) in &self.fields {
            buf.extend_from_slice(&c.to_le_bytes());
            buf.extend_from_slice(&t.to_le_bytes());
            buf.extend_from_slice(&n.to_le_bytes());
        }

        let method_ids_off = if self.methods.is_empty() {
            0u32
        } else {
            buf.len() as u32
        };
        for &(c, p, n) in &self.methods {
            buf.extend_from_slice(&c.to_le_bytes());
            buf.extend_from_slice(&p.to_le_bytes());
            buf.extend_from_slice(&n.to_le_bytes());
        }

        let class_defs_off = if self.classes.is_empty() {
            0u32
        } else {
            buf.len() as u32
        };
        let class_table_pos = buf.len();
        buf.resize(buf.len() + self.classes.len() * 32, 0);

        let data_off = buf.len() as u32;

        // string_data_item
        for (i, s) in self.strings.iter().enumerate() {
            let off = buf.len() as u32;
            let pos = (string_ids_off as usize) + i * 4;
            buf[pos..pos + 4].copy_from_slice(&off.to_le_bytes());
            Self::push_uleb(s.encode_utf16().count() as u32, &mut buf);
            mutf8::push_mutf8(s, &mut buf);
            buf.push(0);
        }

        // proto type_lists
        for (i, &(shorty_idx, ret_idx, ref params)) in self.protos.iter().enumerate() {
            let params_off = if params.is_empty() {
                0u32
            } else {
                Self::align4(&mut buf);
                let off = buf.len() as u32;
                buf.extend_from_slice(&(params.len() as u32).to_le_bytes());
                for &tid in params {
                    buf.extend_from_slice(&tid.to_le_bytes());
                }
                off
            };
            let p = proto_table_pos + i * 12;
            buf[p..p + 4].copy_from_slice(&shorty_idx.to_le_bytes());
            buf[p + 4..p + 8].copy_from_slice(&ret_idx.to_le_bytes());
            buf[p + 8..p + 12].copy_from_slice(&params_off.to_le_bytes());
        }

        // class interfaces + code_items + class_data_items
        for (i, cls) in self.classes.iter().enumerate() {
            let ifaces_off = if cls.interfaces.is_empty() {
                0u32
            } else {
                Self::align4(&mut buf);
                let off = buf.len() as u32;
                buf.extend_from_slice(&(cls.interfaces.len() as u32).to_le_bytes());
                for &tid in &cls.interfaces {
                    buf.extend_from_slice(&tid.to_le_bytes());
                }
                off
            };

            let class_data_off = if cls.direct_methods.is_empty() {
                0u32
            } else {
                let mut method_entries = Vec::with_capacity(cls.direct_methods.len());
                for &(midx, mflags, ref units) in &cls.direct_methods {
                    let code_off = if units.is_empty() {
                        0u32
                    } else {
                        Self::align4(&mut buf);
                        let off = buf.len() as u32;
                        buf.extend_from_slice(&1u16.to_le_bytes()); // registers_size
                        buf.extend_from_slice(&0u16.to_le_bytes()); // ins_size
                        buf.extend_from_slice(&0u16.to_le_bytes()); // outs_size
                        buf.extend_from_slice(&0u16.to_le_bytes()); // tries_size
                        buf.extend_from_slice(&0u32.to_le_bytes()); // debug_info_off
                        buf.extend_from_slice(&(units.len() as u32).to_le_bytes());
                        for &u in units {
                            buf.extend_from_slice(&u.to_le_bytes());
                        }
                        off
                    };
                    method_entries.push((midx, mflags, code_off));
                }

                let cd_off = buf.len() as u32;
                Self::push_uleb(0, &mut buf); // static_fields
                Self::push_uleb(0, &mut buf); // instance_fields
                Self::push_uleb(method_entries.len() as u32, &mut buf); // direct_methods
                Self::push_uleb(0, &mut buf); // virtual_methods
                let mut prev = 0u32;
                for (midx, mflags, code_off) in method_entries {
                    Self::push_uleb(midx - prev, &mut buf);
                    Self::push_uleb(mflags, &mut buf);
                    Self::push_uleb(code_off, &mut buf);
                    prev = midx;
                }
                cd_off
            };

            let cpos = class_table_pos + i * 32;
            buf[cpos..cpos + 4].copy_from_slice(&cls.class_idx.to_le_bytes());
            buf[cpos + 4..cpos + 8].copy_from_slice(&cls.access_flags.to_le_bytes());
            buf[cpos + 8..cpos + 12].copy_from_slice(&cls.superclass_idx.to_le_bytes());
            buf[cpos + 12..cpos + 16].copy_from_slice(&ifaces_off.to_le_bytes());
            buf[cpos + 16..cpos + 20].copy_from_slice(&cls.source_file_idx.to_le_bytes());
            buf[cpos + 20..cpos + 24].copy_from_slice(&0u32.to_le_bytes());
            buf[cpos + 24..cpos + 28].copy_from_slice(&class_data_off.to_le_bytes());
            buf[cpos + 28..cpos + 32].copy_from_slice(&0u32.to_le_bytes());
        }

        Self::align4(&mut buf);
        let map_off = buf.len() as u32;
        buf.extend_from_slice(&1u32.to_le_bytes()); // 1 item: MapList
        buf.extend_from_slice(&0x1000u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&map_off.to_le_bytes());

        let file_size = buf.len() as u32;
        let data_size = file_size - data_off;
        buf[0..8].copy_from_slice(b"dex\n038\0");
        buf[32..36].copy_from_slice(&file_size.to_le_bytes());
        buf[36..40].copy_from_slice(&112u32.to_le_bytes());
        buf[40..44].copy_from_slice(&ENDIAN_CONSTANT_LE.to_le_bytes());
        buf[52..56].copy_from_slice(&map_off.to_le_bytes());
        buf[56..60].copy_from_slice(&(self.strings.len() as u32).to_le_bytes());
        buf[60..64].copy_from_slice(&string_ids_off.to_le_bytes());
        buf[64..68].copy_from_slice(&(self.types.len() as u32).to_le_bytes());
        buf[68..72].copy_from_slice(&type_ids_off.to_le_bytes());
        buf[72..76].copy_from_slice(&(self.protos.len() as u32).to_le_bytes());
        buf[76..80].copy_from_slice(&proto_ids_off.to_le_bytes());
        buf[80..84].copy_from_slice(&(self.fields.len() as u32).to_le_bytes());
        buf[84..88].copy_from_slice(&field_ids_off.to_le_bytes());
        buf[88..92].copy_from_slice(&(self.methods.len() as u32).to_le_bytes());
        buf[92..96].copy_from_slice(&method_ids_off.to_le_bytes());
        buf[96..100].copy_from_slice(&(self.classes.len() as u32).to_le_bytes());
        buf[100..104].copy_from_slice(&class_defs_off.to_le_bytes());
        buf[104..108].copy_from_slice(&data_size.to_le_bytes());
        buf[108..112].copy_from_slice(&data_off.to_le_bytes());

        let csum = adler32(&buf[12..]);
        buf[8..12].copy_from_slice(&csum.to_le_bytes());
        buf
    }
}

#[test]
fn lazy_dex_mount_and_section_parity_with_eager_dex() -> RdResult<()> {
    let mut b = MiniDexBuilder::new();
    let unicode_idx = b.intern_str("Olá-中-\u{1F600}-\0-fim");
    let fidx = b.intern_field("Lpkg/Alpha;", "COUNT", "I");
    let midx = b.intern_method("Lpkg/Alpha;", "compute", "II", "I", &["Ljava/lang/String;"]);
    b.add_class(
        "Lpkg/Alpha;",
        0x0001,
        "Ljava/lang/Object;",
        &["Ljava/lang/Runnable;"],
        Some("Alpha.java"),
        vec![(midx, 0x0009, vec![0x0012, 0x000f])], // const/4 v0, 0 ; return v0
    );

    let raw = b.finish();
    let eager = Dex::parse(raw.clone())?;
    let lazy_unv = Dex::parse_unverified(raw.clone())?;
    let lazy_alias = Dex::parse_lazy(raw.clone())?;

    // Também testa `DexBacking::Slice` com prefixo/sufixo sintéticos.
    let mut container = vec![0xAAu8; 37];
    let slice_off = container.len();
    container.extend_from_slice(&raw);
    container.extend_from_slice(&[0x55u8; 19]);
    let backing = DexBacking::from_slice_window(Arc::from(container), slice_off, raw.len())?;
    let mounted = LazyDex::mount(backing)?;
    let mounted_ver = LazyDex::mount_verified(DexBacking::from(raw))?;

    assert!(eager.header.checksum_ok);
    assert!(!lazy_unv.header.checksum_ok);
    assert!(lazy_unv.verify_checksum());
    assert!(!mounted.header().checksum_ok);
    assert!(mounted.verify_checksum());
    assert!(mounted_ver.header().checksum_ok);

    assert_eq!(mounted.strings_len(), eager.header.string_ids_size);
    assert_eq!(mounted.decoded_strings_count(), 0);
    assert_eq!(lazy_unv.strings.decoded_count(), 0);

    // `string_eq` compara MUTF-8 cru (inclusive surrogates + NUL C0 80) sem decodificar no cache!
    assert!(mounted.string_eq(unicode_idx, "Olá-中-\u{1F600}-\0-fim")?);
    assert!(!mounted.string_eq(unicode_idx, "Olá-中-\u{1F600}-X-fim")?);
    assert_eq!(mounted.decoded_strings_count(), 0);
    assert!(lazy_unv.string_eq(unicode_idx, "Olá-中-\u{1F600}-\0-fim")?);
    assert_eq!(lazy_unv.strings.decoded_count(), 0);

    // Paridade de strings decodificadas sob demanda
    for idx in 0..eager.header.string_ids_size {
        let want = eager.string(idx);
        assert_eq!(lazy_unv.string(idx), want);
        assert_eq!(lazy_alias.string(idx), want);
        assert_eq!(mounted.string(idx)?.as_ref(), want);
    }

    // Paridade de field_id, method_id, proto_params, class_def, class_data e code_item
    let ef = eager.field(fidx)?;
    let lf = mounted.field_id(fidx)?;
    assert_eq!(
        (lf.class_idx, lf.type_idx, lf.name_idx),
        (ef.class_idx, ef.type_idx, ef.name_idx)
    );

    let em = eager.method(midx)?;
    let lm = mounted.method_id(midx)?;
    assert_eq!(
        (lm.class_idx, lm.proto_idx, lm.name_idx),
        (em.class_idx, em.proto_idx, em.name_idx)
    );
    assert_eq!(
        mounted.proto_params(lm.proto_idx)?,
        eager.proto_params(em.proto_idx)?
    );

    let edef = eager
        .find_class("Lpkg/Alpha;")
        .ok_or_else(|| RdError::missing_entry("Lpkg/Alpha; in eager"))?;
    let ldef = mounted
        .find_class("Lpkg/Alpha;")
        .ok_or_else(|| RdError::missing_entry("Lpkg/Alpha; in LazyDex"))?;
    assert_eq!(ldef.access_flags, edef.access_flags);
    assert_eq!(mounted.interfaces(&ldef)?, eager.interfaces(edef)?);

    let lcd = mounted
        .class_data(&ldef)?
        .ok_or_else(|| RdError::missing_entry("class_data"))?;
    assert_eq!(lcd.direct_methods.len(), 1);
    let lcode = mounted
        .code(lcd.direct_methods[0].code_off)?
        .ok_or_else(|| RdError::missing_entry("code_item"))?;
    assert_eq!(lcode.insns.len(), 2);

    // Paridade byte-a-byte de disasm smali entre eager e lazy_unverified
    let smali_eager = rd_dex::disasm::render_class(&eager, 0)?;
    let smali_lazy = rd_dex::disasm::render_class(&lazy_unv, 0)?;
    assert_eq!(smali_lazy, smali_eager);
    Ok(())
}

#[test]
fn lazy_strings_decode_only_touched_pages() -> RdResult<()> {
    let mut b = MiniDexBuilder::new();
    for i in 0..600 {
        // Inclui escalar multibyte para exercitar as páginas do `PagedStringCache`.
        b.intern_str(&format!("str-página-{i:04}-λ"));
    }
    let raw = b.finish();
    let dex = Dex::parse_unverified(raw.clone())?;
    let lazy = LazyDex::mount(DexBacking::from(raw))?;

    assert_eq!(dex.strings.len(), 600);
    assert_eq!(dex.strings.decoded_count(), 0);
    assert_eq!(lazy.decoded_strings_count(), 0);

    assert_eq!(dex.string(7), "str-página-0007-λ");
    assert_eq!(dex.string(300), "str-página-0300-λ");
    assert_eq!(dex.string(599), "str-página-0599-λ");
    assert_eq!(dex.strings.decoded_count(), 3);

    assert_eq!(lazy.string(42)?.as_ref(), "str-página-0042-λ");
    assert_eq!(lazy.string(512)?.as_ref(), "str-página-0512-λ");
    assert_eq!(lazy.decoded_strings_count(), 2);
    Ok(())
}

#[test]
fn raw_class_index_preserves_first_definition_wins_and_unicode() -> RdResult<()> {
    let mut b = MiniDexBuilder::new();
    b.add_class(
        "Lpkg/Dup;",
        0x0001,
        "Ljava/lang/Object;",
        &[],
        None,
        Vec::new(),
    );
    b.add_class(
        "Lpkg/Café;",
        0x0011,
        "Ljava/lang/Object;",
        &[],
        None,
        Vec::new(),
    );
    // Segunda definição com mesmo descritor e flags distintas: a primeira (0x0001) deve vencer.
    b.add_class(
        "Lpkg/Dup;",
        0x0401,
        "Ljava/lang/Object;",
        &[],
        None,
        Vec::new(),
    );

    let raw = b.finish();
    let eager = Dex::parse(raw.clone())?;
    let lazy_dex = Dex::parse_unverified(raw.clone())?;
    let mounted = LazyDex::mount(DexBacking::from(raw))?;

    let d_eager = eager
        .find_class("Lpkg/Dup;")
        .ok_or_else(|| RdError::missing_entry("Dup eager"))?;
    let d_lazy = lazy_dex
        .find_class("Lpkg/Dup;")
        .ok_or_else(|| RdError::missing_entry("Dup lazy"))?;
    let d_mount = mounted
        .find_class("Lpkg/Dup;")
        .ok_or_else(|| RdError::missing_entry("Dup mount"))?;
    assert_eq!(d_eager.index, 0);
    assert_eq!(d_eager.access_flags, 0x0001);
    assert_eq!(d_lazy.index, 0);
    assert_eq!(d_lazy.access_flags, 0x0001);
    assert_eq!(d_mount.index, 0);
    assert_eq!(d_mount.access_flags, 0x0001);

    assert_eq!(
        lazy_dex
            .find_class("Lpkg/Café;")
            .map(|c| (c.index, c.access_flags)),
        Some((1, 0x0011))
    );
    assert_eq!(
        mounted
            .find_class("Lpkg/Café;")
            .map(|c| (c.index, c.access_flags)),
        Some((1, 0x0011))
    );
    assert!(lazy_dex.find_class("Lpkg/Missing;").is_none());
    assert!(mounted.find_class("Lpkg/Missing;").is_none());
    // Nem `find_class` nem misses precisam materializar strings no cache!
    assert_eq!(lazy_dex.strings.decoded_count(), 0);
    assert_eq!(mounted.decoded_strings_count(), 0);
    Ok(())
}

#[test]
fn malformed_lazy_inputs_return_structured_rd_error() -> RdResult<()> {
    // 1. Slice window fora do buffer
    let short_arc: Arc<[u8]> = Arc::from(vec![0u8; 32]);
    let win_err = DexBacking::from_slice_window(short_arc, 16, 32);
    assert!(win_err.is_err());

    // 2. DEX válido com terminador NUL corrompido em uma string não-ASCII:
    // o mount O(1) aceita o header/offsets, e `LazyDex::string` falha tipado ao tocar o índice.
    let mut b = MiniDexBuilder::new();
    let idx = b.intern_str("corromper-é");
    let mut raw = b.finish();
    // Aponta `string_ids[idx]` para um `string_data_item` sem NUL no fim do buffer.
    let bad_off = raw.len() as u32;
    raw.extend_from_slice(&[0x03, 0xC3, 0xA9, b'x']);
    let file_size = raw.len() as u32;
    raw[32..36].copy_from_slice(&file_size.to_le_bytes());
    let sids_off = u32::from_le_bytes([raw[60], raw[61], raw[62], raw[63]]) as usize;
    let entry_pos = sids_off + (idx as usize) * 4;
    raw[entry_pos..entry_pos + 4].copy_from_slice(&bad_off.to_le_bytes());
    let mounted = LazyDex::mount(DexBacking::from(raw.clone()))?;
    let err = mounted
        .string(idx)
        .err()
        .ok_or_else(|| RdError::parse("esperava RdError em string sem NUL"))?;
    assert_eq!(err.module_id, "rd-dex");

    let unv = Dex::parse_unverified(raw)?;
    assert!(unv.strings.try_get_with_data(idx, &unv.data).is_err());
    // Caminho tolerante de rendering devolve string vazia sem pânico.
    assert_eq!(unv.string(idx), "");
    Ok(())
}
