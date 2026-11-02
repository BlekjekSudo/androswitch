//! fd-devices — perfis de device + orçamentos (Apêndice A2: piso `moto-e5`).
//!
//! Os perfis são **contratos**: estourar budget do piso = bug bloqueante (Lei 8).
//! Fonte: `data/devices.toml` (override em disco) ou embutido no binário.

use rd_apk::{RdError, RdResult};
use serde::{Deserialize, Serialize};

pub const MODULE_ID: &str = "fd-devices";
pub const EMBEDDED_DEVICES_TOML: &str = include_str!("../../../data/devices.toml");

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeviceProfile {
    pub id: String,
    pub label: String,
    /// phone | tablet | foldable | tv | wear
    pub class: String,
    pub width_px: u32,
    pub height_px: u32,
    pub density_dpi: u32,
    pub diagonal_in: f32,
    pub aspect_ratio: String,
    pub refresh_hz: u32,
    pub cpu_cores: u32,
    /// Fração de um core do host por thread de app (ex.: 0.25 no piso E5).
    pub cpu_single_thread_factor: f32,
    /// Total de cores do host que o app pode usar.
    pub cpu_quota_cores: f32,
    pub gpu: String,
    pub fill_rate_budget: String,
    pub ram_device_mb: u32,
    /// Teto de RSS do processo RustyDroid.
    pub ram_process_cap_mb: u32,
    /// Equivalente a dalvik.vm.heapgrowthlimit.
    pub ram_app_heap_mb: u32,
    pub storage_quota_gb: u32,
    pub io_profile: String,
    /// API level do SO de fábrica (comportamentos de sistema).
    pub factory_api: u32,
    pub min_api: u32,
    pub max_api: u32,
    #[serde(default)]
    pub flags: Vec<String>,
    #[serde(default)]
    pub notes: String,
}

impl DeviceProfile {
    /// É o piso de hardware canônico?
    pub fn is_hardware_floor(&self) -> bool {
        self.flags.iter().any(|f| f == "hardware-floor")
    }

    /// Dump amigável dos orçamentos (contrato A2).
    pub fn budget_summary(&self) -> String {
        format!(
            "CPU ≤{:.2} cores ({}×{}%) · RSS ≤{} MB · heap ≤{} MB · {}×{} @{}dpi {}Hz · quota {} GB ({})",
            self.cpu_quota_cores,
            self.cpu_cores,
            (self.cpu_single_thread_factor * 100.0) as u32,
            self.ram_process_cap_mb,
            self.ram_app_heap_mb,
            self.width_px,
            self.height_px,
            self.density_dpi,
            self.refresh_hz,
            self.storage_quota_gb,
            self.io_profile
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceTable {
    #[serde(default = "one")]
    pub schema_version: u32,
    #[serde(default)]
    pub device: Vec<DeviceProfile>,
}

fn one() -> u32 {
    1
}

impl DeviceTable {
    pub fn from_toml(contents: &str) -> RdResult<Self> {
        toml::from_str(contents)
            .map_err(|e| RdError::new("DATA_ERROR", format!("devices.toml: {e}"), MODULE_ID))
    }

    pub fn embedded() -> RdResult<Self> {
        Self::from_toml(EMBEDDED_DEVICES_TOML)
    }

    /// Carrega do disco se existir, senão embutido.
    pub fn load(dir: Option<&std::path::Path>) -> RdResult<Self> {
        match dir {
            Some(d) => {
                let path = d.join("devices.toml");
                if path.exists() {
                    let s = std::fs::read_to_string(&path)
                        .map_err(|e| RdError::io(&e, format!("reading {}", path.display())))?;
                    Self::from_toml(&s)
                } else {
                    Self::embedded()
                }
            }
            None => Self::embedded(),
        }
    }

    pub fn get(&self, id: &str) -> Option<&DeviceProfile> {
        self.device.iter().find(|d| d.id == id)
    }

    /// O piso de hardware — presente por contrato.
    pub fn hardware_floor(&self) -> RdResult<&DeviceProfile> {
        self.device
            .iter()
            .find(|d| d.is_hardware_floor())
            .ok_or_else(|| {
                RdError::new(
                    "DATA_ERROR",
                    "no device flagged hardware-floor in devices.toml",
                    MODULE_ID,
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_table_has_floor_contract() {
        let table = DeviceTable::embedded().unwrap();
        assert!(table.device.len() >= 6);
        let e5 = table.hardware_floor().unwrap();
        assert_eq!(e5.id, "moto-e5");
        // contrato A2 exato
        assert_eq!(e5.ram_process_cap_mb, 1024);
        assert_eq!(e5.ram_app_heap_mb, 256);
        assert_eq!(e5.ram_device_mb, 2048);
        assert_eq!(e5.width_px, 720);
        assert_eq!(e5.height_px, 1440);
        assert_eq!(e5.density_dpi, 280);
        assert_eq!(e5.factory_api, 26);
        assert!((e5.cpu_single_thread_factor - 0.25).abs() < f32::EPSILON);
        assert_eq!(e5.cpu_cores, 4);
    }

    #[test]
    fn all_profiles_are_coherent() {
        let table = DeviceTable::embedded().unwrap();
        for d in &table.device {
            assert!(d.min_api >= 26, "{}: min_api abaixo do piso 26", d.id);
            assert!(d.max_api <= 36, "{}: max_api acima do teto 36", d.id);
            assert!(d.ram_app_heap_mb <= d.ram_process_cap_mb);
            assert!(d.cpu_single_thread_factor > 0.0 && d.cpu_single_thread_factor <= 1.0);
        }
    }
}
