use crate::kernel::thread;
use crate::{fs, printlnk};

// This must stay in regular .text after the .init.text phase ends, so do not
// let it inline into init-only code.
#[inline(never)]
pub fn kernel_init() -> ! {
    printlnk!("hello, init!");

    fs::init();

    thread::spawn(|| {
        #[cfg(debug_assertions)]
        crate::debug::smoke();

        fs::kernel_exec("/bin/micropython").expect("failed to run micropython");
    });

    thread::jump_to_idle();
}
