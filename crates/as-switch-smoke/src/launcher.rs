//! One NRO owns launcher and one quiescent application session at a time.
use crate::{application::Event, device_fs::LibnxFileSystem, session_report};
use alloc::{
    boxed::Box,
    format,
    rc::Rc,
    string::{String, ToString},
    vec::Vec,
};
use as_launcher::{Key, Selection, Shell};
use as_platform::fs::{self, FileSystem};
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Script {
    schema_version: u32,
    session_id: String,
    commands: Vec<Command>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Key { key: Key },
    Touch { x: i32, y: i32 },
    Open { events: Vec<Event> },
    Frame,
    Frames { count: u32 },
}
struct Launcher {
    shell: Shell,
    fs: Rc<LibnxFileSystem>,
    launch: Option<Selection>,
    script: Option<Script>,
    cursor: usize,
    pending: Option<usize>,
    snapshots: Vec<serde_json::Value>,
    sessions: Vec<serde_json::Value>,
    serial: u64,
    session_prefix: String,
    current_request: Option<serde_json::Value>,
    reported: bool,
    remaining_frames: Option<u32>,
}
fn open() -> Result<Launcher, String> {
    let fs = Rc::new(LibnxFileSystem::new("sdmc:/switch/androswitch").map_err(|e| e.to_string())?);
    let mut shell = Shell::new(fs.clone()).map_err(|e| e.to_string())?;
    if fs
        .stat("logs/pending-session.json")
        .map_err(|e| e.to_string())?
        .is_some()
    {
        let bytes = fs::read_all(fs.as_ref(), "logs/pending-session.json", 64 << 10)
            .map_err(|e| e.to_string())?;
        let pending = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        let report = fs::read_all(fs.as_ref(), "logs/session-report.json", 1 << 20)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        let snapshot = session_report::interrupted(&pending, report.as_ref());
        fs::write_all(
            fs.as_ref(),
            "logs/last-interrupted.json",
            &serde_json::to_vec(&snapshot).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let _ = fs.remove("logs/pending-session.json", false);
        shell.message("Previous session was interrupted. Its identity and matching report are preserved in logs/last-interrupted.json");
    }
    session_report::clear_process_evidence(fs.as_ref()).map_err(|e| e.to_string())?;
    let script = if fs
        .stat("runtime/launcher-test.json")
        .map_err(|e| e.to_string())?
        .is_some()
    {
        let bytes = fs::read_all(fs.as_ref(), "runtime/launcher-test.json", 64 << 10)
            .map_err(|e| e.to_string())?;
        fs.remove("runtime/launcher-test.json", false)
            .map_err(|e| e.to_string())?;
        let value: Script = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if value.schema_version != 1
            || !session_report::valid_session_id(&value.session_id)
            || value.commands.len() > 256
        {
            return Err("INVALID_LAUNCHER_TEST".into());
        }
        Some(value)
    } else {
        None
    };
    let session_prefix = if let Some(script) = &script {
        script.session_id.clone()
    } else {
        #[allow(unsafe_code)]
        extern "C" {
            fn as_reporting_nonce() -> u64;
            fn as_reporting_tick() -> u64;
        }
        // SAFETY: scalar libnx randomness/system-tick reads; no pointers retained.
        #[allow(unsafe_code)]
        unsafe {
            session_report::console_prefix(as_reporting_nonce(), as_reporting_tick())
        }
    };
    Ok(Launcher {
        shell,
        fs,
        launch: None,
        script,
        cursor: 0,
        pending: Some(0),
        snapshots: Vec::new(),
        sessions: Vec::new(),
        serial: 0,
        session_prefix,
        current_request: None,
        reported: false,
        remaining_frames: None,
    })
}
impl Launcher {
    fn prepare(&mut self, selection: Selection, events: Option<&Vec<Event>>) -> Result<(), String> {
        self.serial = self
            .serial
            .checked_add(1)
            .ok_or("SESSION_SERIAL_OVERFLOW")?;
        let session = session_report::session_id(&self.session_prefix, self.serial)
            .ok_or("INVALID_SESSION_ID")?;
        session_report::clear_current_evidence(self.fs.as_ref()).map_err(|e| e.to_string())?;
        let request = serde_json::json!({"schema_version":1,"package":selection.package,"revision":selection.revision,"session_id":session,"events":events});
        let bytes = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
        fs::write_all(self.fs.as_ref(), "runtime/launch.json", &bytes)
            .map_err(|e| e.to_string())?;
        fs::write_all(self.fs.as_ref(), "logs/pending-session.json", &bytes)
            .map_err(|e| e.to_string())?;
        let report = serde_json::json!({"schema_version":1,"status":"running","session_id":session,"package":selection.package,"revision":selection.revision});
        // Previous application images were removed before publishing this
        // generation. Capture readers bind to this identity, not filesystem
        // timestamps, which can follow the guest clock's configured offset.
        let owner = serde_json::json!({"schema_version":1,"capture_protocol":1,
            "session_id":session,"package":selection.package,"revision":selection.revision});
        fs::write_all(
            self.fs.as_ref(),
            "logs/capture-owner.json",
            &serde_json::to_vec(&owner).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        fs::write_all(
            self.fs.as_ref(),
            "logs/session-report.json",
            &serde_json::to_vec(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        self.current_request = Some(request);
        self.launch = Some(selection);
        Ok(())
    }
    fn step(&mut self) -> Result<i32, String> {
        if self.launch.is_some() {
            return Ok(1);
        }
        if self.pending.is_some() {
            return Ok(0);
        }
        let Some(script) = &self.script else {
            return Ok(0);
        };
        if self.cursor >= script.commands.len() {
            if !self.reported {
                let report = serde_json::json!({"schema_version":1,"session_id":script.session_id,"status":"passed_execution","snapshots":self.snapshots,"sessions":self.sessions});
                fs::write_all(
                    self.fs.as_ref(),
                    "logs/launcher-report.json",
                    &serde_json::to_vec(&report).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                self.reported = true;
            }
            return Ok(0);
        }
        let command = &script.commands[self.cursor];
        if let Command::Frames { count } = command {
            let remaining = self.remaining_frames.get_or_insert((*count).min(600));
            if *remaining > 0 {
                *remaining -= 1;
                return Ok(0);
            }
            self.remaining_frames = None;
        }
        let selection = match command {
            Command::Key { key } => self.shell.key(*key),
            Command::Touch { x, y } => self.shell.touch(*x, *y),
            Command::Open { .. } => self.shell.key(Key::Accept),
            Command::Frame | Command::Frames { .. } => None,
        };
        if let Some(selection) = selection {
            let events = match command {
                Command::Open { events } => Some(events.clone()),
                _ => None,
            };
            self.prepare(selection, events.as_ref())?;
        } else if matches!(command, Command::Open { .. }) {
            return Err("LAUNCHER_TEST_OPEN_FAILED: selection did not launch".into());
        }
        self.cursor += 1;
        self.pending = Some(self.cursor);
        Ok(if self.launch.is_some() { 1 } else { 0 })
    }
}
/// No pointers accepted; the returned owner token lives only in the C event loop.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn as_shell_open() -> u64 {
    match open() {
        Ok(shell) => Box::into_raw(Box::new(shell)) as u64,
        Err(_) => 0,
    }
}
/// # Safety
/// Token is a live launcher, uniquely accessed by its owning event loop.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_shell_step(token: u64) -> i32 {
    let l = unsafe { &mut *(token as *mut Launcher) };
    match l.step() {
        Ok(n) => n,
        Err(e) => {
            l.shell.message(e);
            -1
        }
    }
}
/// # Safety
/// Token is live; pixels remain valid until next input/step.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_shell_pixels(token: u64) -> *const u32 {
    unsafe { &*(token as *const Launcher) }
        .shell
        .frame
        .pixels
        .as_ptr()
}
/// # Safety
/// Token is live; only the final automation frame requests a capture.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_shell_capture(token: u64) -> bool {
    let l = unsafe { &*(token as *const Launcher) };
    l.script
        .as_ref()
        .is_some_and(|s| l.pending == Some(s.commands.len()))
}
/// # Safety
/// Token is live. Scalars are copied; key values are defined by the C loop.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_shell_input(token: u64, kind: i32, a: i32, b: i32) {
    let l = unsafe { &mut *(token as *mut Launcher) };
    let selection = if kind == 1 {
        l.shell.touch(a, b)
    } else {
        let key = match a {
            0 => Key::Up,
            1 => Key::Down,
            2 => Key::Left,
            3 => Key::Right,
            4 => Key::Accept,
            5 => Key::Back,
            6 => Key::Details,
            7 => Key::Remove,
            8 => Key::ClearData,
            _ => Key::Refresh,
        };
        l.shell.key(key)
    };
    if let Some(selection) = selection {
        if let Err(e) = l.prepare(selection, None) {
            l.shell.message(e);
        }
    }
}
/// # Safety
/// Token is live; called after the actual framebuffer submission.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_shell_presented(token: u64, hash: u64) {
    let l = unsafe { &mut *(token as *mut Launcher) };
    if let Some(step) = l.pending.take() {
        l.snapshots.push(serde_json::json!({"step":step,"presented_hash":format!("{hash:016x}"),"shell":l.shell.snapshot()}));
    }
}
/// # Safety
/// Token is live. Called only after runtime quiescence and resource cleanup.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_shell_returned(token: u64, success: i32) {
    let l = unsafe { &mut *(token as *mut Launcher) };
    l.launch = None;
    let expected = l.current_request.take().unwrap_or(serde_json::Value::Null);
    let actual = fs::read_all(l.fs.as_ref(), "logs/session-report.json", 1 << 20)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    if let Some(mut report) = actual
        .as_ref()
        .filter(|report| session_report::identity_matches(report, &expected))
        .cloned()
    {
        if let Ok(trace) = fs::read_all(l.fs.as_ref(), "logs/teardown.log", 64 << 10) {
            let phases: Vec<serde_json::Value> = trace
                .split(|b| *b == b'\n')
                .filter_map(|line| serde_json::from_slice(line).ok())
                .collect();
            report["teardown"] = phases.into();
        }
        if report["status"] == "failed" {
            l.shell
                .message(format!("Launch failed: {}", report["error"]));
        } else {
            l.shell
                .message("Application stopped. Ready for another app.");
        }
        l.sessions.push(report);
    } else {
        let diagnostic = session_report::reporting_error(&expected, actual.as_ref());
        if let Ok(bytes) = serde_json::to_vec(&diagnostic) {
            let _ = fs::write_all(l.fs.as_ref(), "logs/reporting-error.json", &bytes);
            let _ = fs::write_all(l.fs.as_ref(), "logs/session-report.json", &bytes);
        }
        l.shell.message("SESSION_REPORT_IDENTITY_MISMATCH: current launch evidence is missing or belongs to another session; see logs/reporting-error.json");
        l.sessions.push(diagnostic);
    }
    let _ = success; // Only called after proved cleanup, including a clean launch error.
    let _ = l.fs.remove("logs/pending-session.json", false);
    let _ = l.fs.remove("runtime/launch.json", false);
    let _ = l.shell.refresh();
}
/// # Safety
/// Consume the live launcher exactly once.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn as_shell_close(token: u64) {
    drop(unsafe { Box::from_raw(token as *mut Launcher) });
}
