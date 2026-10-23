//! Console navigation and package operations shared by Horizon and host tests.
#![no_std]
extern crate alloc;
mod png;
use alloc::{
    format,
    rc::Rc,
    string::{String, ToString},
    vec::Vec,
};
use as_framework::Frame;
use as_package::{PackageManager, Record};
use as_platform::{
    fs::{self, FileSystem},
    PlatformError,
};
use serde::{Deserialize, Serialize};
/// Container remains memory-resident in this alpha. Large OBB files use VFS
/// streaming; increasing the APK limit requires a positioned ZIP container.
pub const APK_MEMORY_LIMIT: usize = 64 << 20;
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Selection {
    pub package: String,
    pub revision: u64,
}
#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Accept,
    Back,
    Details,
    Remove,
    ClearData,
    Refresh,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Tab {
    Installed,
    Apks,
}
#[derive(Serialize)]
pub struct Snapshot {
    pub tab: Tab,
    pub selected: usize,
    pub installed: Vec<Record>,
    pub apks: Vec<String>,
    pub message: String,
    pub details: bool,
    pub confirmation: Option<String>,
    pub frame_hash: String,
    pub decoded_icons: usize,
}
#[derive(Clone, Copy)]
enum Confirmation {
    Remove,
    ClearData,
}
pub struct Shell {
    fs: Rc<dyn FileSystem>,
    pub frame: Frame,
    installed: Vec<Record>,
    apks: Vec<String>,
    icons: Vec<Option<png::Icon>>,
    tab: Tab,
    selected: usize,
    message: String,
    details: bool,
    confirmation: Option<Confirmation>,
}
impl Shell {
    pub fn new(fs: Rc<dyn FileSystem>) -> Result<Self, PlatformError> {
        PackageManager::new(fs.clone()).initialize()?;
        let mut shell = Self {
            fs,
            frame: Frame::new(640, 360),
            installed: Vec::new(),
            apks: Vec::new(),
            icons: Vec::new(),
            tab: Tab::Installed,
            selected: 0,
            message: "Ready. L/R changes list; A opens selection.".into(),
            details: false,
            confirmation: None,
        };
        shell.refresh()?;
        Ok(shell)
    }
    pub fn message(&mut self, message: impl Into<String>) {
        self.message = message.into();
        self.draw();
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            tab: self.tab,
            selected: self.selected,
            installed: self.installed.clone(),
            apks: self.apks.clone(),
            message: self.message.clone(),
            details: self.details,
            confirmation: self.confirmation.map(|c| match c {
                Confirmation::Remove => "uninstall_keep_data".into(),
                Confirmation::ClearData => "clear_internal_data".into(),
            }),
            frame_hash: format!("{:016x}", self.frame.checksum()),
            decoded_icons: self.icons.iter().filter(|i| i.is_some()).count(),
        }
    }
    pub fn refresh(&mut self) -> Result<(), PlatformError> {
        self.installed = PackageManager::new(self.fs.clone()).list()?;
        self.installed.sort_by(|a, b| {
            a.label
                .as_deref()
                .unwrap_or(&a.package)
                .cmp(b.label.as_deref().unwrap_or(&b.package))
        });
        self.apks = self
            .fs
            .list("apks")?
            .into_iter()
            .filter(|e| !e.directory && e.name.to_ascii_lowercase().ends_with(".apk"))
            .map(|e| e.name)
            .collect();
        self.apks.sort();
        self.icons = self.installed.iter().map(|r| self.icon(r)).collect();
        self.selected = self.selected.min(self.count().saturating_sub(1));
        self.draw();
        Ok(())
    }
    fn icon(&self, r: &Record) -> Option<png::Icon> {
        let bytes = fs::read_all(
            self.fs.as_ref(),
            &format!("installed/{}/{:020}/package.apk", r.package, r.revision),
            APK_MEMORY_LIMIT,
        )
        .ok()?;
        let apk = rd_apk::Apk::from_bytes(bytes).ok()?;
        let id = apk.manifest.application.icon_res?;
        let path = apk.arsc.as_ref()?.resolve_string(id)?;
        let bytes = apk.zip.read(&path).ok()?;
        png::decode(&bytes)
    }
    fn count(&self) -> usize {
        match self.tab {
            Tab::Installed => self.installed.len(),
            Tab::Apks => self.apks.len(),
        }
    }
    pub fn key(&mut self, key: Key) -> Option<Selection> {
        match self.act(key) {
            Ok(value) => {
                self.draw();
                value
            }
            Err(e) => {
                self.message = e.to_string();
                self.confirmation = None;
                self.draw();
                None
            }
        }
    }
    fn act(&mut self, key: Key) -> Result<Option<Selection>, PlatformError> {
        if let Some(confirm) = self.confirmation {
            match key {
                Key::Back => {
                    self.confirmation = None;
                    self.message = "Cancelled".into();
                }
                Key::Accept => {
                    if let Some(record) = self.installed.get(self.selected).cloned() {
                        let pm = PackageManager::new(self.fs.clone());
                        match confirm {
                            Confirmation::Remove => {
                                pm.uninstall(&record.package)?;
                                self.message =
                                    format!("Removed {}. Data retained.", record.package);
                            }
                            Confirmation::ClearData => {
                                pm.clear_data(&record.package)?;
                                self.message = format!("Internal data cleared: {}", record.package);
                            }
                        }
                        self.confirmation = None;
                        self.details = false;
                        self.refresh()?;
                    }
                }
                _ => {}
            }
            return Ok(None);
        }
        match key {
            Key::Up => self.selected = self.selected.saturating_sub(1),
            Key::Down => self.selected = (self.selected + 1).min(self.count().saturating_sub(1)),
            Key::Left | Key::Right => {
                self.tab = if self.tab == Tab::Installed {
                    Tab::Apks
                } else {
                    Tab::Installed
                };
                self.selected = 0;
                self.details = false;
            }
            Key::Back => self.details = false,
            Key::Refresh => {
                self.refresh()?;
                self.message = "Lists refreshed".into();
            }
            Key::Details => {
                self.details = !self.details;
            }
            Key::Remove | Key::ClearData
                if self.tab == Tab::Installed && self.installed.get(self.selected).is_some() =>
            {
                self.confirmation = Some(if matches!(key, Key::Remove) {
                    Confirmation::Remove
                } else {
                    Confirmation::ClearData
                });
                self.message = if matches!(key, Key::Remove) {
                    "Uninstall? Saved data will be retained. A confirm / B cancel".into()
                } else {
                    "Erase internal data? This cannot be undone. A confirm / B cancel".into()
                };
            }
            Key::Accept => match self.tab {
                Tab::Installed => {
                    if let Some(r) = self.installed.get(self.selected) {
                        return Ok(Some(Selection {
                            package: r.package.clone(),
                            revision: r.revision,
                        }));
                    }
                }
                Tab::Apks => {
                    if let Some(file) = self.apks.get(self.selected).cloned() {
                        self.message = format!("Installing {file}...");
                        let bytes = fs::read_all(
                            self.fs.as_ref(),
                            &format!("apks/{file}"),
                            APK_MEMORY_LIMIT,
                        )?;
                        let before = self.installed.clone();
                        let record =
                            PackageManager::new(self.fs.clone()).install(bytes, true, None)?;
                        self.refresh()?;
                        self.tab = Tab::Installed;
                        self.selected = self
                            .installed
                            .iter()
                            .position(|r| r.package == record.package)
                            .unwrap_or(0);
                        self.details = false;
                        let unchanged = before.iter().any(|r| r == &record);
                        self.message = format!(
                            "{} / rev {}. A: open",
                            if unchanged {
                                "Already installed"
                            } else {
                                "Installed"
                            },
                            record.revision
                        );
                    }
                }
            },
            _ => {}
        }
        Ok(None)
    }
    pub fn touch(&mut self, x: i32, y: i32) -> Option<Selection> {
        if y < 56 {
            return if x < 320 && self.tab != Tab::Installed || x >= 320 && self.tab != Tab::Apks {
                self.key(Key::Right)
            } else {
                None
            };
        }
        if (64..288).contains(&y) {
            let index = (self.selected / 4) * 4 + ((y - 64) / 56) as usize;
            if index < self.count() {
                if index == self.selected {
                    return self.key(Key::Accept);
                }
                self.selected = index;
                self.draw();
            }
        }
        None
    }
    pub fn draw(&mut self) {
        let f = &mut self.frame;
        f.clear(0xff11191f);
        f.rect(0, 0, 640, 57, 0xff1b2b35);
        f.text("AndroSwitch", 20, 25, 22., 0xffe5f1f5);
        f.text("Android runtime / HorizonOS", 220, 24, 14., 0xff8ba6b4);
        for (x, label, active) in [
            (20, "Installed", self.tab == Tab::Installed),
            (340, "APKs", self.tab == Tab::Apks),
        ] {
            f.text(
                label,
                x,
                49,
                16.,
                if active { 0xff55d9c0 } else { 0xff8ba6b4 },
            );
        }
        let start = (self.selected / 4) * 4;
        match self.tab {
            Tab::Installed => {
                for (i, r) in self.installed.iter().enumerate().skip(start).take(4) {
                    let y = 64 + (i - start) as i32 * 56;
                    let selected = i == self.selected;
                    f.rect(
                        12,
                        y,
                        616,
                        52,
                        if selected { 0xff24484c } else { 0xff192931 },
                    );
                    if let Some(icon) = self.icons.get(i).and_then(|v| v.as_ref()) {
                        icon.draw(f, 20, y + 4, 44);
                    } else {
                        f.rect(20, y + 4, 44, 44, 0xff39696c);
                        f.text("A", 31, y + 35, 26., 0xffb7f0df);
                    }
                    f.text(
                        &r.label
                            .as_deref()
                            .unwrap_or(&r.package)
                            .chars()
                            .take(43)
                            .collect::<String>(),
                        76,
                        y + 23,
                        18.,
                        0xfff0f6f7,
                    );
                    f.text(
                        &format!("{}  /  rev {}", r.package, r.revision)
                            .chars()
                            .take(65)
                            .collect::<String>(),
                        76,
                        y + 43,
                        12.,
                        0xff94b0ba,
                    );
                }
            }
            Tab::Apks => {
                for (i, name) in self.apks.iter().enumerate().skip(start).take(4) {
                    let y = 64 + (i - start) as i32 * 56;
                    f.rect(
                        12,
                        y,
                        616,
                        52,
                        if i == self.selected {
                            0xff24484c
                        } else {
                            0xff192931
                        },
                    );
                    f.text("APK", 24, y + 32, 18., 0xff55d9c0);
                    f.text(
                        &name.chars().take(48).collect::<String>(),
                        82,
                        y + 31,
                        17.,
                        0xffeff6f7,
                    );
                }
            }
        }
        if self.details && self.confirmation.is_none() {
            f.rect(12, 60, 616, 230, 0xff20343e);
            let lines = if self.tab == Tab::Installed {
                self.installed
                    .get(self.selected)
                    .map(|r| {
                        alloc::vec![
                            r.package.clone(),
                            format!(
                                "Version {} / revision {}",
                                r.version_name.as_deref().unwrap_or("?"),
                                r.revision
                            ),
                            format!("Entry: {}", r.entry_activity.as_deref().unwrap_or("none")),
                            format!("APK: {} bytes / ARM64 + DEX", r.apk_bytes),
                            "Compatibility is determined when launched.".into(),
                            "Y uninstall (keep data). ZR clear data (separate).".into()
                        ]
                    })
                    .unwrap_or_default()
            } else {
                alloc::vec![
                    "Drop single APK files in switch/androswitch/apks/".into(),
                    "A installs or updates; existing data is retained.".into(),
                    "API 26, ARM64. Missing APIs show a named error.".into()
                ]
            };
            for (i, line) in lines.iter().enumerate() {
                f.text(
                    &line.chars().take(74).collect::<String>(),
                    24,
                    88 + i as i32 * 28,
                    14.,
                    0xffd8e8ed,
                );
            }
        }
        let message: Vec<char> = self.message.chars().collect();
        for (i, chunk) in message.chunks(87).take(2).enumerate() {
            f.text(
                &chunk.iter().collect::<String>(),
                16,
                306 + i as i32 * 17,
                12.,
                if self.confirmation.is_some() {
                    0xffffc36b
                } else {
                    0xff93b0bc
                },
            );
        }
        f.rect(0, 333, 640, 27, 0xff1b2b35);
        f.text(
            "A open/install   B back   X details   Y remove   L/R list   - refresh   + exit",
            12,
            351,
            12.,
            0xffbad0d9,
        );
    }
}
