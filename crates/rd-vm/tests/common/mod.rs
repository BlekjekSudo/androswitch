//! Shim de compatibilidade: os fixtures vivem em `rd-testsupport` (issue #56
//! — o e2e do CLI usa os MESMOS fixtures determinísticos). Os testes do
//! rd-vm continuam importando via `common::…` sem mudança.
#[allow(unused_imports)] // cada suite usa o que precisa via `common::*`
pub use rd_testsupport::apkfix;
pub use rd_testsupport::vm::*;
