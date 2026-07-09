use crate::kernel::thread;
use crate::printlnk;

// This must stay in regular .text after the .init.text phase ends, so do not
// let it inline into init-only code.
#[inline(never)]
pub fn kernel_init() -> ! {
    printlnk!("hello, init!");

    thread::spawn(|| {
        #[cfg(debug_assertions)]
        crate::debug::smoke();

        printlnk!("hello, kernel thread!");

        loop {
            core::hint::spin_loop();
        }
    });

    thread::jump_to_idle();
}
