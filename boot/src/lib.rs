#![allow(clippy::identity_op)]
#![feature(allocator_api)]
#![no_std]

mod arch;
mod boot;
mod bump;

use core::arch::global_asm;

use boot::{BootData, BootInfo};
use runtime::dev::dt::Fdt;
use runtime::mm::addr::Pa;

#[cfg(target_arch = "riscv64")]
global_asm!(
    include_str!("arch/rv64/boot.s"),
    kernel_vma_offset = const runtime::arch::consts::KERNEL_VMA_OFFSET,
);

#[cfg(target_arch = "riscv32")]
global_asm!(include_str!("arch/rv32/boot.s"));

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
    // TODO(improvement): handle early exceptions across MMU setup until runtime installs its trap vector.
    unsafe { arch::paging::enable_mmu_and_jump(_after_mmu as *const u8 as usize, hart_id, dtb_ptr) }
}

#[unsafe(link_section = ".init.text")]
unsafe extern "C" fn _after_mmu(hart_id: usize, dtb_ptr: usize) -> ! {
    unsafe {
        let dtb_pa = Pa::new(dtb_ptr);
        let fdt = Fdt::new(dtb_pa.into_va().as_ptr()).unwrap();
        let boot_info = BootInfo {
            boot_cpu_id: hart_id,
            initarfs: include_bytes!("../../artifacts/initarfs"),
            init_path: "/bin/micropython",
            boot_data: BootData::DeviceTree(fdt),
        };

        boot::kernel_boot(boot_info);
    }
}
