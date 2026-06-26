#![no_main]
#![no_std]

use core::panic::PanicInfo;

mod console;
mod machine;

unsafe fn kernel_init() -> ! {
    println!("hello, world");
    loop {}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
