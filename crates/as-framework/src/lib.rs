//! Android API contracts on the shared RustyDroid Engine, with no host OS dependency.
#![no_std]
extern crate alloc;
mod android;
mod audio;
pub mod memory;
pub mod properties;
pub use audio::PcmOutput;
mod raster;
use alloc::{
    boxed::Box,
    rc::Rc,
    string::{String, ToString},
    vec::Vec,
};
pub use android::*;
use as_platform::{fs::FileSystem, Clock};
pub use raster::Frame;
use rd_vm::{Engine, VmConfig, VmExit};
pub struct App {
    pub vm: Box<Engine>,
    pub android: Rc<Android>,
    pub frame: Frame,
    pub entry: String,
    pub memory: memory::LoadMemoryReport,
}
#[derive(Default)]
pub struct AppOptions {
    pub memory: memory::LoadMemory,
    /// Actual root-relative immutable installed APK, exposed read-only to guest.
    pub source_apk: Option<String>,
}
impl App {
    pub fn new(
        apk: rd_apk::Apk,
        fs: Rc<dyn FileSystem>,
        clock: Rc<dyn Clock>,
        width: i32,
        height: i32,
        entry: Option<&str>,
    ) -> Result<Self, VmExit> {
        Self::new_with_options(apk, fs, clock, width, height, entry, AppOptions::default())
    }
    pub fn new_with_options(
        apk: rd_apk::Apk,
        fs: Rc<dyn FileSystem>,
        clock: Rc<dyn Clock>,
        width: i32,
        height: i32,
        entry: Option<&str>,
        options: AppOptions,
    ) -> Result<Self, VmExit> {
        if width <= 0 || height <= 0 || width > 1920 || height > 1920 {
            return Err(
                rd_vm::err::vm_error("INVALID_VIEWPORT", "dimensions must be 1..1920").into(),
            );
        }
        let entry = entry
            .map(String::from)
            .or_else(|| {
                apk.manifest
                    .application
                    .components
                    .iter()
                    .find(|c| c.is_launcher && c.enabled != Some(false))
                    .map(|c| {
                        c.target_activity
                            .clone()
                            .unwrap_or_else(|| c.class_name.clone())
                    })
            })
            .ok_or_else(|| {
                rd_vm::err::vm_error(
                    "NO_LAUNCHER_ACTIVITY",
                    "APK has no enabled MAIN/LAUNCHER activity",
                )
            })?;
        // Raw DEX is only one contributor to decoded VM memory. Bound the total
        // before materializing any entry, independently of APK/container size.
        let dex_bytes = apk.dex_files.iter().try_fold(0u64, |total, name| {
            let entry = apk
                .zip
                .find(name)
                .ok_or_else(|| rd_vm::err::vm_error("MISSING_DEX_ENTRY", name))?;
            total
                .checked_add(entry.uncompressed_size)
                .ok_or_else(|| rd_vm::err::vm_error("DEX_SIZE_OVERFLOW", name))
        })?;
        if dex_bytes > options.memory.raw_dex_budget() as u64 {
            return Err(rd_vm::err::vm_error(
                "DEX_MEMORY_BUDGET",
                alloc::format!("combined raw DEX input {:?} bytes exceeds {} bytes for the available load budget {} bytes", dex_bytes, options.memory.raw_dex_budget(), options.memory.budget),
            )
            .into());
        }
        let mut dexes = Vec::new();
        let mut memory = memory::LoadMemoryReport {
            raw_dex_bytes: dex_bytes as usize,
            parser_budget_bytes: options.memory.parser_budget(),
            ..Default::default()
        };
        options.memory.stage("dex_load_begin")?;
        for name in &apk.dex_files {
            let bytes = apk
                .zip
                .read(name)
                .map_err(|e| rd_vm::err::vm_error(&e.code, e.cause))?;
            let (dex, estimate) = rd_dex::Dex::parse_budgeted(
                bytes,
                memory
                    .parser_budget_bytes
                    .saturating_sub(memory.estimated_parser_bytes),
            )?;
            dexes.push(dex);
            memory.estimated_parser_bytes += estimate;
            options.memory.stage("dex_parsed")?;
        }
        let mut vm = Box::new(Engine::new(
            dexes,
            VmConfig {
                fuel: 5_000_000,
                max_depth: 32,
                heap_budget: 64 << 20,
                exception_trace: true,
            },
        ));
        let mut vfs = as_platform::vfs::Vfs::new(fs.clone(), &apk.manifest.package)
            .map_err(|e| rd_vm::err::vm_error(e.code, e.to_string()))?;
        let source = options
            .source_apk
            .map(|path| vfs.bind_apk(path))
            .transpose()
            .map_err(|e| rd_vm::err::vm_error(e.code, e.to_string()))?;
        vm.bind_device_storage(&apk.manifest.package, Box::new(vfs))?;
        let android = Android::new(apk, clock, width, height, fs)?;
        if let Some(source) = source {
            android.bind_package_source(source)?;
        }
        options.memory.stage("vm_ready")?;
        vm.bind_framework(android.clone());
        Ok(Self {
            vm,
            android,
            frame: Frame::new(width as usize, height as usize),
            entry,
            memory,
        })
    }
    pub fn launch(&mut self) -> Result<(), VmExit> {
        self.android.bind_main_thread(&self.vm)?;
        self.android.launch(&mut self.vm, &self.entry)?;
        self.draw()
    }
    pub fn draw(&mut self) -> Result<(), VmExit> {
        self.android.tick(&mut self.vm)?;
        let (width, height) = self.android.software_size();
        if (self.frame.width, self.frame.height) != (width as usize, height as usize) {
            self.frame = Frame::new(width as usize, height as usize);
        }
        self.android.render(&mut self.vm, &mut self.frame)
    }
}
