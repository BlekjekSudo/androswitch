//! rd-apk — parsing de APK: ZIP, AXML, resources.arsc, assinaturas e o modelo
//! completo do manifest (Apêndice F). Dependência de base de todo o RustyDroid.
//!
//! Contrato de erro: `RdError { code, cause, suggestion, module_id }` (Apêndice I).

pub mod arsc;
pub mod axml;
pub mod error;
pub mod manifest;
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
    /// 512 MB (piso E5) e panic=abort em release — input maior vira erro
    /// tipado ANTES de alocar, nunca abort sem contrato.
    pub const MAX_INPUT_BYTES: u64 = 512 * 1024 * 1024;

    /// Abre e parseia um APK do disco.
    pub fn open(path: &Path) -> RdResult<Apk> {
        let meta = std::fs::metadata(path)
            .map_err(|e| RdError::io(&e, format!("stat {}", path.display())))?;
        if meta.len() > Self::MAX_INPUT_BYTES {
            return Err(RdError::invalid_format(format!(
                "{} tem {} bytes (> teto de {} bytes): leitura recusada antes de alocar",
                path.display(),
                meta.len(),
                Self::MAX_INPUT_BYTES
            )));
        }
        let data = std::fs::read(path)
            .map_err(|e| RdError::io(&e, format!("reading {}", path.display())))?;
        Self::from_bytes(data)
    }

    /// Parseia um APK já em memória.
    pub fn from_bytes(data: Vec<u8>) -> RdResult<Apk> {
        let file_size_bytes = data.len() as u64;
        let zip = Zip::parse(data)?;

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

        // 4. assinaturas — offset do CD único e canônico
        let signing = {
            // issue #35: o Zip já resolveu o EOCD (incluindo ZIP64) — reusar
            // o offset em vez de re-variar (o scan duplicado tinha off-by-one
            // em i==start e perdia o sentinela ZIP64 → APK v2-only reportado
            // como unsigned)
            signing::detect(zip.raw_bytes(), zip.cd_offset(), &zip)
        };

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum FloorStatus {
    Ok,
    BelowFloor { min_sdk: u32, floor: u32 },
}
