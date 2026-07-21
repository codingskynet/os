use core::sync::atomic::{AtomicUsize, Ordering};

use crate::kernel::per_core::PerCore;
use crate::kernel::thread;
use crate::{arch, fs, printlnk};

/// Number of secondary harts that have started their preinstalled idle context.
pub static SECONDARY_ONLINE: AtomicUsize = AtomicUsize::new(0);

// This must stay in regular .text after the .init.text phase ends, so do not
// let it inline into init-only code.
#[inline(never)]
pub fn kernel_init() -> ! {
    printlnk!("hello, init!");

    fs::init();

    init_this_hart();
    start_secondary_harts();

    thread::spawn(|| {
        #[cfg(debug_assertions)]
        crate::debug::smoke();

        fs::kernel_exec("/bin/micropython").expect("failed to run micropython");
    });

    thread::jump_to_scheduler();
}

/// Runtime entry for a secondary hart released by the low-level boot assembly.
///
/// # Safety
///
/// The caller must enter in S-mode with interrupts disabled, the shared kernel
/// page table active, and this hart's `tp` plus the serialized secondary init
/// stack installed by the stackless assembly entry. `hart_id` must identify the
/// calling hardware thread, which must enter exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn secondary_init(hart_id: usize) -> ! {
    debug_assert_eq!(
        PerCore::with_mut(|per_core| per_core.hart_id),
        hart_id,
        "secondary hart entered with the wrong PerCore slot"
    );
    init_this_hart();
    thread::jump_to_scheduler();
}

fn init_this_hart() {
    arch::trap::init();
    arch::timer::init();
}

fn start_secondary_harts() {
    // Every parked hart already knows its hardware ID. The stackless entry
    // assigns PerCore slots atomically, so no dense hart-ID assumption or
    // serial bootstrap-stack handoff is required here.
    unsafe { arch::asm::smp::release_secondary_harts() };

    let expected = PerCore::count() - 1;
    while SECONDARY_ONLINE.load(Ordering::Acquire) < expected {
        core::hint::spin_loop();
    }
}

/// Publish that the current hart is executing on its private idle stack.
///
/// This is invoked once by every idle thread's entry function. The boot hart
/// reaches it only after all secondary bring-up waits have completed, so it is
/// deliberately excluded from the acknowledgement count.
pub fn idle_online() {
    if PerCore::is_boot_core() {
        // All secondaries are already on runtime-owned idle stacks, and this
        // primary has now abandoned boot_stack as well. The complete .init
        // range, including both temporary stacks, can be reclaimed safely.
        crate::mm::free_init();
    } else {
        // This hart has abandoned secondary_stack through _switch_to. The
        // release increment lets the next PerCore-index ticket enter it.
        SECONDARY_ONLINE.fetch_add(1, Ordering::Release);
    }
}
