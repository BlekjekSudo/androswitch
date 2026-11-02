//! resources.arsc — parse mínimo porém correto (ResTable) + resolução de recursos.
//!
//! Escopo M0: resolver **strings** (labels) e **nomes** (`@type/key`) por resource id,
//! com preferência pela config default. Valores complexos são marcados (`complex`)
//! sem desdobrar bag/maps — suficiente para o inspect e para o motor de permissões.
//!
//! **Lazy (ADR 014):** o parse valida a estrutura e constrói um ÍNDICE leve
//! (`resid → chunks de tipo`); as entradas pesadas (`ResEntry`, bags, strings) são
//! materializadas SOB DEMANDA por resid nos `resolve_*` e memoizadas num cache
//! limitado. O cold path do ADR 008 toca <2–5% dos recursos; o resultado de cada
//! resolução é byte-idêntico ao eager (mesma ordem de walk, mesma seleção de
//! config, mesmos guards — Lei 4). O buffer inteiro fica retido atrás de um
//! `Arc` (Lei 2: offsets crus preservados; clones baratos).

use crate::axml::ComplexUnit;
use crate::error::{RdError, RdResult};
use crate::pool::StringPool;
use crate::sync::Mutex;
use alloc::sync::Arc;
use alloc::{borrow::ToOwned, collections::BTreeMap, format, string::String, vec::Vec};
use core::sync::atomic::{AtomicUsize, Ordering};

pub const RES_TABLE_TYPE: u16 = 0x0002;
pub const RES_TABLE_PACKAGE_TYPE: u16 = 0x0200;
pub const RES_TABLE_TYPE_SPEC: u16 = 0x0202;
pub const RES_TABLE_TYPE_CHUNK: u16 = 0x0201;

const FLAG_SPARSE: u8 = 0x01;
const FLAG_OFFSET16: u8 = 0x02;
const ENTRY_FLAG_COMPLEX: u16 = 0x0001;
#[path = "arsc_identifier.rs"]
mod identifier;
pub use identifier::{identifier_name, identifier_number};

/// Capacidade do cache de entradas materializadas (ADR 014): limita o pico de
/// RSS em APKs hostis; ao estourar, o cache é reconstruído do zero (bounded,
/// determinístico — só afeta QUANDO decodificar, nunca O QUÊ retornar).
const ENTRY_CACHE_CAP: usize = 8192;

/// Orçamento de materialização (issue #331 item 2), compartilhado entre pacotes
/// de um mesmo `Arsc`: entradas + itens de bag, e bytes de strings clonadas.
/// Lazy: é consumido no ACESSO (`entries_for`); ao estourar, as entradas
/// restantes deixam de ser materializadas (resolve → `None`), sem pânico.
const MAX_TOTAL_ARSC_ENTRIES: usize = 4_000_000; // entradas + itens de bag
const MAX_TOTAL_ARSC_STRING_BYTES: usize = 128 * 1024 * 1024;

#[inline]
fn u16_at(d: &[u8], off: usize) -> RdResult<u16> {
    d.get(off..off + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| RdError::parse(format!("arsc: truncated u16 @ {off}")))
}

#[inline]
fn u32_at(d: &[u8], off: usize) -> RdResult<u32> {
    d.get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| RdError::parse(format!("arsc: truncated u32 @ {off}")))
}

/// Config simplificada — só o que o M0 precisa para escolher a variante certa.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResConfigInfo {
    pub language: Option<String>,
    pub country: Option<String>,
    pub density: Option<u16>,
    pub api_level: Option<u16>,
    /// Raw configuration contains qualifiers beyond locale, density and SDK.
    /// Existing inspection selectors retain their behavior; consumers requiring
    /// exact supported configuration matching can explicitly decline these.
    pub unsupported_qualifiers: bool,
}

impl ResConfigInfo {
    pub fn is_default(&self) -> bool {
        self.language.is_none() && self.density.is_none_or(|d| d == 0 || d == 160)
    }
    fn specificity(&self) -> u8 {
        let mut s = 0;
        if self.language.is_some() {
            s += 2;
        }
        if self.country.is_some() {
            s += 1;
        }
        s
    }
}

/// Uma entrada de um bag/mapa de recurso (por exemplo, um atributo de style).
#[derive(Debug, Clone, PartialEq)]
pub struct ResMapEntry {
    /// Nome/resource id do atributo dentro do bag.
    pub name: u32,
    pub data_type: u8,
    pub data: u32,
    /// Pré-resolvida quando `data_type == 3` (STRING → pool global).
    pub string: Option<String>,
}

/// Uma entrada de recurso resolvida para uma config.
#[derive(Debug, Clone)]
pub struct ResEntry {
    pub config: ResConfigInfo,
    pub data_type: u8,
    pub data: u32,
    /// Pré-resolvida quando `data_type == 3` (STRING → pool global).
    pub string: Option<String>,
    pub complex: bool,
    /// Parent style resource id, when this entry is a complex bag.
    pub parent: Option<u32>,
    /// Resource map values when this entry is a complex bag (style/array/etc.).
    pub bag: Option<Vec<ResMapEntry>>,
    /// Índice no key string pool (para `resolve_name`).
    pub key_idx: u32,
}

/// Referência leve a um chunk `ResTable_type` (ADR 014): offsets ABSOLUTOS no
/// buffer do ARSC inteiro — a materialização usa exatamente os mesmos endereços
/// (e os mesmos guards) que o parse eager usava, garantindo paridade byte-a-byte.
#[derive(Debug, Clone)]
struct TypeChunkRef {
    /// Fim absoluto do chunk (`p + csize`): todo guard de leitura usa este limite.
    chunk_end: usize,
    /// Início absoluto da tabela de offsets: `p + headerSize`.
    table_start: usize,
    /// Início absoluto das entradas: `p + entriesStart` (ou `table_start` se inválido).
    entries_abs: usize,
    config: ResConfigInfo,
    flags: u8,
}

/// Soma `add` em `counter` se o resultado couber em `limit` (CAS, sem
/// `fetch_update`/`try_update`: funciona em qualquer toolchain do MSRV ao stable).
/// `false` = estouraria o limite; o contador fica como estava.
pub(crate) fn try_add_capped(counter: &AtomicUsize, add: usize, limit: usize) -> bool {
    let mut cur = counter.load(Ordering::Relaxed);
    loop {
        let Some(next) = cur.checked_add(add).filter(|n| *n <= limit) else {
            return false;
        };
        match counter.compare_exchange_weak(cur, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return true,
            Err(actual) => cur = actual,
        }
    }
}

/// Orçamento compartilhado de materialização (ver `MAX_TOTAL_ARSC_*`).
/// Atômico: `entries_for` é `&self`, e pacotes podem ser usados por threads.
#[derive(Debug, Default)]
struct ArscBudget {
    entries: AtomicUsize,
    string_bytes: AtomicUsize,
}

impl ArscBudget {
    /// Cobra 1 entrada/item + os bytes da string clonada (se houver).
    /// `false` = orçamento estourado: o chamador deixa de materializar.
    fn charge(&self, string: Option<&str>) -> bool {
        if !try_add_capped(&self.entries, 1, MAX_TOTAL_ARSC_ENTRIES) {
            return false;
        }
        match string {
            None => true,
            Some(s) => try_add_capped(&self.string_bytes, s.len(), MAX_TOTAL_ARSC_STRING_BYTES),
        }
    }
}

/// Pacote com índice + materialização sob demanda (ADR 014). O `Clone` é
/// manual: buffers/índice são `Arc` (compartilhados, baratos); o cache de
/// entradas renasce vazio (mesmo conteúdo, decodificação por instância).
#[derive(Debug)]
pub struct Package {
    pub id: u8,
    pub name: String,
    /// Added to each on-disk type id; absent in legacy 284-byte headers.
    pub type_id_offset: u8,
    pub type_strings: StringPool,
    pub key_strings: StringPool,
    /// Clone do pool global (Arc compartilhado — barato): a pré-resolução de
    /// strings das entradas materializadas precisa dele, e o pacote fica
    /// autocontido (`resolve_name`/`entries_for` sem parâmetro extra).
    global: StringPool,
    /// Índice leve (ADR 014): chunks de tipo do pacote + pares
    /// (resid, chunk_idx) ORDENADOS. O append durante o parse é O(1); a
    /// busca por resid é binária. A ordem (resid, chunk_idx) reproduz a
    /// ordem de walk do eager — decisiva para o tie-breaking do `min_by_key`.
    chunks: Arc<Vec<TypeChunkRef>>,
    /// Ordenados por (resid, chunk_idx, slot): `slot` é o índice da tabela
    /// (de onde o offset é lido na materialização). Ordem = ordem do walk eager.
    pairs: Arc<Vec<(u32, u32, u32)>>,
    /// Orçamento compartilhado pelo `Arsc` inteiro (Arc).
    budget: Arc<ArscBudget>,
    /// Buffer do ARSC inteiro compartilhado (Arc) — Leis 2 e clones baratos.
    raw: Arc<Vec<u8>>,
    /// Cache de entradas materializadas por resid (cap `ENTRY_CACHE_CAP`;
    /// reset ao estourar). Estado POR INSTÂNCIA (nunca global): `Mutex` direto,
    /// sem `OnceLock` — `Mutex::new(BTreeMap::new())` não aloca, então a
    /// inicialização preguiçosa não comprava nada. Lock envenenado = cache
    /// ignorado (re-decodifica), nunca panic.
    cache: Mutex<BTreeMap<u32, Arc<Vec<ResEntry>>>>,
}

