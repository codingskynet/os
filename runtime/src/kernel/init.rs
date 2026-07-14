use crate::kernel::thread;
use crate::{fs, printlnk};

// This must stay in regular .text after the .init.text phase ends, so do not
// let it inline into init-only code.
#[inline(never)]
pub fn kernel_init() -> ! {
    printlnk!("hello, init!");

    thread::spawn(|| {
        fs::init();

        #[cfg(debug_assertions)]
        crate::debug::smoke();

        // fs::kernel_exec("/bin/just_return").expect("failed to run kernel exec");

        loop {
            core::hint::spin_loop();
        }
    });

    thread::jump_to_idle();
}
