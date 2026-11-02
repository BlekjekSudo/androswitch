#![no_main]
//! Fuzz: parser ELF (`rd_ndk::elf::Elf::parse`) sob entradas malformadas —
//! deve rejeitar com erro, nunca entrar em pânico nem alocar sem limite.
use libfuzzer_sys::fuzz_target;
use rd_ndk::elf::Elf;

fuzz_target!(|data: &[u8]| {
    let _ = Elf::parse(data);
});
