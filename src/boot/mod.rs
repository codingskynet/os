mod phya;

use core::ffi::CStr;

use crate::arch::consts::PAGE_SIZE;
use crate::boot::phya::{PhysicalAllocator, Region};
use crate::dev::dt::memory::find_memory_reg;
use crate::dev::dt::{Fdt, RegIter};
use crate::init::kernel_init;
use crate::mm::BUDDY;
use crate::mm::addr::Pa;
use crate::mm::page::{Page, PageMeta};
use crate::util::debug::dump_page_list;
use crate::{console, println};

pub struct BootInfo {
    /// Hardware identifier of the CPU that entered the common kernel path.
    pub boot_cpu_id: usize,
    /// Platform description handed over by firmware or the bootloader.
    pub boot_data: BootData,
}

pub enum BootData {
    /// Flattened Device Tree pointer, commonly used by RISC-V and ARM systems.
    DeviceTree(Fdt),
}

/// Kernel entry point
///
/// `boot_info` describes the boot CPU and any platform data supplied by the
/// architecture-specific entry code.
///
/// # Safety
/// It must be called with a valid stack pointer and BSS already zeroed.
pub unsafe fn kernel_boot(boot_info: BootInfo) {
    unsafe {
        let metadata = match &boot_info.boot_data {
            BootData::DeviceTree(fdt) => {
                let model = fdt
                    .query()
                    .prop("model")
                    .and_then(|s| CStr::from_bytes_with_nul(s).ok())
                    .and_then(|s| s.to_str().ok())
                    .unwrap_or("(unknown)");
                println!("dtb: FDT detected, model = \"{}\"", model);
                if let Err(e) = console::install_from_fdt(fdt) {
                    println!("dtb: Failed to install console: {:?}", e);
                }

                let (reg, ac, sc) = find_memory_reg(fdt).expect("No memory");
                let region = RegIter::new(reg, ac, sc)
                    .filter_map(|reg| {
                        if let (addr, Some(size)) = reg {
                            Some(Region::from_size(
                                Pa::new(addr as usize),
                                (size as usize).try_into().ok()?,
                            )?)
                        } else {
                            None
                        }
                    })
                    .next()
                    .expect("No Memory");

                let mut allocator =
                    PhysicalAllocator::new(region).expect("Failed to init PhysicalAllocator");
                crate::arch::init_page_table(region.start, region.end, || {
                    allocator
                        .alloc_uninit()
                        .expect("Failed to allocate PageTable")
                });
                init_page_metadata(allocator, region)
            }
        };

        dump_page_list(&metadata);

        BUDDY.as_mut().initialize(metadata);
        println!("{:#?}", BUDDY.as_mut());
        kernel_init();
    }
}

fn init_page_metadata(mut allocator: PhysicalAllocator, region: Region) -> PageMeta {
    let offset = region.start.align(PAGE_SIZE).as_raw() / PAGE_SIZE;
    let end = region.end.align(PAGE_SIZE).as_raw() / PAGE_SIZE;
    let len = end - offset;
    let pages = allocator
        .alloc_slice(len, |i| Page::free(Pa::new((offset + i) * PAGE_SIZE.get())))
        .expect("Failed to allocate page metadata");

    for reserved in allocator.reserved_iter() {
        let start = reserved.start.align(PAGE_SIZE).as_raw() / PAGE_SIZE - offset;
        let end = reserved.end.align(PAGE_SIZE).as_raw() / PAGE_SIZE - offset;
        for page in &mut pages[start..end] {
            page.reserve();
        }
    }

    PageMeta::new(pages, offset)
}