impl Clone for Package {
    /// Clone manual (ADR 014): buffer e índice são `Arc` (compartilhados,
    /// O(1)); o cache de entradas renasce vazio — mesmo conteúdo, decodifica
    /// de novo por instância (determinístico).
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            name: self.name.clone(),
            type_id_offset: self.type_id_offset,
            type_strings: self.type_strings.clone(),
            key_strings: self.key_strings.clone(),
            global: self.global.clone(),
            chunks: Arc::clone(&self.chunks),
            pairs: Arc::clone(&self.pairs),
            budget: Arc::clone(&self.budget),
            raw: Arc::clone(&self.raw),
            cache: Mutex::new(BTreeMap::new()),
        }
    }
}

/// The identity of an entry in its owning table, independent of value/config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceName<'a> {
    pub package: &'a str,
    pub resource_type: &'a str,
    pub entry: &'a str,
}

#[derive(Debug, Clone, Default)]
pub struct Arsc {
    pub global_strings: StringPool,
    pub packages: Vec<Package>,
}

impl Package {
    /// Construtor sintético (testes/testsupport): entradas já materializadas
    /// viram o cache pré-povoado; o índice fica vazio (nada a materializar).
    pub fn synthetic(
        id: u8,
        name: String,
        type_strings: StringPool,
        key_strings: StringPool,
        entries: BTreeMap<u32, Vec<ResEntry>>,
    ) -> Self {
        let cache = Mutex::new(
            entries
                .into_iter()
                .map(|(k, v)| (k, Arc::new(v)))
                .collect::<BTreeMap<_, _>>(),
        );
        Package {
            id,
            name,
            type_id_offset: 0,
            type_strings,
            key_strings,
            global: StringPool::default(),
            chunks: Arc::new(Vec::new()),
            pairs: Arc::new(Vec::new()),
            budget: Arc::new(ArscBudget::default()),
            raw: Arc::new(Vec::new()),
            cache,
        }
    }

    /// Entradas de um resid, materializando sob demanda (ADR 014) —
    /// snapshot em cache (`Arc` compartilha com resoluções futuras).
    /// Mesma ordem/conteúdo que o eager produzia no parse (mesmos guards,
    /// mesma pré-resolução de strings pela pool global). Ferramentas de
    /// diagnóstico e testes inspecionam aqui; caminho quente usa os
    /// `resolve_*` (que chamam isto internamente).
    // The core-only spin lock cannot poison; retain the original std poison
    // handling in the shared algorithm without inventing a core lock failure.
    #[cfg_attr(not(feature = "std"), allow(irrefutable_let_patterns))]
    pub fn entries_for(&self, res_id: u32) -> Option<Arc<Vec<ResEntry>>> {
        let cache = &self.cache;
        if let Ok(guard) = cache.lock() {
            if let Some(hit) = guard.get(&res_id) {
                return Some(Arc::clone(hit));
            }
        }
        let start = self.pairs.partition_point(|(r, _, _)| *r < res_id);
        let end = self.pairs.partition_point(|(r, _, _)| *r <= res_id);
        if start == end {
            return None; // resid desconhecido (índice e cache vazios p/ ele)
        }
        let data: &[u8] = &self.raw;
        let mut out: Vec<ResEntry> = Vec::new();
        for &(_, ci, slot) in &self.pairs[start..end] {
            let Some(r) = self.chunks.get(ci as usize) else {
                continue;
            };
            let rel = slot_rel(data, r, slot as usize);
            if !materialize_slot(data, r, &self.global, &self.budget, rel, &mut out) {
                break; // orçamento estourado: o resto deste resid fica de fora
            }
        }
        if out.is_empty() {
            // Eager: resid sem entradas empilhadas = ausente (None), não Some([]).
            return None;
        }
        let arc = Arc::new(out);
        if let Ok(mut guard) = cache.lock() {
            if guard.len() >= ENTRY_CACHE_CAP {
                guard.clear(); // bounded: reset determinístico (só re-decodifica)
            }
            guard.insert(res_id, Arc::clone(&arc));
        }
        Some(arc)
    }

    /// Todos os resids conhecidos do pacote (índice + cache sintético),
    /// ordenados e sem duplicatas — equivalente ao `entries.keys()` do eager.
    /// Consumidores tipo `getIdentifier` iteram aqui e chamam `resolve_name`.
    #[cfg_attr(not(feature = "std"), allow(irrefutable_let_patterns))]
    pub fn resource_ids(&self) -> Vec<u32> {
        // pairs já está ordenado por resid — unique preservando a ordem;
        // cache sintético (testes) contribui com chaves extras, re-ordenadas.
        let mut ids: Vec<u32> = Vec::with_capacity(self.pairs.len());
        for &(r, _, _) in self.pairs.iter() {
            if ids.last() != Some(&r) {
                ids.push(r);
            }
        }
        if let Ok(guard) = self.cache.lock() {
            ids.extend(guard.keys().copied());
        }
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Snapshot materializado de TODAS as entradas (resid, entradas),
    /// ordenado por resid — ferramentas de DIAGNÓSTICO (ex.: example
    /// verify_config_apilevel). Custo ≈ eager de hoje; nunca usar em caminho
    /// quente (materializa o pacote inteiro, inclusive bags/strings).
    pub fn entries_snapshot(&self) -> Vec<(u32, Vec<ResEntry>)> {
        self.resource_ids()
            .into_iter()
            .map(|id| {
                let es = self
                    .entries_for(id)
                    .map(|a| (*a).clone())
                    .unwrap_or_default();
                (id, es)
            })
            .collect()
    }

    pub fn resource_name(&self, res_id: u32) -> Option<ResourceName<'_>> {
        if self.id == 0 || (res_id >> 24) != u32::from(self.id) {
            return None;
        }
        let raw_type = ((res_id >> 16) as u8)
            .checked_sub(self.type_id_offset)?
            .checked_sub(1)?;
        let entries = self.entries_for(res_id)?;
        let first = entries.first()?;
        // Configuration variants describe the same entry. Conflicting key
        // indexes are malformed metadata, not a reason to guess an identity.
        if entries.iter().any(|e| e.key_idx != first.key_idx) {
            return None;
        }
        let resource_type = self.type_strings.get(u32::from(raw_type))?;
        let entry = self.key_strings.get(first.key_idx)?;
        if self.name.is_empty() || resource_type.is_empty() || entry.is_empty() {
            return None;
        }
        Some(ResourceName {
            package: &self.name,
            resource_type,
            entry,
        })
    }
    /// Nome qualificado `@type/key` de um resid (sem pacote).
    pub fn resolve_name(&self, res_id: u32) -> Option<String> {
        let name = self.resource_name(res_id)?;
        Some(format!("@{}/{}", name.resource_type, name.entry))
    }
}

/// issue #61: valor TIPADO de uma entrada de recurso (melhor config) — o que
/// o LayoutInflater precisa para resolver `@dimen`/`@bool` por referência.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedValue {
    Str(String),
    Boolean(bool),
    Integer(i32),
    Float(f32),
    /// Valor complexo (mantissa×radix) com unidade — px/dp/sp/…
    Dimension {
        value: f32,
        unit: ComplexUnit,
    },
    /// Referência a outro recurso (resid) — o caller decide se segue a cadeia.
    Reference(u32),
    /// Referência a um atributo de tema (`?attr/...`) inside a style bag.
    Attribute(u32),
}

/// complexToFloat (TypedValue) — mesma matemática do axml (mantissa bits
/// 8–31 com sinal no bit 31, radix bits 4–5).
fn complex_to_float(data: u32) -> f32 {
    const RADIX_MULTS: [f32; 4] = [
        0.003_906_25,   // radix 0: 2⁻⁸
        3.051_758e-5,   // radix 1: 2⁻¹⁵
        1.192_092_9e-7, // radix 2: 2⁻²³
        4.656_613e-10,  // radix 3: 2⁻³¹
    ];
    let mantissa = (data & 0xFFFF_FF00) as i32;
    mantissa as f32 * RADIX_MULTS[((data >> 4) & 0x3) as usize]
}

