//! Reuse the unchanged upstream APK fixture source without importing its VM.
//! This crate is a dev-dependency only; it is never part of the device graph.
#[path = "../../rd-testsupport/src/apkfix.rs"]
pub mod apkfix;
