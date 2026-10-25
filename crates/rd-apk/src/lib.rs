//! rd-apk — parsing de APK: ZIP, AXML, resources.arsc, assinaturas e o modelo
//! completo do manifest (Apêndice F). Dependência de base de todo o RustyDroid.
//!
//! Contrato de erro: `RdError { code, cause, suggestion, module_id }` (Apêndice I).

pub mod arsc;
pub mod axml;
pub mod error;
pub mod manifest;
pub mod paged;
pub mod pool;
pub mod signing;
pub mod zip;

pub use error::{RdError, RdResult};
pub use manifest::Manifest;
pub use signing::SigningInfo;
pub use zip::Zip;

use serde::Serialize;
use std::path::Path;

/// Piso de API do RustyDroid (spec: minSdk 26 — Android 8.0 Oreo).
pub const FLOOR_MIN_SDK: u32 = 26;

pub const MODULE_ID: &str = "rd-apk";

#[derive(Debug, Clone, Serialize)]
pub struct Apk {
    /// Nome da entrada do manifest dentro do ZIP (constante na plataforma).
    pub manifest_entry: String,
    #[serde(flatten)]
    pub manifest: Manifest,
    #[serde(skip)]
    pub zip: Zip,
    #[serde(skip)]
    pub arsc: Option<arsc::Arsc>,
    #[serde(skip)]
    pub signing: SigningInfo,
    /// classes*.dex presentes.
    pub dex_files: Vec<String>,
    /// lib/<abi>/<name>.so
    pub native_libs: Vec<String>,
    /// ABIs distintas detectadas.
    pub abis: Vec<String>,
    /// Entradas sob assets/.
    pub assets: Vec<String>,
    /// Entradas sob res/ (contagem apenas — é grande).
    pub res_count: usize,
    pub resources_arsc: bool,
    pub zip_entry_count: usize,
    /// issue #84: nomes de entrada repetidos no ZIP (ambiguidade). Vazio nos
    /// APKs normais; quando presente, o inventário abaixo conta por nome
    /// ÚNICO e o `inspect` avisa explicitamente.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub zip_duplicates: Vec<String>,
    pub file_size_bytes: u64,
}

impl Apk {
    /// Teto de leitura whole-file (issue #38): o processo tem budget de
    /// 512 MB (piso E5) e panic=abort em release — acima deste teto o APK
    /// NÃO é materializado em memória; `open` roteia para o modo paginado
    /// (ADR 008 Stage 2, teto efetivo de 16 GiB do `PagedZip`).
    pub const MAX_INPUT_BYTES: u64 = 512 * 1024 * 1024;

    /// Abre e parseia um APK do disco.
    pub fn open(path: &Path) -> RdResult<Apk> {
        let meta = std::fs::metadata(path)
            .map_err(|e| RdError::io(&e, format!("stat {}", path.display())))?;
        if meta.len() > Self::MAX_INPUT_BYTES {
            // ADR 008 Stage 2: acima do teto whole-file, abre PAGINADO em
            // vez de recusar — só manifest/arsc/CD são lidos, payload sob
            // demanda com RSS travado. Antes era INVALID_FORMAT antes de
            // alocar (issue #38): um flagship de 820 MB nem inspecionava.
            let zip = Zip::open_paged(path)?;
            return Self::from_zip(zip, meta.len());
        }
        let data = std::fs::read(path)
            .map_err(|e| RdError::io(&e, format!("reading {}", path.display())))?;
        Self::from_bytes(data)
    }

    /// Parseia um APK já em memória.
    pub fn from_bytes(data: Vec<u8>) -> RdResult<Apk> {
        let file_size_bytes = data.len() as u64;
        Self::from_zip(Zip::parse(data)?, file_size_bytes)
    }