/// Unidade de um Res_value complexo (bits 0–3 — mesma tabela do axml).
fn complex_unit_of(data: u32) -> ComplexUnit {
    match (data & 0xF) as u8 {
        0 => ComplexUnit::Px,
        1 => ComplexUnit::Dip,
        2 => ComplexUnit::Sp,
        3 => ComplexUnit::Pt,
        4 => ComplexUnit::In,
        5 => ComplexUnit::Mm,
        other => ComplexUnit::Unknown(other),
    }
}

impl Arsc {
    /// Parse a partir de bytes emprestados (copia 1× para o `Arc` interno).
    pub fn parse(data: &[u8]) -> RdResult<Arsc> {
        Self::parse_inner(Arc::new(data.to_vec()))
    }

    /// Parse tomando posse do buffer (zero cópias — caminho do `Apk::open`,
    /// que já possui o `Vec<u8>` inflado do ZIP; ADR 008: RSS conta).
    pub fn parse_owned(data: Vec<u8>) -> RdResult<Arsc> {
        Self::parse_inner(Arc::new(data))
    }

    fn parse_inner(raw: Arc<Vec<u8>>) -> RdResult<Arsc> {
        let data: &[u8] = &raw;
        if data.len() < 12 {
            return Err(RdError::parse("arsc: shorter than ResTable header"));
        }
        if u16_at(data, 0)? != RES_TABLE_TYPE {
            return Err(RdError::parse(format!(
                "arsc: expected type 0x{RES_TABLE_TYPE:04x}, got 0x{:04x}",
                u16_at(data, 0)?
            )));
        }
        let _header_size = u16_at(data, 2)? as usize;
        let file_size = u32_at(data, 4)? as usize;
        // ADR 014: o buffer (limitado ao file_size declarado) é o único Arc
        // compartilhado por pools e pacotes — Lei 2, clones baratos, zero
        // cópias por pool/pacote. Truncar o Arc seria copy-on-write caro;
        // guards de leitura usam raw.len() com offsets absolutos validados
        // pelo file_size abaixo (chunks além do limite rejeitados como hoje).
        let data = &data[..file_size.min(data.len())];
        let package_count = u32_at(data, 8)?;

        let limit = file_size.min(raw.len());
        // pool global logo após o header (12 bytes) — limitada ao file_size
        let (global_strings, pool_used) = StringPool::parse_in(&raw, 12, limit)?;
        let mut p = 12 + pool_used as usize;

        let budget = Arc::new(ArscBudget::default());
        let mut packages = Vec::new();
        let mut seen_packages = 0u32;
        while p + 8 <= limit && seen_packages < package_count {
            let ctype = u16_at(&raw, p)?;
            let cheader = u16_at(&raw, p + 2)? as usize;
            let csize = u32_at(&raw, p + 4)? as usize;
            // progresso garantido: chunk mínimo = 8 bytes (mesmo guard dos
            // irmãos em axml.rs e no loop interno de parse_package — sem ele,
            // csize=0 com packageCount≥1 gira eternamente; issue #9)
            if csize < 8 {
                return Err(RdError::parse(format!(
                    "arsc: chunk at {p} declares size {csize} < 8 (no progress possible)"
                )));
            }
            if csize < cheader || p + csize > limit {
                return Err(RdError::parse(format!(
                    "arsc: package chunk at {p} declares {csize} beyond bounds"
                )));
            }
            if ctype == RES_TABLE_PACKAGE_TYPE {
                let pkg_header_size = u16_at(&raw, p + 2)? as usize;
                packages.push(parse_package(
                    &raw,
                    p,
                    csize,
                    pkg_header_size,
                    &global_strings,
                    &budget,
                )?);
                seen_packages += 1;
            }
            p += csize;
        }
        if seen_packages != package_count {
            return Err(RdError::invalid_format(format!(
                "arsc: header announces {package_count} packages, walked {seen_packages}"
            )));
        }

        Ok(Arsc {
            global_strings,
            packages,
        })
    }

    /// Resolve o resource id para string, seguindo cadeias de referência
    /// (ex.: `@string/app_name` → alias → texto). Limite de 8 saltos contra ciclos.
    pub fn resolve_string(&self, res_id: u32) -> Option<String> {
        self.resolve_string_depth(res_id, 0)
    }

    fn resolve_string_depth(&self, res_id: u32, depth: usize) -> Option<String> {
        if depth > 8 {
            return None;
        }
        let pkg = self.package_for(res_id)?;
        let entries = pkg.entries_for(res_id)?;
        let best = entries
            .iter()
            .min_by_key(|e| (e.config.specificity(), !e.config.is_default()))?;
        match best.data_type {
            3 => best.string.clone(),
            1 => self.resolve_string_depth(best.data, depth + 1), // REFERENCE
            _ => None,
        }
    }

    /// `@type/key` do recurso (primeiro pacote que o conhece).
    pub fn resolve_name(&self, res_id: u32) -> Option<String> {
        let name = self.resource_name(res_id)?;
        Some(format!("@{}/{}", name.resource_type, name.entry))
    }

    /// Lookup does not follow aliases or select a value configuration. Table
    /// fragments must agree on the identity; conflicting owners are ambiguous.
    pub fn resource_name(&self, res_id: u32) -> Option<ResourceName<'_>> {
        let mut found = None;
        for package in self
            .packages
            .iter()
            .filter(|p| p.entries_for(res_id).is_some())
        {
            let candidate = package.resource_name(res_id)?;
            if found.is_some_and(|name| name != candidate) {
                return None;
            }
            found = Some(candidate);
        }
        found
    }

    /// issue #61: valor TIPADO da melhor config de um resid. `None` = resid
    /// desconhecido ou tipo não representável (bag/complex — o caller falha
    /// tipado; nunca default silencioso).
    pub fn resolve_value(&self, res_id: u32) -> Option<ResolvedValue> {
        self.resolve_value_version(res_id, None)
    }
    /// Runtime selection: retain locale/density precedence, choose the newest
    /// eligible SDK variant, and never use an entry requiring a newer SDK.
    /// This does not implement Android's remaining configuration qualifiers.
    pub fn resolve_value_for_api(&self, res_id: u32, api: u16) -> Option<ResolvedValue> {
        self.resolve_value_version(res_id, Some(api))
    }
    fn resolve_value_version(&self, res_id: u32, api: Option<u16>) -> Option<ResolvedValue> {
        let pkg = self.package_for(res_id)?;
        let entries = pkg.entries_for(res_id)?;
        let best = entries
            .iter()
            .filter(|e| api.is_none_or(|api| e.config.api_level.unwrap_or(0) <= api))
            .min_by_key(|e| {
                (
                    e.config.specificity(),
                    !e.config.is_default(),
                    core::cmp::Reverse(api.map_or(0, |_| e.config.api_level.unwrap_or(0))),
                )
            })?;
        match best.data_type {
            0x03 => best.string.clone().map(ResolvedValue::Str),
            0x01 => Some(ResolvedValue::Reference(best.data)),
            0x02 => Some(ResolvedValue::Attribute(best.data)),
            // INT_DEC (u32 com semântica i32 — TypedValue.data é cru)
            0x10 => Some(ResolvedValue::Integer(best.data as i32)),
            0x11 => Some(ResolvedValue::Integer(best.data as i32)),
            0x12 => Some(ResolvedValue::Boolean(best.data != 0)),
            // INT_COLOR_ARGB8/RGB8/ARGB4/RGB4: aapt2 stores the expanded ARGB
            0x1c..=0x1f => Some(ResolvedValue::Integer(best.data as i32)),
            0x04 => Some(ResolvedValue::Float(f32::from_bits(best.data))),
            0x05 => Some(ResolvedValue::Dimension {
                value: complex_to_float(best.data),
                unit: complex_unit_of(best.data),
            }),
            _ => None,
        }
    }

    /// Resolve one attribute from a style, including its parent style and
    /// `?attr/…` aliases. Resource references remain explicit so the VM can
    /// dereference them using the APK's normal resource policy.
    pub fn resolve_style_attribute(&self, style_id: u32, attr_id: u32) -> Option<ResolvedValue> {
        self.resolve_style_attribute_inner(style_id, attr_id, 0)
    }

    fn resolve_style_attribute_inner(
        &self,
        style_id: u32,
        attr_id: u32,
        depth: usize,
    ) -> Option<ResolvedValue> {
        if depth >= 64 {
            return None;
        }
        let pkg = self.package_for(style_id)?;
        let entries = pkg.entries_for(style_id)?;
        let style = entries
            .iter()
            .min_by_key(|e| (e.config.specificity(), !e.config.is_default()))?;
        if let Some(value) = style
            .bag
            .as_ref()
            .and_then(|bag| bag.iter().find(|value| value.name == attr_id))
        {
            let resolved = match value.data_type {
                0x01 => ResolvedValue::Reference(value.data),
                0x02 => ResolvedValue::Attribute(value.data),
                0x03 => ResolvedValue::Str(value.string.clone()?),
                0x04 => ResolvedValue::Float(f32::from_bits(value.data)),
                0x05 => ResolvedValue::Dimension {
                    value: complex_to_float(value.data),
                    unit: complex_unit_of(value.data),
                },
                0x10 | 0x11 | 0x1c..=0x1f => ResolvedValue::Integer(value.data as i32),
                0x12 => ResolvedValue::Boolean(value.data != 0),
                _ => return None,
            };
            return match resolved {
                ResolvedValue::Attribute(next) => {
                    self.resolve_style_attribute_inner(style_id, next, depth + 1)
                }
                other => Some(other),
            };
        }
        let parent = style.parent?;
        self.resolve_style_attribute_inner(parent, attr_id, depth + 1)
    }

    pub fn package_for(&self, res_id: u32) -> Option<&Package> {
        let pid = ((res_id >> 24) & 0xFF) as u8;
        // issue #54: SEM fallback para o primeiro package — um arsc hostil
        // não pode nomear ids de package estranho (ex.: 0x01 = framework
        // android:id/…). Package id inexistente → resolução honesta falha.
        self.packages.iter().find(|p| p.id == pid)
    }
}

