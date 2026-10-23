#![no_std]
#[no_mangle]
pub extern "C" fn Java_org_androswitch_fixture_RustBridge_calculate(_environment:usize,_class:usize,a:i32,b:i64,c:f64)->i64 {
    b.wrapping_add(a as i64).wrapping_add((c*16.0) as i64)
}
#[panic_handler]
fn panic(_info:&core::panic::PanicInfo<'_>)->!{loop{core::hint::spin_loop();}}
