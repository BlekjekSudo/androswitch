//! Testes de integração com .dex real (GH-05 — fixture NUNCA commitada).
//! Rodar: `RD_TEST_DEX=/tmp/fdroid-dex/classes.dex cargo test -p rd-dex`
//! Sem a env var, os testes passam trivialmente (CI-safe).
//!
//! Invariantes de parse completo verificados num dex de produção:
//! 1. header: magic/versão, adler32, file_size == bytes lidos;
//! 2. string_ids: contagem do header == entradas decodificáveis;
//! 3. map_list: cobertura — todo offset dentro do arquivo, contagens das
//!    seções de ids batem com o header, item do próprio map_list presente;
//! 4. todo class_def + class_data parseia; cada code_item é percorrido por
//!    completo sem OOB (o parse já falha tipado em overrun) e o re-encode das
//!    instruções decodificadas reproduz os bytes brutos (round-trip);
//! 5. debug_info, static_values, interfaces, call sites e anotações
//!    (directory → set → item) de todas as classes parseiam sem erro.

use rd_dex::annotations;
use rd_dex::code::Kind;
use rd_dex::opcode;
use rd_dex::{Dex, MapType};
use std::path::{Path, PathBuf};

fn dex_path() -> Option<PathBuf> {
    std::env::var_os("RD_TEST_DEX").map(|p| {
        let p = PathBuf::from(p);
        // caminhos relativos: resolver contra a raiz do workspace (CWD do teste é a crate)
        if p.is_relative() {
            let ws = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(Path::parent)
                .map(|w| w.join(&p))
                .unwrap_or_else(|| p.clone());
            if ws.exists() {
                ws
            } else {
                p
            }
        } else {
            p
        }
    })
}

#[test]
fn real_dex_header_and_map_coverage() {
    let Some(path) = dex_path() else {
        eprintln!("RD_TEST_DEX não definida — fixture ausente, skip (CI-safe)");
        return;
    };
    let data = std::fs::read(path).expect("ler .dex");
    let dex = Dex::parse(data.clone()).expect("parse completo do dex real");

    // 1. header
    assert_eq!(&dex.header.magic[0..4], b"dex\n", "magic");
    assert!(
        matches!(
            dex.header.version_str(),
            "035" | "037" | "038" | "039" | "041"
        ),
        "versão conhecida, got {}",
        dex.header.version_str()
    );
    assert!(dex.header.checksum_ok, "adler32 deve conferir num dex real");
    assert!(
        dex.verify_checksum(),
        "verify_checksum autoritativo confere"
    );
    // ADR 009: parse_unverified defere o adler (checksum_ok=false =
    // "não verificado") mas produz o mesmo Dex; verify_checksum confere.
    let duv = Dex::parse_unverified(data.clone()).expect("parse_unverified");
    assert!(
        !duv.header.checksum_ok,
        "unverified marca checksum_ok=false"
    );
    assert!(duv.verify_checksum(), "adler sob demanda confere");
    assert_eq!(duv.strings.len(), dex.strings.len());
    assert_eq!(
        dex.header.file_size as usize,
        data.len(),
        "file_size == tamanho do arquivo lido"
    );
    assert_eq!(dex.header.endian_tag, rd_dex::dex::ENDIAN_CONSTANT_LE);

    // 2. string_ids: header count == entradas percorridas
    assert_eq!(dex.strings.len() as u32, dex.header.string_ids_size);
    assert_eq!(dex.strings.offsets.len(), dex.strings.values.len());
    assert!(!dex.strings.is_empty(), "dex de app real tem strings");
    // todos os offsets de string_data dentro do arquivo
    for (i, &off) in dex.strings.offsets.iter().enumerate() {
        assert!(
            (off as usize) < data.len(),
            "string #{i} offset 0x{off:x} fora do arquivo"
        );
    }

    // 3. map_list: cobertura de todas as seções
    assert!(!dex.map_list.is_empty(), "dex real tem map_list");
    let map_self = dex
        .map_list
        .iter()
        .find(|m| m.map_type == MapType::MapList)
        .expect("map_list se auto-referencia");
    assert_eq!(
        map_self.offset, dex.header.map_off,
        "item MapList → map_off"
    );
    for item in &dex.map_list {
        assert!(
            (item.offset as usize) < data.len(),
            "map item {:?} offset 0x{:x} fora do arquivo",
            item.map_type,
            item.offset
        );
        if item.size == 0 && !matches!(item.map_type, MapType::Unknown(_)) {
            // seções presentes no map com size 0: tolerado só p/ CallSite/MethodHandle
            assert!(
                matches!(item.map_type, MapType::CallSiteId | MapType::MethodHandle),
                "{:?} com size 0",
                item.map_type
            );
        }
    }
    // contagens das seções de ids batem com o header
    let map_count = |t: MapType| {
        dex.map_list
            .iter()
            .find(|m| m.map_type == t)
            .map(|m| m.size)
    };
    assert_eq!(
        map_count(MapType::StringId),
        Some(dex.header.string_ids_size)
    );
    assert_eq!(map_count(MapType::TypeId), Some(dex.header.type_ids_size));
    assert_eq!(map_count(MapType::ProtoId), Some(dex.header.proto_ids_size));
    assert_eq!(map_count(MapType::FieldId), Some(dex.header.field_ids_size));
    assert_eq!(
        map_count(MapType::MethodId),
        Some(dex.header.method_ids_size)
    );
    assert_eq!(
        map_count(MapType::ClassDef),
        Some(dex.header.class_defs_size)
    );

    // contagens coerentes com um dex de app real
    assert!(dex.type_ids.len() > 100, "types: {}", dex.type_ids.len());
    assert!(
        dex.method_ids.len() > 1000,
        "methods: {}",
        dex.method_ids.len()
    );
    assert!(
        !dex.class_defs.is_empty(),
        "classes: {}",
        dex.class_defs.len()
    );
}

