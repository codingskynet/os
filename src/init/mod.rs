use crate::kernel::thread;
use crate::printlnk;

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
