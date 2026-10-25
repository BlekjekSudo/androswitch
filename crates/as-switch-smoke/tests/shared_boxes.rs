include!("../../rd-vm/tests/fixtures/portable-boxes/contracts.rs");

#[test]
#[allow(clippy::assertions_on_constants)]
fn uses_the_actual_device_vm_path() {
    assert!(rd_vm::PORTABLE_CORE, "run this package in isolation");
}