/// Constrói o pacote: valida o header (issue #331/#246 — mesmos guards do eager)
/// e indexa as entradas de tipo. Nada de entrada é materializado aqui (ADR 014).
fn parse_package(
    raw: &Arc<Vec<u8>>,
    base: usize,
    size: usize,
    header_size: usize,
    global: &StringPool,
    budget: &Arc<ArscBudget>,
) -> RdResult<Package> {
    let data: &[u8] = raw;
    let end = base
        .checked_add(size)
        .ok_or_else(|| RdError::parse("arsc: package size overflow"))?;
    if header_size < 284 || header_size > size || end > data.len() {
        return Err(RdError::parse("arsc: package chunk truncated (header)"));
    }
    // Every read below is confined to the package that owns the metadata.
    let data = &data[..end];
    let pkg_id = u8::try_from(u32_at(data, base + 8)?)
        .map_err(|_| RdError::parse("arsc: package id exceeds 8 bits"))?;
    let type_id_offset = if header_size >= 288 {
        u8::try_from(u32_at(data, base + 284)?)
            .map_err(|_| RdError::parse("arsc: typeIdOffset exceeds 8 bits"))?
    } else {
        0
    };
    let type_strings_off = u32_at(data, base + 268)? as usize;
    let key_strings_off = u32_at(data, base + 276)? as usize;

    // name: 128 UTF-16 units @12
    let mut name_units = Vec::with_capacity(128);
    for i in 0..128 {
        let u = u16_at(data, base + 12 + i * 2)?;
        if u == 0 {
            break;
        }
        name_units.push(u);
    }
    let name = String::from_utf16_lossy(&name_units);

    let symbol_pool = |offset: usize| -> RdResult<StringPool> {
        // Zero denotes inherited overlay pools. Overlay/dynamic package
        // composition is not implemented, so do not invent owning names.
        if offset < header_size || offset > size.saturating_sub(8) {
            return Err(RdError::parse("arsc: symbol pool outside owning package"));
        }
        let (pool, used) = StringPool::parse_in(raw, base + offset, end)?;
        if used as usize > size - offset {
            return Err(RdError::parse("arsc: symbol pool exceeds owning package"));
        }
        Ok(pool)
    };
    let type_strings = symbol_pool(type_strings_off)?;
    let key_strings = symbol_pool(key_strings_off)?;

    // Walk idêntico ao eager (mesmos breaks e janelas), mas sem materializar.
    let mut chunks: Vec<TypeChunkRef> = Vec::new();
    let mut pairs: Vec<(u32, u32, u32)> = Vec::new();
    let mut p = base
        .checked_add(header_size)
        .ok_or_else(|| RdError::parse("arsc: package header offset overflow"))?;
    while p + 8 <= end {
        let ctype = u16_at(data, p)?;
        let cheader = u16_at(data, p + 2)? as usize;
        let csize = u32_at(data, p + 4)? as usize;
        let Some(chunk_end) = p.checked_add(csize) else {
            break;
        };
        // progresso garantido (chunk mínimo = 8 bytes) — endurece contra fuzz
        if csize < 8 || csize < cheader || chunk_end > end {
            break; // chunk truncado: não fatal — o que veio antes fica
        }
        if ctype == RES_TABLE_TYPE_CHUNK {
            if cheader < 20 || csize < 20 {
                return Err(RdError::parse("arsc: type header shorter than 20 bytes"));
            }
            let type_id = data.get(p + 8).copied().unwrap_or(0);
            let effective_type = type_id
                .checked_add(type_id_offset)
                .filter(|_| type_id != 0)
                .ok_or_else(|| RdError::parse("arsc: invalid or overflowing type id"))?;
            let flags = data.get(p + 9).copied().unwrap_or(0);
            let entry_count = u32_at(data, p + 12)? as usize;
            if entry_count > 65_536 {
                return Err(RdError::parse(
                    "arsc: entry count exceeds 16-bit resource ids",
                ));
            }
            let entries_start = u32_at(data, p + 16)? as usize;
            let config = parse_config(data, p + 20, cheader - 20)?;

            // Issue #246: a tabela de offsets ocupa [header, entriesStart) DENTRO
            // deste chunk. entryCount além dessa janela não lê chunks vizinhos.
            let table_start = p + cheader;
            let table_limit = if entries_start >= cheader && entries_start <= csize {
                p + entries_start
            } else {
                table_start
            };
            let table_len = |width: usize| entry_count.min((table_limit - table_start) / width);
            let chunk_idx = chunks.len() as u32;
            let make_resid = |idx: usize| {
                ((pkg_id as u32) << 24) | ((effective_type as u32) << 16) | (idx as u32 & 0xFFFF)
            };

            if flags & FLAG_SPARSE != 0 {
                // ResTable_sparseTypeEntry: {idx u16, offset u16}; cada slot é um par.
                for i in 0..table_len(4) {
                    let idx = u16_at(data, table_start + i * 4)? as usize;
                    pairs.push((make_resid(idx), chunk_idx, i as u32));
                }
            } else if flags & FLAG_OFFSET16 != 0 {
                for i in 0..table_len(2) {
                    pairs.push((make_resid(i), chunk_idx, i as u32));
                }
            } else {
                for i in 0..table_len(4) {
                    if u32_at(data, table_start + i * 4)? == 0xFFFF_FFFF {
                        continue; // sem entrada
                    }
                    pairs.push((make_resid(i), chunk_idx, i as u32));
                }
            }

            chunks.push(TypeChunkRef {
                chunk_end,
                table_start,
                entries_abs: table_limit,
                config,
                flags,
            });
        }
        p = chunk_end;
    }

    // Ordem determinística (Lei 4): (resid, chunk, slot) = ordem de walk do eager.
    pairs.sort_unstable();

    Ok(Package {
        id: pkg_id,
        name,
        type_id_offset,
        type_strings,
        key_strings,
        global: global.clone(),
        chunks: Arc::new(chunks),
        pairs: Arc::new(pairs),
        budget: Arc::clone(budget),
        raw: Arc::clone(raw),
        cache: Mutex::new(BTreeMap::new()),
    })
}

/// Offset relativo (×4, como o eager) do slot `slot` de um chunk de tipo.
/// O slot já foi validado contra a janela no parse; `unwrap_or` é inalcançável.
fn slot_rel(data: &[u8], r: &TypeChunkRef, slot: usize) -> u32 {
    if r.flags & FLAG_SPARSE != 0 {
        // sparse: offset16 fica no 2º u16 do slot
        u16_at(data, r.table_start + slot * 4 + 2).unwrap_or(0xFFFF) as u32 * 4
    } else if r.flags & FLAG_OFFSET16 != 0 {
        u16_at(data, r.table_start + slot * 2).unwrap_or(0xFFFF) as u32 * 4
    } else {
        u32_at(data, r.table_start + slot * 4).unwrap_or(0xFFFF_FFFF)
    }
}

