include!("../../rd-vm/tests/fixtures/portable-boxes/contracts.rs");
mod bit_utilities {
    include!("../../rd-vm/tests/fixtures/portable-boxes/bit_contracts.rs");
}

#[test]
#[allow(clippy::assertions_on_constants)]
fn uses_the_actual_device_vm_path() {
    assert!(rd_vm::PORTABLE_CORE, "run this package in isolation");
}
