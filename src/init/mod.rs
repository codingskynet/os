use alloc::boxed::Box;

use crate::arch::switch::_switch_to;
use crate::kernel::thread::{self, Thread, ThreadState};
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

    let mut idle = Thread::new(|| {
        loop {
            core::hint::spin_loop();
            thread::yield_now();
        }
    });

    idle.state = ThreadState::Running;
    unsafe { _switch_to(Box::into_raw(idle).as_ref().unwrap().regs()) }
}
