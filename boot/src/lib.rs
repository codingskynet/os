#![allow(clippy::identity_op)]
#![feature(allocator_api)]
#![no_std]

pub mod drivers;

pub use drivers::{BootDevice, DriverSet};
#[cfg(target_arch = "riscv64")]
pub use runtime::arch::page_table::LeafAttributes;

mod arch;
mod boot;
mod bump;

use boot::{BootData, BootInfo};
use runtime::dev::dt::Fdt;
use runtime::mm::addr::Pa;

/// Define the architecture handoff for a board's selected device type.
/// Invoke once at the board executable's crate root with `boot::entry!(Devices)`.
/// RV64 boards can select leaf PTE attributes with
/// `boot::entry!(Devices, paging = boot::LeafAttributes::THEAD_C906)`.
#[macro_export]
macro_rules! entry {
    ($devices:ty) => {
        $crate::entry!($devices, paging = $crate::LeafAttributes::DEFAULT);
    };
    ($devices:ty, paging = $paging:expr) => {
        #[cfg(target_arch = "riscv64")]
        #[unsafe(no_mangle)]
        #[unsafe(link_section = ".rodata.rv64_leaf_attributes")]
        pub static _rv64_leaf_attributes: $crate::LeafAttributes = $paging;

        /// Architecture handoff with a board-selected driver set.
        ///
        /// # Safety
        /// Called by boot assembly with a valid stack, cleared BSS and firmware DTB.
        #[unsafe(no_mangle)]
        #[unsafe(link_section = ".init.text")]
        pub unsafe extern "C" fn _start_rust(hart_id: usize, dtb: *const u8) -> ! {
            unsafe { $crate::entry::<$devices>(hart_id, dtb) }
        }
    };
}

/// Common entry, called by the board Rust entry with its selected device type.
///
/// # Safety
///
/// The caller must provide a valid stack and firmware DTB after the assembly
/// handoff, with BSS cleared and only the boot hart entering this path.
#[unsafe(link_section = ".init.text")]
pub unsafe fn entry<D: BootDevice>(hart_id: usize, dtb_ptr: *const u8) -> ! {
    // TODO(improvement): handle early exceptions across MMU setup until runtime installs its trap vector.
    unsafe {
        arch::paging::enable_mmu_and_jump(_after_mmu::<D> as *const u8 as usize, hart_id, dtb_ptr)
    }
}

#[unsafe(link_section = ".init.text")]
unsafe extern "C" fn _after_mmu<D: BootDevice>(hart_id: usize, dtb_ptr: usize) -> ! {
    unsafe {
        let dtb_pa = Pa::new(dtb_ptr);
        let fdt = Fdt::new(dtb_pa.into_va().as_ptr()).unwrap();
        let boot_info = BootInfo {
            boot_cpu_id: hart_id,
            initarfs: include_bytes!("../../artifacts/initarfs"),
            init_path: "/bin/micropython",
            boot_data: BootData::DeviceTree(fdt),
        };

        boot::kernel_boot::<D>(boot_info);
    }
}
