mod bump;

use crate::arch::consts::PAGE_SIZE;
use crate::boot::bump::{Alloc, BumpAllocator};
use crate::dev::dt::{Fdt, prop};
use crate::init::kernel_init;
use crate::mm::addr::Pa;
use crate::mm::page_meta::{PageMeta, PageMetaSection};
use crate::mm::{BUDDY, PAGE_META_MAP};
use crate::{console, println};

#[allow(unused)]
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
        match &boot_info.boot_data {
            BootData::DeviceTree(fdt) => {
                let model = fdt
                    .query()
                    .prop("model")
                    .and_then(prop::Value::into_str)
                    .unwrap_or("(unknown)");
                println!("dtb: FDT detected, model = \"{}\"", model);
                if let Err(e) = console::install_from_fdt(fdt) {
                    println!("dtb: Failed to install console: {:?}", e);
                }

                let mut allocator =
                    BumpAllocator::new(fdt).expect("Failed to init PhysicalAllocator");
                crate::arch::init_page_table(fdt, || {
                    allocator
                        .alloc_uninit()
                        .expect("Failed to allocate PageTable")
                });
                init_page_metadata(allocator);
            }
        }

        kernel_init();
    }
}

fn init_page_metadata(mut allocator: BumpAllocator) {
    for memory in allocator.memories_mut() {
        let memory_region = memory.region();
        let offset = memory_region.start.align_down(PAGE_SIZE).as_raw() / PAGE_SIZE.get();
        let end = memory_region.end.align_up(PAGE_SIZE).as_raw() / PAGE_SIZE.get();
        let len = end - offset;
        let page_meta_items = memory
            .alloc_slice(len, |i| {
                PageMeta::uninit(Pa::new((offset + i) * PAGE_SIZE.get()))
            })
            .expect("Failed to allocate page metadata");

        // reserve outside RAM region
        for page_meta in &mut *page_meta_items {
            let page_region = page_meta.region();
            if page_region.start < memory_region.start || memory_region.end < page_region.end {
                page_meta.owned_uninit().consume_as_reserved();
            }
        }

        for reserved in memory.reserved() {
            // TODO: improve by selecting range
            for page_meta in &mut *page_meta_items {
                if !page_meta.is_uninit() {
                    continue;
                }
                let page_region = page_meta.region();
                if reserved.overlap(page_region) {
                    page_meta.owned_uninit().consume_as_reserved();
                }
            }
        }

        BUDDY.lock().initialize_section(&mut *page_meta_items);

        // SAFETY: page metadata is initialized during single-threaded boot,
        // before allocator hot paths can read PAGE_META_MAP concurrently.
        unsafe { PAGE_META_MAP.add(PageMetaSection::new(page_meta_items, offset, memory_region)) };
    }
}
