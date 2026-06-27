use core::arch::global_asm;

use crate::dev::dt::Fdt;
use crate::kernel::init::{BootData, BootInfo};

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
#[unsafe(no_mangle)]
pub unsafe fn _start_rust(hart_id: usize, dtb_ptr: usize) -> ! {
    let boot_info = BootInfo {
        boot_cpu_id: hart_id,
        boot_data: BootData::DeviceTree(Fdt::new(dtb_ptr)),
    };

    unsafe { crate::kernel::init::kernel_init(boot_info) }
}