/// Materializa UMA entrada (porta 1:1 do `collect` eager: mesmos guards,
/// mesma pré-resolução de strings, mesma cobrança de orçamento).
/// Retorna `false` se o orçamento estourou (o chamador para de materializar).
fn materialize_slot(
    data: &[u8],
    r: &TypeChunkRef,
    global: &StringPool,
    budget: &ArscBudget,
    rel_off: u32,
    out: &mut Vec<ResEntry>,
) -> bool {
    if rel_off == 0xFFFF_FFFF {
        return true;
    }
    let chunk_end = r.chunk_end;
    let Some(eoff) = r.entries_abs.checked_add(rel_off as usize) else {
        return true;
    };
    if eoff
        .checked_add(8)
        .is_none_or(|entry_end| entry_end > chunk_end)
    {
        return true; // entrada fora/truncada neste chunk: skip
    }
    let e_size = u16_at(data, eoff).unwrap_or(0) as usize;
    let e_flags = u16_at(data, eoff + 2).unwrap_or(0);
    let key_idx = u32_at(data, eoff + 4).unwrap_or(0);
    let (data_type, ddata, complex, parent, bag) = if e_flags & ENTRY_FLAG_COMPLEX != 0 {
        let parent = if e_size >= 16 && eoff + 16 <= chunk_end {
            let parent = u32_at(data, eoff + 8).unwrap_or(0);
            (parent != 0).then_some(parent)
        } else {
            None
        };
        let map_count = if e_size >= 16 && eoff + 16 <= chunk_end {
            u32_at(data, eoff + 12).unwrap_or(0) as usize
        } else {
            usize::MAX
        };
        let maps_start = eoff.saturating_add(e_size);
        let available = chunk_end.saturating_sub(maps_start) / 12;
        let bag = if map_count <= 65_536 && map_count <= available {
            let mut bag = Vec::with_capacity(map_count);
            for i in 0..map_count {
                let m = maps_start + i * 12;
                let value_size = u16_at(data, m + 4).unwrap_or(0);
                if value_size < 8 {
                    bag.clear();
                    break;
                }
                let name = u32_at(data, m).unwrap_or(0);
                let dtype = *data.get(m + 7).unwrap_or(&0);
                let value = u32_at(data, m + 8).unwrap_or(0);
                let item_string = (dtype == 3)
                    .then(|| global.get(value).map(str::to_owned))
                    .flatten();
                if !budget.charge(item_string.as_deref()) {
                    return false;
                }
                bag.push(ResMapEntry {
                    name,
                    data_type: dtype,
                    data: value,
                    string: item_string,
                });
            }
            (bag.len() == map_count).then_some(bag)
        } else {
            None
        };
        (0u8, 0u32, true, parent, bag)
    } else {
        // Res_value após ResTable_entry (e_size bytes)
        let Some(v) = eoff.checked_add(e_size) else {
            return true;
        };
        if v.checked_add(8)
            .is_none_or(|value_end| value_end > chunk_end)
        {
            return true;
        }
        let dtype = *data.get(v + 3).unwrap_or(&0);
        let d = u32_at(data, v + 4).unwrap_or(0);
        (dtype, d, false, None, None)
    };
    let string = if data_type == 3 && !complex {
        global.get(ddata).map(str::to_owned)
    } else {
        None
    };
    if !budget.charge(string.as_deref()) {
        return false;
    }
    out.push(ResEntry {
        config: r.config.clone(),
        data_type,
        data: ddata,
        string,
        complex,
        parent,
        bag,
        key_idx,
    });
    true
}