    fn from_zip(zip: Zip, file_size_bytes: u64) -> RdResult<Apk> {
        // 1. AndroidManifest.xml (binário)
        let manifest_entry = "AndroidManifest.xml";
        let manifest_bytes = zip.read(manifest_entry)?;
        let doc = axml::parse(&manifest_bytes)?;

        // 2. resources.arsc (opcional em APKs de teste, obrigatório em produção)
        let arsc = match zip.read("resources.arsc") {
            Ok(bytes) => Some(arsc::Arsc::parse(&bytes)?),
            Err(_) => None,
        };

        let manifest = manifest::build(&doc, arsc.as_ref())?;

        // 3. inventário de conteúdo
        let mut dex_files = Vec::new();
        let mut native_libs = Vec::new();
        let mut abis = Vec::new();
        let mut assets = Vec::new();
        let mut res_count = 0usize;
        // issue #84: inventário por nome ÚNICO — uma entrada repetida não
        // vira "dois DEX" (o que distorcia o resumo do `inspect`).
        let mut seen_names = std::collections::HashSet::new();
        for name in zip.names() {
            if !seen_names.insert(name) {
                continue;
            }
            if name.ends_with(".dex") && !name.contains('/') {
                dex_files.push(name.to_string());
            } else if let Some(rest) = name.strip_prefix("lib/") {
                if rest.ends_with(".so") {
                    native_libs.push(name.to_string());
                    if let Some(abi) = rest.split('/').next() {
                        if !abis.iter().any(|a| a == abi) {
                            abis.push(abi.to_string());
                        }
                    }
                }
            } else if let Some(rest) = name.strip_prefix("assets/") {
                if !rest.is_empty() {
                    assets.push(name.to_string());
                }
            } else if name.starts_with("res/") {
                res_count += 1;
            }
        }

        // 4. assinaturas — offset do CD único e canônico (issue #35);
        // no modo paginado só a janela do bloco é lida (detect_signing).
        let signing = detect_signing(&zip);

        let zip_entry_count = zip.entry_count();
        // issue #84: capturado antes de `zip` ser movido para a struct
        let zip_duplicates = zip.duplicates().to_vec();

        Ok(Apk {
            manifest_entry: manifest_entry.to_string(),
            manifest,
            resources_arsc: arsc.is_some(),
            zip,
            arsc,
            signing,
            dex_files,
            native_libs,
            abis,
            assets,
            res_count,
            zip_entry_count,
            zip_duplicates,
            file_size_bytes,
        })
    }

    /// Árvore AXML crua do manifest (Lei 2 — nada se perde).
    pub fn manifest_raw(&self) -> RdResult<axml::AxmlDocument> {
        let bytes = self.zip.read(&self.manifest_entry)?;
        axml::parse(&bytes)
    }

    /// Container ZIP (entradas, leitura, CRC).
    pub fn container(&self) -> &Zip {
        &self.zip
    }

    /// Verificação do piso de API (spec Apêndice A).
    /// Parse/inspect funcionam para qualquer minSdk; **execução** é que responde
    /// `BELOW_FLOOR` quando minSdk < 26.
    pub fn floor_status(&self) -> FloorStatus {
        if self.manifest.min_sdk < FLOOR_MIN_SDK {
            FloorStatus::BelowFloor {
                min_sdk: self.manifest.min_sdk,
                floor: FLOOR_MIN_SDK,
            }
        } else {
            FloorStatus::Ok
        }
    }

    /// Activities lançáveis (MAIN+LAUNCHER).
    pub fn launcher_activities(&self) -> Vec<&manifest::Component> {
        self.manifest
            .application
            .components
            .iter()
            .filter(|c| {
                c.is_launcher
                    && matches!(
                        c.kind,
                        manifest::ComponentKind::Activity | manifest::ComponentKind::ActivityAlias
                    )
            })
            .collect()
    }
}

