//! Fixtures de teste compartilhados do RustyDroid.
//!
//! Extraído de `crates/rd-vm/tests/common` (issue #56: o e2e do CLI precisa
//! dos MESMOS fixtures determinísticos — nenhum fixture binário commitado,
//! tudo gerado byte a byte e auditável, mesma política do DexBuilder).
//!
//! - `apkfix`: ZIP (STORED) + string pools + AXML + ARSC mínimos-corretos.
//! - `vm`: DexBuilder (DEX sintético) + helpers de engine/invoke.
//!
//! Crate de DEV-DEPENDENCY: nunca é dependência de runtime dos crates.

pub mod apkfix;
pub mod trivial_app;
pub mod vm;

pub mod m9_app;