/// ResTable_config — extrai locale/densidade/api sem reclamar de variações de tamanho.
///
/// Layout do `ResTable_config` (frameworks/base/libs/androidfw/include/androidfw/
/// ResourceTypes.h) — offsets conferidos campo a campo:
///
/// ```text
/// size:u32@0 | mcc:u16@4 mnc:u16@6 | language[2]@8 country[2]@10
/// orientation:u8@12 touchscreen:u8@13 density:u16@14
/// keyboard:u8@16 navigation:u8@17 inputFlags:u8@18 pad0:u8@19
/// screenWidth:u16@20 screenHeight:u16@22
/// sdkVersion:u16@24 minorVersion:u16@26
/// screenLayout:u8@28 uiMode:u8@29 smallestScreenWidthDp:u16@30 (se size ≥ 32)
/// ```
///
/// `api_level` é o `sdkVersion` (u16 @24 — o mesmo campo que dá nome às configs
/// "v26" do aapt2). NÃO é `screenHeight` (u16 @22); o bloco [20..24) é o par
/// screenWidth/screenHeight de 2+2 bytes, não dois u32 (verificado empiricamente
/// contra resources.arsc reais — configs "vNN" decodificam com o valor da API).
fn parse_config(data: &[u8], cfg_base: usize, cfg_max: usize) -> RdResult<ResConfigInfo> {
    let mut cfg = ResConfigInfo::default();
    if cfg_max < 4 || cfg_base + 4 > data.len() {
        cfg.unsupported_qualifiers = true;
        return Ok(cfg);
    }
    let cfg_size = u32_at(data, cfg_base)? as usize;
    let readable = cfg_size
        .min(cfg_max)
        .min(data.len().saturating_sub(cfg_base));
    cfg.unsupported_qualifiers = cfg_size < 4
        || cfg_size > readable
        || (4..readable).any(|offset| {
            !matches!(offset, 8..=11 | 14..=15 | 24..=25) && data[cfg_base + offset] != 0
        });
    let lang = |off: usize| -> Option<String> {
        let b = data.get(cfg_base + off..cfg_base + off + 2)?;
        if b[0] == 0 && b[1] == 0 {
            None
        } else {
            // estilo AOSP: 0 se byte < 128, senão 'a' + (b - 128) (packed unicode é raro)
            let chars: Vec<char> = b
                .iter()
                .filter(|&&c| c != 0)
                .map(|&c| {
                    if c < 128 {
                        c as char
                    } else {
                        char::from_u32('a' as u32 + (c - 128) as u32).unwrap_or('?')
                    }
                })
                .collect();
            Some(chars.into_iter().collect())
        }
    };
    if readable >= 12 {
        cfg.language = lang(8);
        cfg.country = lang(10);
    }
    if readable >= 16 {
        let d = u16_at(data, cfg_base + 14)?;
        cfg.density = Some(d);
    }
    if readable >= 26 {
        // sdkVersion:u16 @24 (minorVersion:u16 @26 — não lido)
        cfg.api_level = Some(u16_at(data, cfg_base + 24)?);
    }
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmap_consumers_can_detect_unmodeled_raw_resource_qualifiers() {
        let mut data = [0u8; 64];
        data[..4].copy_from_slice(&64u32.to_le_bytes());
        data[14..16].copy_from_slice(&320u16.to_le_bytes());
        data[24..26].copy_from_slice(&26u16.to_le_bytes());
        assert!(!parse_config(&data, 0, 64).unwrap().unsupported_qualifiers);
        for offset in [4, 12, 13, 16, 20, 26, 28, 36, 60] {
            data[offset] = 1;
            assert!(parse_config(&data, 0, 64).unwrap().unsupported_qualifiers);
            data[offset] = 0;
        }
        assert!(parse_config(&data, 0, 32).unwrap().unsupported_qualifiers);
        data[8] = b'f';
        data[9] = b'r';
        let config = parse_config(&data, 0, 64).unwrap();
        assert_eq!(config.language.as_deref(), Some("fr"));
        assert!(!config.unsupported_qualifiers);
    }

    fn empty_global_pool() -> Vec<u8> {
        // ResStringPool vazio válido: 28 bytes de header, 0 strings
        let mut p = Vec::new();
        p.extend_from_slice(&0x0001u16.to_le_bytes()); // type
        p.extend_from_slice(&28u16.to_le_bytes()); // headerSize
        p.extend_from_slice(&28u32.to_le_bytes()); // size
        p.extend_from_slice(&0u32.to_le_bytes()); // stringCount
        p.extend_from_slice(&0u32.to_le_bytes()); // styleCount
        p.extend_from_slice(&0u32.to_le_bytes()); // flags (UTF-16)
        p.extend_from_slice(&28u32.to_le_bytes()); // stringsStart
        p.extend_from_slice(&0u32.to_le_bytes()); // stylesStart
        p
    }

    /// PoC da issue #9: 48 bytes = header ResTable (packageCount=1) + pool
    /// global vazia + chunk {type=0x0202, headerSize=0, size=0}. Sem o guard
    /// `csize < 8` o loop externo não progride (`p += 0`) e trava para sempre
    /// (hang, não crash). Deve retornar erro tipado em tempo finito.
    #[test]
    fn zero_size_chunk_is_typed_error_not_hang() {
        let mut d = Vec::new();
        d.extend_from_slice(&0x0002u16.to_le_bytes()); // RES_TABLE_TYPE
        d.extend_from_slice(&12u16.to_le_bytes()); // headerSize
        d.extend_from_slice(&48u32.to_le_bytes()); // file size
        d.extend_from_slice(&1u32.to_le_bytes()); // packageCount = 1
        assert_eq!(d.len(), 12);
        d.extend(empty_global_pool());
        assert_eq!(d.len(), 40);
        d.extend_from_slice(&0x0202u16.to_le_bytes()); // tipo ≠ package
        d.extend_from_slice(&0u16.to_le_bytes()); // headerSize = 0
        d.extend_from_slice(&0u32.to_le_bytes()); // size = 0 → sem progresso
        assert_eq!(d.len(), 48);

        let e = Arsc::parse(&d).expect_err("csize=0 deve ser rejeitado");
        assert_eq!(e.code, "PARSE_ERROR");
        assert!(e.cause.contains("< 8"), "mensagem: {}", e.cause);
    }

    /// Issue #16: `api_level` é o `sdkVersion` u16 @24 do ResTable_config —
    /// NÃO é screenHeight (@22). screenHeight=1440 (0x5A0) NÃO pode vazar
    /// para api_level; sdkVersion=26 deve decodificar como 26 (config "v26").
    #[test]
    fn config_api_level_is_sdk_version_u16_at_offset_24() {
        let mut cfg = vec![0u8; 28];
        cfg[0..4].copy_from_slice(&28u32.to_le_bytes()); // size
        cfg[8..10].copy_from_slice(b"en"); // language
        cfg[10..12].copy_from_slice(b"US"); // country
        cfg[14..16].copy_from_slice(&256u16.to_le_bytes()); // density
        cfg[20..22].copy_from_slice(&720u16.to_le_bytes()); // screenWidth
        cfg[22..24].copy_from_slice(&1440u16.to_le_bytes()); // screenHeight
        cfg[24..26].copy_from_slice(&26u16.to_le_bytes()); // sdkVersion

        let info = parse_config(&cfg, 0, cfg.len()).expect("config");
        assert_eq!(
            info.api_level,
            Some(26),
            "sdkVersion@24, não screenHeight@22"
        );
        assert_eq!(info.density, Some(256));
        assert_eq!(info.language.as_deref(), Some("en"));
        assert_eq!(info.country.as_deref(), Some("US"));
    }
    #[test]
    fn runtime_resource_selects_newest_eligible_sdk_without_changing_inspection() {
        let id = 0x7f020000;
        let variant = |api, label: &str| ResEntry {
            config: ResConfigInfo {
                api_level: api,
                ..Default::default()
            },
            data_type: 3,
            data: 0,
            string: Some(label.into()),
            complex: false,
            parent: None,
            bag: None,
            key_idx: 0,
        };
        let mut entries = BTreeMap::new();
        entries.insert(
            id,
            vec![
                variant(None, "base"),
                variant(Some(1), "v1"),
                variant(Some(26), "v26"),
                variant(Some(27), "future"),
            ],
        );
        let arsc = Arsc {
            global_strings: Default::default(),
            packages: vec![Package::synthetic(
                0x7f,
                "test".into(),
                Default::default(),
                Default::default(),
                entries,
            )],
        };
        assert_eq!(
            arsc.resolve_value(id),
            Some(ResolvedValue::Str("base".into()))
        );
        assert_eq!(
            arsc.resolve_value_for_api(id, 25),
            Some(ResolvedValue::Str("v1".into()))
        );
        assert_eq!(
            arsc.resolve_value_for_api(id, 26),
            Some(ResolvedValue::Str("v26".into()))
        );
        assert_eq!(
            arsc.resolve_value_for_api(id, 27),
            Some(ResolvedValue::Str("future".into()))
        );
    }

    #[test]
    fn style_attributes_follow_parent_and_theme_attribute_aliases() {
        let parent = 0x7f12_0001;
        let child = 0x7f12_0002;
        let primary = 0x7f04_0001;
        let alias = 0x7f04_0002;
        let inherited = 0x7f04_0003;
        let absent = 0x7f04_0004;
        let entry = |parent, bag| ResEntry {
            config: ResConfigInfo::default(),
            data_type: 0,
            data: 0,
            string: None,
            complex: true,
            parent,
            bag: Some(bag),
            key_idx: 0,
        };
        let mut entries = BTreeMap::new();
        entries.insert(
            parent,
            vec![entry(
                None,
                vec![
                    ResMapEntry {
                        name: primary,
                        data_type: 0x12,
                        data: 1,
                        string: None,
                    },
                    ResMapEntry {
                        name: inherited,
                        data_type: 0x12,
                        data: 1,
                        string: None,
                    },
                ],
            )],
        );
        entries.insert(
            child,
            vec![entry(
                Some(parent),
                vec![
                    ResMapEntry {
                        name: primary,
                        data_type: 0x12,
                        data: 0,
                        string: None,
                    },
                    ResMapEntry {
                        name: alias,
                        data_type: 0x02,
                        data: primary,
                        string: None,
                    },
                ],
            )],
        );
        let arsc = Arsc {
            global_strings: StringPool::default(),
            packages: vec![Package::synthetic(
                0x7f,
                "test".into(),
                StringPool::default(),
                StringPool::default(),
                entries,
            )],
        };
        assert_eq!(
            arsc.resolve_style_attribute(child, primary),
            Some(ResolvedValue::Boolean(false)),
            "the child style overrides its parent"
        );
        assert_eq!(
            arsc.resolve_style_attribute(child, alias),
            Some(ResolvedValue::Boolean(false)),
            "?attr alias resolves through the child's effective theme"
        );
        assert_eq!(
            arsc.resolve_style_attribute(child, inherited),
            Some(ResolvedValue::Boolean(true)),
            "a missing child attribute is inherited from the parent"
        );
        assert_eq!(
            arsc.resolve_style_attribute(child, absent),
            None,
            "absent attributes stay absent"
        );
    }

    /// ADR 014: materialização sob demanda produz os mesmos resolvables do
    /// eager — aqui um ARSC mínimo com 1 pacote, 1 chunk de tipo, 1 string.
    #[test]
    fn lazy_index_resolves_like_eager() {
        // constrói via ArscFix-like bytes: header + pool global ["Oi"] +
        // pacote com type_strings ["string"], key_strings ["hello"],
        // 1 chunk ResTable_type com 1 entrada STRING apontando p/ "Oi".
        let mut d = Vec::new();
        // ResTable header (12) — packageCount=1 (patched no fim)
        d.extend_from_slice(&0x0002u16.to_le_bytes());
        d.extend_from_slice(&12u16.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes()); // size (patch)
        d.extend_from_slice(&1u32.to_le_bytes());
        // pool global UTF-16: ["Oi"] — header 28 + 1 offset + 6 bytes
        let mut g = Vec::new();
        g.extend_from_slice(&0x0001u16.to_le_bytes());
        g.extend_from_slice(&28u16.to_le_bytes());
        g.extend_from_slice(&0u32.to_le_bytes()); // size (patch)
        g.extend_from_slice(&1u32.to_le_bytes()); // count
        g.extend_from_slice(&0u32.to_le_bytes()); // styleCount
        g.extend_from_slice(&0u32.to_le_bytes()); // flags = UTF-16
        g.extend_from_slice(&32u32.to_le_bytes()); // stringsStart
        g.extend_from_slice(&0u32.to_le_bytes()); // stylesStart
        g.extend_from_slice(&0u32.to_le_bytes()); // offset da string 0
        g.extend_from_slice(&2u16.to_le_bytes()); // len 2 ("Oi")
        g.extend_from_slice(&"O".encode_utf16().next().unwrap().to_le_bytes());
        g.extend_from_slice(&"i".encode_utf16().next().unwrap().to_le_bytes());
        g.extend_from_slice(&0u16.to_le_bytes()); // terminador
        let gs = g.len() as u32;
        g[4..8].copy_from_slice(&gs.to_le_bytes());
        d.extend(g);

        // pacote: header 288 (headerSize) + typeStrings/keyStrings + 1 chunk tipo
        let pkg_header_size: usize = 288;
        let mut type_pool = Vec::new();
        type_pool.extend_from_slice(&0x0001u16.to_le_bytes());
        type_pool.extend_from_slice(&28u16.to_le_bytes());
        type_pool.extend_from_slice(&0u32.to_le_bytes());
        type_pool.extend_from_slice(&1u32.to_le_bytes());
        type_pool.extend_from_slice(&0u32.to_le_bytes());
        type_pool.extend_from_slice(&0u32.to_le_bytes());
        type_pool.extend_from_slice(&32u32.to_le_bytes());
        type_pool.extend_from_slice(&0u32.to_le_bytes());
        type_pool.extend_from_slice(&0u32.to_le_bytes());
        type_pool.extend_from_slice(&6u16.to_le_bytes()); // 's','t' via chars
        for ch in "string".encode_utf16() {
            type_pool.extend_from_slice(&ch.to_le_bytes());
        }
        type_pool.extend_from_slice(&0u16.to_le_bytes());
        let ts = type_pool.len() as u32;
        type_pool[4..8].copy_from_slice(&ts.to_le_bytes());
        // (key pool = mesmo formato, 1 string "hello")
        let mut key_pool = Vec::new();
        key_pool.extend_from_slice(&0x0001u16.to_le_bytes());
        key_pool.extend_from_slice(&28u16.to_le_bytes());
        key_pool.extend_from_slice(&0u32.to_le_bytes());
        key_pool.extend_from_slice(&1u32.to_le_bytes());
        key_pool.extend_from_slice(&0u32.to_le_bytes());
        key_pool.extend_from_slice(&0u32.to_le_bytes());
        key_pool.extend_from_slice(&32u32.to_le_bytes());
        key_pool.extend_from_slice(&0u32.to_le_bytes());
        key_pool.extend_from_slice(&0u32.to_le_bytes());
        key_pool.extend_from_slice(&5u16.to_le_bytes()); // len 5
        for ch in "hello".encode_utf16() {
            key_pool.extend_from_slice(&ch.to_le_bytes());
        }
        key_pool.extend_from_slice(&0u16.to_le_bytes());
        let ks = key_pool.len() as u32;
        key_pool[4..8].copy_from_slice(&ks.to_le_bytes());

        let type_strings_off = pkg_header_size; // logo após o header do pacote
        let key_strings_off = type_strings_off + type_pool.len();

        // chunk ResTable_type: header 20 + config 8 + offsets 4 + entry 8 + value 8
        // ResTable_type = header(8) + id@8 + res0@9 + res1@10 + reserved@11 +
        // entryCount@12 + entriesStart@16 (+ config após os 20 bytes de header)
        let entries_start_off: u32 = 20 + 8 + 4; // relativo ao início do chunk
        let mut tc = Vec::new();
        tc.extend_from_slice(&0x0201u16.to_le_bytes()); // type
        tc.extend_from_slice(&28u16.to_le_bytes()); // headerSize (20 header + config 8)
        tc.extend_from_slice(&0u32.to_le_bytes()); // size (patch)
        tc.push(1u8); // id@8
        tc.push(0u8); // res0@9
        tc.push(0u8); // res1@10
        tc.push(0u8); // reserved@11
        tc.extend_from_slice(&1u32.to_le_bytes()); // entryCount@12
        tc.extend_from_slice(&entries_start_off.to_le_bytes()); // entriesStart@16
        tc.extend_from_slice(&8u32.to_le_bytes()); // config size = 8 (vazia)
        tc.extend_from_slice(&[0u8; 4]); // config: 4 bytes restantes (screenConfig mínimo)
                                         // offsets (1 entrada, rel a entriesStart): 0
        tc.extend_from_slice(&0u32.to_le_bytes());
        // ResTable_entry: size 8, flags 0, key_idx 0
        tc.extend_from_slice(&8u16.to_le_bytes()); // entry size
        tc.extend_from_slice(&0u16.to_le_bytes()); // flags = simples
        tc.extend_from_slice(&0u32.to_le_bytes()); // key idx (na key pool)
                                                   // Res_value: size 8, res0 0, dataType 0x03 (STRING), data = índice 0 na global
        tc.extend_from_slice(&8u16.to_le_bytes()); // value size
        tc.push(0u8); // res0
        tc.push(0x03u8); // dataType STRING
        tc.extend_from_slice(&0u32.to_le_bytes()); // data = 0 (global idx 0)
        let tcs = tc.len() as u32;
        tc[4..8].copy_from_slice(&tcs.to_le_bytes());

        let pkg_size = pkg_header_size + type_pool.len() + key_pool.len() + tc.len();
        let mut pkg = Vec::new();
        pkg.extend_from_slice(&0x0200u16.to_le_bytes()); // RES_TABLE_PACKAGE_TYPE
        pkg.extend_from_slice(&(pkg_header_size as u16).to_le_bytes());
        pkg.extend_from_slice(&(pkg_size as u32).to_le_bytes()); // size (patch se precisar)
        pkg.extend_from_slice(&0x7fu32.to_le_bytes()); // id@8 = 0x7f → [8..12)
                                                       // name: 128 UTF-16 units @12 — "t\0..." (termina em \0) → [12..268)
        let mut name = vec![0u16; 128];
        name[0] = 't' as u16;
        for u in name {
            pkg.extend_from_slice(&u.to_le_bytes());
        }
        // [12..268) = name; typeStrings@268, lastPublicType@272,
        // keyStrings@276, lastPublicKey@280; header termina em 288.
        while pkg.len() < 268 {
            pkg.push(0);
        }
        pkg.extend_from_slice(&(type_strings_off as u32).to_le_bytes()); // typeStrings @268
        pkg.extend_from_slice(&1u32.to_le_bytes()); // lastPublicType @272
        pkg.extend_from_slice(&(key_strings_off as u32).to_le_bytes()); // keyStrings @276
        pkg.extend_from_slice(&1u32.to_le_bytes()); // lastPublicKey @280
                                                    // header 288: resto zero até 288
        while pkg.len() < 288 {
            pkg.push(0);
        }
        pkg.extend(type_pool);
        pkg.extend(key_pool);
        pkg.extend(tc);

        d.extend(pkg);
        let total = d.len() as u32;
        d[4..8].copy_from_slice(&total.to_le_bytes());

        let arsc = Arsc::parse(&d).expect("arsc válido");
        assert_eq!(arsc.packages.len(), 1);
        let resid = 0x7f01_0000u32; // pkg 0x7f, tipo 1, idx 0
        assert_eq!(arsc.resolve_string(resid).as_deref(), Some("Oi"));
        assert_eq!(arsc.resolve_name(resid).as_deref(), Some("@string/hello"));
        // resolve_value pela melhor config (default)
        match arsc.resolve_value(resid) {
            Some(ResolvedValue::Str(s)) => assert_eq!(s, "Oi"),
            other => panic!("esperava Str(\"Oi\"), veio {other:?}"),
        }
        // resid desconhecido → None honesto
        assert_eq!(arsc.resolve_string(0x7f01_0001), None);
    }

    /// arsc mínimo: pool global com UMA string UTF-8 de `slen` bytes e
    /// um pacote cujo type chunk (OFFSET16) tem `count` entradas, TODAS
    /// com `rel_off = 0` apontando para uma única entrada simples
    /// (string idx 0) — cada entrada clona a string inteira.
    fn aliased_string_arsc(count: u32, slen: usize) -> Vec<u8> {
        fn varint2(v: usize) -> [u8; 2] {
            [0x80 | ((v >> 8) as u8), (v & 0xFF) as u8]
        }
        // pool global UTF-8 (1 string grande)
        let mut global = Vec::new();
        global.extend_from_slice(&0x0001u16.to_le_bytes());
        global.extend_from_slice(&28u16.to_le_bytes());
        global.extend_from_slice(&0u32.to_le_bytes()); // size (patched)
        global.extend_from_slice(&1u32.to_le_bytes());
        global.extend_from_slice(&0u32.to_le_bytes());
        global.extend_from_slice(&crate::pool::UTF8_FLAG.to_le_bytes());
        global.extend_from_slice(&32u32.to_le_bytes()); // stringsStart = 28+4
        global.extend_from_slice(&0u32.to_le_bytes());
        global.extend_from_slice(&0u32.to_le_bytes()); // offset 0
        global.extend(varint2(slen));
        global.extend(varint2(slen));
        global.extend(std::iter::repeat_n(b'A', slen));
        global.push(0);
        let gs = global.len() as u32;
        global[4..8].copy_from_slice(&gs.to_le_bytes());

        // type chunk: header 56 (20 + config zero), tabela OFFSET16 zeros
        let cheader = 56usize;
        let entries_start = cheader + count as usize * 2;
        let mut tc = Vec::new();
        tc.extend_from_slice(&RES_TABLE_TYPE_CHUNK.to_le_bytes());
        tc.extend_from_slice(&(cheader as u16).to_le_bytes());
        tc.extend_from_slice(&((entries_start + 16) as u32).to_le_bytes());
        tc.push(1); // type id
        tc.push(FLAG_OFFSET16);
        tc.extend_from_slice(&0u16.to_le_bytes());
        tc.extend_from_slice(&count.to_le_bytes());
        tc.extend_from_slice(&(entries_start as u32).to_le_bytes());
        tc.resize(cheader, 0); // config (zeros)
        tc.extend(std::iter::repeat_n(0u8, count as usize * 2));
        // ResTable_entry {size 8, flags 0, key 0} + Res_value {8, 0, 3, 0}
        tc.extend_from_slice(&8u16.to_le_bytes());
        tc.extend_from_slice(&0u16.to_le_bytes());
        tc.extend_from_slice(&0u32.to_le_bytes());
        tc.extend_from_slice(&8u16.to_le_bytes());
        tc.push(0);
        tc.push(3);
        tc.extend_from_slice(&0u32.to_le_bytes());

        // pacote: header 288 + pool vazio (tipos/chaves) + type chunk
        let mut pkg = Vec::new();
        pkg.extend_from_slice(&RES_TABLE_PACKAGE_TYPE.to_le_bytes());
        pkg.extend_from_slice(&288u16.to_le_bytes());
        pkg.extend_from_slice(&0u32.to_le_bytes()); // size (patched)
        pkg.extend_from_slice(&0x7fu32.to_le_bytes()); // id
        pkg.resize(288, 0);
        pkg[268..272].copy_from_slice(&288u32.to_le_bytes()); // typeStrings
        pkg[276..280].copy_from_slice(&288u32.to_le_bytes()); // keyStrings
        pkg.extend(empty_global_pool());
        pkg.extend_from_slice(&tc);
        let ps = pkg.len() as u32;
        pkg[4..8].copy_from_slice(&ps.to_le_bytes());

        let mut d = Vec::new();
        d.extend_from_slice(&RES_TABLE_TYPE.to_le_bytes());
        d.extend_from_slice(&12u16.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes()); // size (patched)
        d.extend_from_slice(&1u32.to_le_bytes()); // packageCount
        d.extend_from_slice(&global);
        d.extend_from_slice(&pkg);
        let total = d.len() as u32;
        d[4..8].copy_from_slice(&total.to_le_bytes());
        d
    }

    // ─── #246: a tabela/entradas de um ResTable_type ficam no próprio chunk ──

    fn complex_entry_header(key_idx: u32) -> Vec<u8> {
        let mut e = Vec::new();
        e.extend_from_slice(&16u16.to_le_bytes()); // ResTable_entry size
        e.extend_from_slice(&ENTRY_FLAG_COMPLEX.to_le_bytes());
        e.extend_from_slice(&key_idx.to_le_bytes());
        e.extend_from_slice(&0u32.to_le_bytes()); // parent
        e.extend_from_slice(&0u32.to_le_bytes()); // count
        e
    }

    fn dummy_chunk_with_complex(chunk_type: u16, key_idx: u32) -> Vec<u8> {
        let mut c = Vec::new();
        c.extend_from_slice(&chunk_type.to_le_bytes());
        c.extend_from_slice(&8u16.to_le_bytes()); // headerSize
        c.extend_from_slice(&24u32.to_le_bytes()); // 8 + complex header
        c.extend(complex_entry_header(key_idx));
        c
    }

    fn offset16_type_chunk(
        type_id: u8,
        entry_count: u32,
        entries_start: u32,
        offsets: &[u16],
    ) -> Vec<u8> {
        const CHEAD: usize = 56;
        let mut c = Vec::new();
        c.extend_from_slice(&RES_TABLE_TYPE_CHUNK.to_le_bytes());
        c.extend_from_slice(&(CHEAD as u16).to_le_bytes());
        c.extend_from_slice(&((CHEAD + offsets.len() * 2) as u32).to_le_bytes());
        c.push(type_id);
        c.push(FLAG_OFFSET16);
        c.extend_from_slice(&0u16.to_le_bytes());
        c.extend_from_slice(&entry_count.to_le_bytes());
        c.extend_from_slice(&entries_start.to_le_bytes());
        c.resize(CHEAD, 0); // ResTable_config zeros
        for off in offsets {
            c.extend_from_slice(&off.to_le_bytes());
        }
        c
    }

    fn package_with_chunks(chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut pkg = Vec::new();
        pkg.extend_from_slice(&RES_TABLE_PACKAGE_TYPE.to_le_bytes());
        pkg.extend_from_slice(&288u16.to_le_bytes());
        pkg.extend_from_slice(&0u32.to_le_bytes()); // size (patched)
        pkg.extend_from_slice(&0x7fu32.to_le_bytes()); // id
        pkg.resize(288, 0);
        pkg[268..272].copy_from_slice(&288u32.to_le_bytes()); // typeStrings
        pkg[276..280].copy_from_slice(&288u32.to_le_bytes()); // keyStrings
        pkg.extend(empty_global_pool());
        for c in chunks {
            pkg.extend_from_slice(c);
        }
        let size = pkg.len() as u32;
        pkg[4..8].copy_from_slice(&size.to_le_bytes());
        pkg
    }

    // ─── #331 (item 3) and #246 — ported to the lazy index (ADR 014) ─────────

    #[test]
    fn issue_331_aliased_entries_hit_materialization_budget() {
        // ~40 KB de arquivo; sem orçamento, 5_000 clones de 30_000 B
        // = 150 MB materializados. Lazy: o parse é estrutural e passa; o
        // orçamento aparece no ACESSO — ao estourar, resolves viram None.
        let d = aliased_string_arsc(5_000, 30_000);
        let arsc = Arsc::parse(&d).expect("structural parse ok");
        let pkg = &arsc.packages[0];
        let mut materialized = 0usize;
        for i in 0..5_000u32 {
            if pkg.entries_for(0x7f01_0000 + i).is_some() {
                materialized += 1;
            }
        }
        assert!(materialized < 5_000, "budget never tripped");
        assert!(
            materialized * 30_000 <= 128 * 1024 * 1024,
            "string bytes past the budget"
        );
        // E a versão pequena (10 entradas) continua válida.
        let small = aliased_string_arsc(10, 30_000);
        let arsc = Arsc::parse(&small).expect("arsc pequeno válido");
        let s = arsc.resolve_string(0x7f01_0000).expect("string resolve");
        assert_eq!(s.len(), 30_000);
    }

    #[test]
    fn issue_246_type_tables_and_entries_cannot_cross_chunk_bounds() {
        // Chunk 1 declares two OFFSET16 entries but its own size contains
        // only the first table slot. The next word is the following chunk's
        // type (0x0001); treating it as the second offset points into that
        // sibling's payload, where a complex entry header is planted.
        let table_chunk = offset16_type_chunk(1, 2, 62, &[0xFFFF]);
        let sibling_payload_1 = dummy_chunk_with_complex(0x0001, 7);
        // Chunk 2 has its table slot in-bounds, but the offset points past
        // its end into the next sibling's planted complex entry header.
        let entry_chunk = offset16_type_chunk(2, 1, 58, &[2]);
        let sibling_payload_2 = dummy_chunk_with_complex(RES_TABLE_TYPE_SPEC, 8);
        let data = package_with_chunks(&[
            table_chunk,
            sibling_payload_1,
            entry_chunk,
            sibling_payload_2,
        ]);

        let raw = Arc::new(data);
        let budget = Arc::new(ArscBudget::default());
        let pkg = parse_package(&raw, 0, raw.len(), 288, &StringPool::default(), &budget)
            .expect("package with hostile type chunks still parses");
        assert!(
            pkg.entries_for(0x7f01_0001).is_none(),
            "offset table read the sibling chunk header as an offset"
        );
        assert!(
            pkg.entries_for(0x7f02_0000).is_none(),
            "entry header was read from the sibling chunk"
        );
    }

    #[test]
    fn issue_246_package_string_pools_cannot_read_past_package_end() {
        let mut data = Vec::new();
        data.extend_from_slice(&RES_TABLE_PACKAGE_TYPE.to_le_bytes());
        data.extend_from_slice(&288u16.to_le_bytes());
        data.extend_from_slice(&288u32.to_le_bytes()); // package size
        data.extend_from_slice(&0x7fu32.to_le_bytes());
        data.resize(288, 0);
        data[268..272].copy_from_slice(&288u32.to_le_bytes());
        data[276..280].copy_from_slice(&288u32.to_le_bytes());
        // A perfectly valid empty pool, but entirely AFTER the package chunk.
        data.extend(empty_global_pool());

        let raw = Arc::new(data);
        let budget = Arc::new(ArscBudget::default());
        let err = parse_package(&raw, 0, 288, 288, &StringPool::default(), &budget)
            .expect_err("package pools must live inside the package chunk");
        assert_eq!(err.code, "PARSE_ERROR");
    }
}
