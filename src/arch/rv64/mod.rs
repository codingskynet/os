//! RISC-V 64-bit architecture support.
//!
//! *Boot* — low-level assembly entry (`_start`), BSS zeroing, stack setup.
//! *Trap* —   (TODO) trap vector, exception / interrupt handling.
//! *Paging* — (TODO) page table management (satp).
//! *Timer* —  (TODO) RISC-V timer (mtime/mtimecmp).
//! *Context* — (TODO) context-switch assembly (`__switch`).

pub use paging::init_page_table;

pub mod asm;
pub mod consts;
mod paging;

use core::arch::global_asm;

use crate::boot::{BootData, BootInfo};
use crate::dev::dt::Fdt;
use crate::mm::addr::Pa;

global_asm!(include_str!("boot.s"));

/// Rust entry point, called from boot.s.
///
/// # Safety
///
/// This function is called directly from assembly (`boot.s`) before the Rust
/// runtime is initialized.  The caller must guarantee:
///
/// * A valid stack pointer (`sp`) has been set up.
/// * The BSS section has been zeroed.
/// * This function is only entered once on the boot hart (hart 0).
///
/// # Debug note
///
/// At this point the MMU is **not yet enabled** — the CPU is still executing
/// from the physical load address (`KERNEL_LMA_BASE = 0x8000_0000`), while
/// the kernel image is linked at a high virtual address (`KERNEL_VMA_BASE`).
///
/// This means linker symbols like `_kernel_start` resolve to **VMA** addresses.
/// Dereferencing them directly would read from the wrong physical location
/// (or page-fault).  `print!`, `println!` or any routine touching statics
/// will likely fault.
///
/// The only reliable debug tool available here is `panic!()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start_rust(hart_id: usize, dtb_ptr: *const u8) -> ! {
    unsafe { paging::enable_mmu_and_jump(after_mmu as *const () as usize, hart_id, dtb_ptr) }
}

unsafe extern "C" fn after_mmu(hart_id: usize, dtb_ptr: Pa) -> ! {
    unsafe {
        let boot_info = BootInfo {
            boot_cpu_id: hart_id,
            boot_data: BootData::DeviceTree(Fdt::new(dtb_ptr.into_va().as_ptr()).unwrap()),
        };

        crate::boot::kernel_boot(boot_info);
    }

    loop {
        core::hint::spin_loop();
    }
}
