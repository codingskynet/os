//! Destructive smoke test for a kernel stack crossing its guard page.

use core::arch::naked_asm;
use core::sync::atomic::Ordering;

use crate::kernel::thread::{self, CurrentThread};
use crate::mm::addr::Va;
use crate::printlnk;

pub fn smoke() -> ! {
    let exit_code = thread::spawn(|| {
        let bottom = CurrentThread::with_mut(|thread| thread.stack_bottom());

        printlnk!("smoke-kernel-stack-overflow: trigger guard below {bottom}");

        // SAFETY: this is an intentionally destructive smoke test. The assembly
        // simulates a downward-growing stack crossing its lower bound. The normal
        // stack is no longer usable afterward, so the guard-page fault must switch
        // to the core-local panic stack and end in the kernel panic handler.
        unsafe { cross_guard(bottom) }
    });

    while exit_code.load(Ordering::Relaxed) == isize::MIN {
        thread::yield_now();
    }

    panic!("kernel-stack overflow thread returned without panicking");
}

#[unsafe(naked)]
unsafe extern "C" fn cross_guard(_bottom: Va) -> ! {
    naked_asm! {
        "mv sp, a0",
        "addi sp, sp, -16",
        "sd zero, 0(sp)",
        "unimp"
    }
}
