//! RISC-V 64-bit architecture support.
//!
//! *Boot* — low-level assembly entry (`_start`), BSS zeroing, stack setup.
//! *Trap* —   (TODO) trap vector, exception / interrupt handling.
//! *Paging* — (TODO) page table management (satp).
//! *Timer* —  (TODO) RISC-V timer (mtime/mtimecmp).
//! *Context* — (TODO) context-switch assembly (`__switch`).

pub use paging::init_page_table;

pub mod consts;
mod paging;

use core::arch::global_asm;

use crate::boot::{BootData, BootInfo};
use crate::dev::dt::memory::find_memory_reg;
use crate::dev::dt::{Fdt, RegIter};
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
pub unsafe extern "C" fn _start_rust(hart_id: usize, dtb_ptr: Pa) -> ! {
    unsafe {
        let fdt = Fdt::new(dtb_ptr.as_raw() as *const u8);
        let (reg, ac, sc) = find_memory_reg(&fdt).expect("No memory");
        let (memory_start, memory_size) = RegIter::new(reg, ac, sc)
            .filter_map(|reg| match reg {
                (addr, Some(size)) => Some((addr as usize, size as usize)),
                _ => None,
            })
            .next()
            .expect("No memory");
        paging::enable_mmu_and_jump(
            after_mmu as *const () as usize,
            hart_id,
            dtb_ptr.as_raw(),
            Pa::new(memory_start),
            Pa::new(memory_start + memory_size),
        );
    }
}

unsafe extern "C" fn after_mmu(hart_id: usize, dtb_pa: usize) -> ! {
    let boot_info = BootInfo {
        boot_cpu_id: hart_id,
        boot_data: BootData::DeviceTree(Fdt::new(Pa::new(dtb_pa).into_va().as_ptr())),
    };

    unsafe {
        crate::boot::kernel_boot(boot_info);
    }

    loop {
        core::hint::spin_loop();
    }
}
