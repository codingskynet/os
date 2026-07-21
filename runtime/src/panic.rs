use alloc::boxed::Box;
use core::cell::UnsafeCell;
use core::panic::PanicInfo;
use core::ptr;

use crate::arch::consts::PAGE_SIZE;
use crate::arch::trap::TrapFrame;
use crate::kernel::per_core::PerCore;
use crate::mm::addr::Va;
use crate::println;

/// Stack used to report a kernel-stack overflow without touching the exhausted
/// stack that triggered the trap.
#[repr(C, align(4096))]
pub struct PanicStack(UnsafeCell<[u8; PAGE_SIZE.get()]>);

impl PanicStack {
    /// Allocate a zeroed panic stack without materializing it on the caller's
    /// already-constrained kernel stack.
    pub fn allocate() -> Box<Self> {
        let mut stack = Box::<Self>::new_uninit();
        // SAFETY: every bit pattern is valid for the wrapped byte array. The
        // allocation is correctly sized and aligned for PanicStack.
        unsafe {
            ptr::write_bytes(stack.as_mut_ptr(), 0, 1);
            stack.assume_init()
        }
    }
}

pub extern "C" fn kernel_stack_overflow(frame: &TrapFrame) -> ! {
    panic!(
        "kernel stack overflow: cause={:?}, sepc={}, sp={}, stval={}",
        frame.cause(),
        frame.sepc,
        Va::new(frame.regs.sp),
        Va::new(frame.stval),
    )
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("kernel panic from core={}", PerCore::core_id());

    if let Some(location) = info.location() {
        println!(
            "  at {}:{}:{}",
            location.file(),
            location.line(),
            location.column()
        );
    } else {
        println!("  at <unknown>");
    }

    println!("  message: {}", info.message());

    #[allow(clippy::empty_loop)]
    loop {
        core::hint::spin_loop();
    }
}
