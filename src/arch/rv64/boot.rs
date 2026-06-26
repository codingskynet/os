use core::arch::global_asm;

global_asm!(include_str!("boot.s"));

#[unsafe(no_mangle)]
pub unsafe fn _start_rust() -> ! {
    unsafe { crate::kernel_init() }
}
