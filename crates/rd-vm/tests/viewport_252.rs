//! Issue #252: the active viewport follows the display orientation.
//! Portrait (720x1440) stays the default. `landscape` swaps the axes, and the
//! layout, window decor, display list, `ScreenModel` and screenshot all read
//! the same size.

use rd_vm::{Engine, VmConfig};

#[test]
fn default_viewport_is_portrait_720x1440() {
    let vm = Engine::new(vec![], VmConfig::default());
    assert_eq!(vm.viewport_px(), (720, 1440));
}

#[test]
fn landscape_swaps_axes_and_portrait_restores() {
    let mut vm = Engine::new(vec![], VmConfig::default());
    vm.m10_set_orientation("landscape").expect("landscape");
    assert_eq!(vm.viewport_px(), (1440, 720));
    vm.m10_set_orientation("portrait").expect("portrait");
    assert_eq!(vm.viewport_px(), (720, 1440));
}

#[test]
fn unknown_orientation_is_rejected_and_keeps_viewport() {
    let mut vm = Engine::new(vec![], VmConfig::default());
    assert!(vm.m10_set_orientation("diagonal").is_err());
    assert_eq!(vm.viewport_px(), (720, 1440));
}
