#![allow(clippy::identity_op)]
#![no_main]
#![no_std]

mod arch;
mod boot;
mod bump;

use core::arch::global_asm;

use boot::{BootData, BootInfo};
use runtime::dev::dt::Fdt;
use runtime::mm::addr::Pa;

global_asm!(include_str!("arch/rv64/boot.s"));

/// Rust entry point, called from boot.s.
///
/// # Safety
///
/// This function is called directly from assembly before the Rust runtime is
/// initialized. The caller must guarantee that a valid stack pointer has been
/// installed, BSS has been zeroed, and only the boot hart enters this path.
#[unsafe(link_section = ".init.text")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start_rust(hart_id: usize, dtb_ptr: *const u8) -> ! {
    unsafe { arch::paging::enable_mmu_and_jump(_after_mmu as *const u8 as usize, hart_id, dtb_ptr) }
}

#[unsafe(link_section = ".init.text")]
unsafe extern "C" fn _after_mmu(hart_id: usize, dtb_ptr: Pa) -> ! {
    unsafe {
        let boot_info = BootInfo {
            boot_cpu_id: hart_id,
            boot_data: BootData::DeviceTree(Fdt::new(dtb_ptr.into_va().as_ptr()).unwrap()),
        };

        boot::kernel_boot(boot_info);
    }
}