/// Detecção de assinatura honrando a fonte do container: whole-file varre
/// os bytes em memória; paginado lê apenas a janela do bloco de assinatura
/// (que termina exatamente no `cd_offset`) sob demanda — nunca o arquivo
/// inteiro. `SigningInfo` não tem canal de erro: janela ilegível ou
/// adversarial degrada para "só v1" (META-INF), como um APK sem bloco v2.
fn detect_signing(zip: &Zip) -> SigningInfo {
    if let Some(bytes) = zip.whole_bytes() {
        // issue #35: o Zip já resolveu o EOCD (incluindo ZIP64) — reusar
        // o offset em vez de re-variar (o scan duplicado tinha off-by-one
        // em i==start e perdia o sentinela ZIP64 → APK v2-only reportado
        // como unsigned)
        return signing::detect(bytes, zip.cd_offset(), zip);
    }
    let cd = zip.cd_offset();
    // Sem bloco v2/v3 detectável, `detect` com dados vazios devolve só o
    // v1 (o guard `cd_offset <= data.len()` pula a varredura do bloco).
    let v1_only = || signing::detect(&[], cd, zip);
    if cd < 24 {
        return v1_only();
    }
    // Trailer do bloco: [block_size u64] … [block_size u64][magic 16B].
    let Ok(trailer) = zip.read_range(cd - 24, 24) else {
        return v1_only();
    };
    if &trailer[8..24] != signing::APK_SIG_BLOCK_MAGIC {
        return v1_only();
    }
    let Ok(size_le) = <[u8; 8]>::try_from(&trailer[0..8]) else {
        return v1_only();
    };
    let block_size = u64::from_le_bytes(size_le);
    // Janela = bloco inteiro + o u64 inicial. O teto protege o RSS de um
    // block_size adversarial (blocos reais têm ≪ 1 MiB mesmo com rotação
    // de chaves); acima dele, degrada para v1-only em vez de alocar.
    const MAX_SIG_WINDOW: u64 = 64 * 1024 * 1024;
    let Some(window_len) = block_size.checked_add(8) else {
        return v1_only();
    };
    if window_len > cd || window_len > MAX_SIG_WINDOW {
        return v1_only();
    }
    match zip.read_range(cd - window_len, window_len as usize) {
        // detect() indexa relativo ao cd_offset informado = fim da janela.
        Ok(window) => signing::detect(&window, window.len() as u64, zip),
        Err(_) => v1_only(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum FloorStatus {
    Ok,
    BelowFloor { min_sdk: u32, floor: u32 },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::axml::{
        NO_INDEX, RES_XML_END_ELEMENT_TYPE, RES_XML_START_ELEMENT_TYPE, RES_XML_TYPE, TYPE_STRING,
    };
    use crate::pool::RES_STRING_POOL_TYPE;
    use std::io::{Seek, SeekFrom, Write as _};

    /// AXML mínimo `<manifest package="…"/>` (só o que `manifest::build`
    /// exige). Pool: 0:"manifest" 1:"package" 2:<package>.
    fn minimal_manifest_axml(package: &str) -> Vec<u8> {
        fn utf16(s: &str) -> Vec<u8> {
            let mut v = Vec::new();
            v.extend((s.len() as u16).to_le_bytes());
            for u in s.encode_utf16() {
                v.extend(u.to_le_bytes());
            }
            v.extend(0u16.to_le_bytes());
            v
        }
        let strs = ["manifest", "package", package];
        let mut pool = Vec::new();
        pool.extend(RES_STRING_POOL_TYPE.to_le_bytes());
        pool.extend(28u16.to_le_bytes());
        pool.extend(0u32.to_le_bytes()); // size (patched)
        pool.extend((strs.len() as u32).to_le_bytes());
        pool.extend(0u32.to_le_bytes());
        pool.extend(0u32.to_le_bytes()); // flags (UTF-16)
        let strings_start = 28 + strs.len() * 4;
        pool.extend((strings_start as u32).to_le_bytes());
        pool.extend(0u32.to_le_bytes());
        let mut offs = Vec::new();
        let mut body = Vec::new();
        for s in strs {
            offs.push(body.len() as u32);
            body.extend(utf16(s));
        }
        pool.extend(offs.iter().flat_map(|o| o.to_le_bytes()));
        pool.extend(&body);
        let pool_size = pool.len() as u32;
        pool[4..8].copy_from_slice(&pool_size.to_le_bytes());

        let mut el = Vec::new();
        el.extend(RES_XML_START_ELEMENT_TYPE.to_le_bytes());
        el.extend(16u16.to_le_bytes());
        el.extend(0u32.to_le_bytes()); // size (patched)
        el.extend(1u32.to_le_bytes()); // line
        el.extend(NO_INDEX.to_le_bytes()); // comment
        el.extend(NO_INDEX.to_le_bytes()); // ns
        el.extend(0u32.to_le_bytes()); // name = "manifest"
        el.extend(20u16.to_le_bytes()); // attributeStart
        el.extend(20u16.to_le_bytes()); // attributeSize
        el.extend(1u16.to_le_bytes()); // attributeCount
        el.extend(0u16.to_le_bytes()); // idIndex
        el.extend(0u16.to_le_bytes()); // classIndex
        el.extend(0u16.to_le_bytes()); // styleIndex
        el.extend(NO_INDEX.to_le_bytes()); // attr ns
        el.extend(1u32.to_le_bytes()); // attr name = "package"
        el.extend(2u32.to_le_bytes()); // attr raw = <package>
        el.extend(8u16.to_le_bytes()); // typed value size
        el.push(0u8); // res0
        el.push(TYPE_STRING);
        el.extend(2u32.to_le_bytes()); // data = pool idx 2
        let el_size = el.len() as u32;
        el[4..8].copy_from_slice(&el_size.to_le_bytes());

        let mut end = Vec::new();
        end.extend(RES_XML_END_ELEMENT_TYPE.to_le_bytes());
        end.extend(16u16.to_le_bytes());
        end.extend(24u32.to_le_bytes());
        end.extend(1u32.to_le_bytes());
        end.extend(NO_INDEX.to_le_bytes());
        end.extend(NO_INDEX.to_le_bytes());
        end.extend(0u32.to_le_bytes());

        let total = (8 + pool.len() + el.len() + end.len()) as u32;
        let mut out = Vec::new();
        out.extend(RES_XML_TYPE.to_le_bytes());
        out.extend(8u16.to_le_bytes());
        out.extend(total.to_le_bytes());
        out.extend(pool);
        out.extend(el);
        out.extend(end);
        out
    }

    /// ZIP STORED mínimo multi-entrada (mesma receita dos testes do zip).
    fn stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use crate::zip::{crc32, CD_ENTRY_SIG, EOCD_SIG, LOCAL_HEADER_SIG};
        let mut v = Vec::new();
        let mut central = Vec::new();
        for (name, content) in entries {
            let crc = crc32(content);
            let local_offset = v.len() as u32;
            v.extend(LOCAL_HEADER_SIG.to_le_bytes());
            v.extend(20u16.to_le_bytes());
            v.extend(0u16.to_le_bytes());
            v.extend(0u16.to_le_bytes()); // STORED
            v.extend(0u16.to_le_bytes());
            v.extend(0u16.to_le_bytes());
            v.extend(crc.to_le_bytes());
            v.extend((content.len() as u32).to_le_bytes());
            v.extend((content.len() as u32).to_le_bytes());
            v.extend((name.len() as u16).to_le_bytes());
            v.extend(0u16.to_le_bytes());
            v.extend_from_slice(name.as_bytes());
            v.extend_from_slice(content);

            central.extend(CD_ENTRY_SIG.to_le_bytes());
            central.extend(20u16.to_le_bytes());
            central.extend(20u16.to_le_bytes());
            central.extend(0u16.to_le_bytes());
            central.extend(0u16.to_le_bytes());
            central.extend(0u16.to_le_bytes());
            central.extend(0u16.to_le_bytes());
            central.extend(crc.to_le_bytes());
            central.extend((content.len() as u32).to_le_bytes());
            central.extend((content.len() as u32).to_le_bytes());
            central.extend((name.len() as u16).to_le_bytes());
            central.extend(0u16.to_le_bytes());
            central.extend(0u16.to_le_bytes());
            central.extend(0u16.to_le_bytes());
            central.extend(0u16.to_le_bytes());
            central.extend(0u32.to_le_bytes());
            central.extend(local_offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd_start = v.len() as u32;
        v.extend(&central);
        let cd_size = v.len() as u32 - cd_start;
        v.extend(EOCD_SIG.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend((entries.len() as u16).to_le_bytes());
        v.extend((entries.len() as u16).to_le_bytes());
        v.extend(cd_size.to_le_bytes());
        v.extend(cd_start.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v
    }

    /// ADR 008 Stage 2: APK acima do teto whole-file (512 MiB) abre
    /// PAGINADO e parseia de verdade — antes era INVALID_FORMAT antes de
    /// alocar (um flagship de 820 MB nem inspecionava). O gap entre os
    /// payloads e o CD é um buraco esparso: disco ~zero, offsets reais.
    #[test]
    fn apk_open_over_wholefile_cap_routes_paged() {
        let axml = minimal_manifest_axml("com.test.paged");
        let bytes = stored_zip(&[
            ("AndroidManifest.xml", &axml),
            ("classes.dex", b"dex\n035\0fake"),
        ]);

        // contíguo/em memória: caminho whole-file de sempre
        let whole = Apk::from_bytes(bytes.clone()).unwrap();
        assert_eq!(whole.manifest.package, "com.test.paged");
        assert!(!whole.zip.is_paged());

        // esparso >512 MiB em disco: rota paginada
        let rd32 = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let cd_start = rd32(&bytes, bytes.len() - 22 + 16) as usize;
        let (head, tail) = bytes.split_at(cd_start);
        let mut tail = tail.to_vec();
        const TARGET_CD: u64 = 600 * 1024 * 1024;
        let tail_eocd = tail.len() - 22;
        tail[tail_eocd + 16..tail_eocd + 20].copy_from_slice(&(TARGET_CD as u32).to_le_bytes());

        let dir = std::env::temp_dir().join(format!("rd-apk-paged-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.apk");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(head).unwrap();
            f.seek(SeekFrom::Start(TARGET_CD)).unwrap();
            f.write_all(&tail).unwrap();
        }
        let apk = Apk::open(&path).unwrap();
        assert!(apk.zip.is_paged(), "acima do teto → paginado");
        assert!(apk.file_size_bytes > Apk::MAX_INPUT_BYTES);
        assert_eq!(apk.manifest.package, "com.test.paged");
        assert_eq!(apk.zip_entry_count, 2);
        assert_eq!(apk.dex_files.len(), 1);
        assert_eq!(apk.dex_files[0], "classes.dex");
        assert_eq!(apk.zip.read("classes.dex").unwrap(), b"dex\n035\0fake");
        std::fs::remove_dir_all(&dir).ok();
    }
}
