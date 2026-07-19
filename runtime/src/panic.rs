use core::cell::UnsafeCell;
use core::panic::PanicInfo;

use crate::arch::consts::PAGE_SIZE;
use crate::arch::trap::TrapFrame;
use crate::mm::addr::Va;
use crate::println;

/// Emergency stack used after the boot hart detects a kernel-stack overflow.
///
/// This is deliberately per-core state in shape even though the kernel starts
/// only one hart today. SMP bring-up should replace it with one instance per
/// hart and select the current hart's stack in trap entry.
#[repr(C, align(4096))]
pub(crate) struct PanicStack(UnsafeCell<[u8; PAGE_SIZE.get()]>);

// SAFETY: normal Rust code never accesses the storage. Trap entry selects it
// only for the single running hart after a fatal stack overflow.
unsafe impl Sync for PanicStack {}

pub(crate) static PANIC_STACK: PanicStack = PanicStack(UnsafeCell::new([0; PAGE_SIZE.get()]));

pub(crate) extern "C" fn kernel_stack_overflow(frame: &TrapFrame) -> ! {
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
    println!("kernel panic");

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
