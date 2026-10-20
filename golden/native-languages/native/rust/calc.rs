#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]
use core::ffi::c_void;
struct Mixer { state:u32 }
impl Mixer {
    fn new()->Self {Self{state:2166136261u32 ^ 3}}
    fn feed(&mut self, value:i32) {
        for byte in value.to_le_bytes() {self.state=(self.state ^ u32::from(byte)).wrapping_mul(16777619);}
    }
    fn result(&self)->i32 {(self.state&0x7fffffff) as i32}
}
#[no_mangle]
pub extern "C" fn Java_org_rustydroid_nativeworkbench_MainActivity_digest(_env:*mut c_void,_class:*mut c_void,a:i32,b:i32)->i32 {
    let mut mixer=Mixer::new();mixer.feed(a);mixer.feed(b);mixer.result()
}
#[no_mangle]
pub extern "C" fn Java_org_rustydroid_nativeworkbench_MainActivity_nativeFlavor(_env:*mut c_void,_class:*mut c_void)->i32 {3}
#[panic_handler]
fn panic(_info:&core::panic::PanicInfo<'_>)->! {loop {}}