#[test]
fn real_dex_all_classes_code_roundtrip_and_annotations() {
    let Some(path) = dex_path() else {
        return;
    };
    let data = std::fs::read(path).expect("ler .dex");
    let dex = Dex::parse(data).expect("parse completo do dex real");

    let mut classes_with_data = 0usize;
    let mut methods_with_code = 0usize;
    let mut total_units = 0usize;
    let mut tries_total = 0usize;
    let mut debug_infos = 0usize;
    let mut annotation_items = 0usize;
    let mut classes_with_annotations = 0usize;
    let mut classes_with_static_values = 0usize;
    // 35c/45cc com count 0 são tolerados (d8 emite filled-new-array vazio)
    let mut count0_opcodes: Vec<&'static str> = Vec::new();

    for def in &dex.class_defs {
        let this_type = dex.type_str(def.class_idx);
        assert!(
            this_type.starts_with('L'),
            "descritor de classe: {this_type}"
        );

        // interfaces
        let interfaces = dex.interfaces(def).expect("type_list de interfaces");
        for &t in &interfaces {
            assert!((t as usize) < dex.type_ids.len(), "interface idx {t}");
        }

        // static_values (encoded_array)
        if def.static_values_off != 0 {
            let sv = dex.static_values(def).expect("static_values");
            classes_with_static_values += 1;
            assert!(
                !sv.is_empty(),
                "static_values de {this_type} vazio com offset != 0"
            );
        }

        // annotations_directory → sets → items
        if def.annotations_off != 0 {
            classes_with_annotations += 1;
            let dir = annotations::parse_annotations_directory(&dex.data, def.annotations_off)
                .expect("annotations_directory");
            let walk_set = |off: u32| -> Vec<u32> {
                annotations::parse_annotation_set_item(&dex.data, off).expect("annotation_set")
            };
            for off in walk_set(dir.class_annotations_off) {
                annotations::parse_annotation_item(&dex.data, off).expect("annotation_item");
                annotation_items += 1;
            }
            for &(_, off) in dir.field_annotations.iter().chain(&dir.method_annotations) {
                for aoff in walk_set(off) {
                    annotations::parse_annotation_item(&dex.data, aoff).expect("annotation_item");
                    annotation_items += 1;
                }
            }
            for &(_, off) in &dir.parameter_annotations {
                for aoff in
                    annotations::parse_annotation_set_ref_list(&dex.data, off).expect("ref_list")
                {
                    if aoff != 0 {
                        for ioff in walk_set(aoff) {
                            annotations::parse_annotation_item(&dex.data, ioff)
                                .expect("param annotation_item");
                            annotation_items += 1;
                        }
                    }
                }
            }
        }

        // class_data + code items
        if let Some(cd) = dex.class_data(def).expect("class_data") {
            classes_with_data += 1;
            let methods = cd.direct_methods.iter().chain(&cd.virtual_methods);
            for m in methods {
                if m.code_off == 0 {
                    continue; // abstract/native
                }
                let ci = dex
                    .code(m.code_off)
                    .expect("code_item")
                    .expect("algum code");
                methods_with_code += 1;
                total_units += ci.insns.len();
                tries_total += ci.tries.len();

                // stream percorrida por completo (decode_all já valida) e
                // round-trip byte-exato: re-encode == units brutos
                assert_eq!(
                    ci.reencode(),
                    ci.insns,
                    "round-trip do code_item @ 0x{:x} (classe {this_type})",
                    ci.offset
                );
                // debug info
                if ci.debug_info_off != 0 {
                    dex.debug_info(ci.debug_info_off).expect("debug_info");
                    debug_infos += 1;
                }
                // inventário de invokes com count 0 (tolerância do parser)
                for (_, insn) in &ci.instructions {
                    let zero = matches!(
                        &insn.kind,
                        Kind::Invoke35c { count: 0, .. } | Kind::Invoke45cc { count: 0, .. }
                    );
                    if zero {
                        count0_opcodes.push(opcode::info(insn.opcode).name);
                    }
                }
            }
        }
    }

    // call sites (encoded_array) — dex 038+ de apps com invokedynamic/desugar
    for &off in &dex.call_site_offsets {
        annotations::parse_encoded_array(&dex.data, off).expect("call_site_item");
    }

    eprintln!(
        "RD_TEST_DEX: classes={} com_class_data={} com_annotations={} com_static_values={} \
         metodos_com_codigo={} units={} tries={} debug_infos={} annotation_items={} call_sites={} \
         invokes_count0={:?}",
        dex.class_defs.len(),
        classes_with_data,
        classes_with_annotations,
        classes_with_static_values,
        methods_with_code,
        total_units,
        tries_total,
        debug_infos,
        annotation_items,
        dex.call_site_offsets.len(),
        count0_opcodes
    );

    assert!(classes_with_data > 0, "dex real tem classes com membros");
    assert!(
        methods_with_code > 100,
        "métodos com código: {methods_with_code}"
    );
    assert!(total_units > 1000, "code units: {total_units}");
}

#[test]
fn real_dex_disasm_contract_renders_smali() {
    let Some(path) = dex_path() else {
        return;
    };
    let data = std::fs::read(path).expect("ler .dex");
    let dex = Dex::parse(data).expect("parse");
    // contrato pós-M1 (issue #36): o disassembler está implementado —
    // render_class produz saída smali textual para a classe 0 sem pânico e
    // sem NOT_IMPLEMENTED; diretivas .class/.source/.method são esperadas
    let out = rd_dex::disasm::render_class(&dex, 0).expect("render class 0");
    assert!(
        out.contains(".class"),
        "saída smali deve conter diretiva .class"
    );
}
