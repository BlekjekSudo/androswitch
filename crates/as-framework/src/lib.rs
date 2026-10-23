//! Android API contracts on the shared RustyDroid Engine, with no host OS dependency.
#![no_std]
extern crate alloc;
mod android;
mod audio;
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
        let mut dexes = Vec::new();
        for name in &apk.dex_files {
            let bytes = apk
                .zip
                .read(name)
                .map_err(|e| rd_vm::err::vm_error(&e.code, e.cause))?;
            let dex = rd_dex::Dex::parse(bytes)?;
            if !dex.verify_checksum() {
                return Err(rd_vm::err::vm_error("INVALID_CHECKSUM", name).into());
            }
            dexes.push(dex);
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
        let vfs = as_platform::vfs::Vfs::new(fs, &apk.manifest.package)
            .map_err(|e| rd_vm::err::vm_error(e.code, e.to_string()))?;
        vm.bind_device_storage(&apk.manifest.package, Box::new(vfs))?;
        let android = Android::new(apk, clock, width, height);
        vm.bind_framework(android.clone());
        Ok(Self {
            vm,
            android,
            frame: Frame::new(width as usize, height as usize),
            entry,
        })
    }
    pub fn launch(&mut self) -> Result<(), VmExit> {
        self.android.launch(&mut self.vm, &self.entry)?;
        self.draw()
    }
    pub fn draw(&mut self) -> Result<(), VmExit> {
        self.android.tick(&mut self.vm)?;
        self.android.render(&mut self.vm, &mut self.frame)
    }
}
