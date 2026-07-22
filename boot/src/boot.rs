//! Common boot handoff into the runtime kernel.
//!
//! Architecture-specific entry code builds [`BootInfo`], enables the initial
//! address space, and then calls [`kernel_boot`] to initialize runtime-owned
//! global state.

use runtime::arch::consts::PAGE_SIZE;
use runtime::arch::external::ExternalMeta;
use runtime::dev::console::ConsoleConfig;
use runtime::dev::dt::{Fdt, prop};
use runtime::kernel::clock::ClockMeta;
use runtime::kernel::console::Console;
use runtime::kernel::per_core::PerCore;
use runtime::mm::addr::Pa;
use runtime::mm::page_meta::{PageMeta, PageMetaMap, PageMetaSection};
use runtime::mm::{BUDDY, PAGE_META_MAP};
use runtime::printlnk;

use crate::arch;
use crate::bump::{BUMP_ALLOCATOR, BumpAllocator};

/// Information passed from architecture-specific boot code to common boot.
pub struct BootInfo {
    /// Hardware identifier of the CPU that entered the common kernel path.
    pub boot_cpu_id: usize,
    /// Platform description handed over by firmware or the bootloader.
    pub boot_data: BootData,
}

/// Platform data supplied by firmware or the bootloader.
pub enum BootData {
    /// Flattened Device Tree pointer, commonly used by RISC-V and ARM systems.
    DeviceTree(Fdt),
}

/// Kernel entry point.
///
/// `boot_info` describes the boot CPU and any platform data supplied by the
/// architecture-specific entry code.
///
/// # Safety
///
/// It must be called with a valid stack pointer and BSS already zeroed.
#[unsafe(link_section = ".init.text")]
pub unsafe fn kernel_boot(boot_info: BootInfo) -> ! {
    match &boot_info.boot_data {
        BootData::DeviceTree(fdt) => {
            ClockMeta::init(fdt).expect("failed to initialize clock");
            let model = fdt
                .query()
                .prop("model")
                .and_then(prop::Value::into_str)
                .unwrap_or("(unknown)");
            printlnk!("dtb: FDT detected, model = \"{model}\"");

            {
                let mut alloc = BUMP_ALLOCATOR.lock();
                alloc.init(fdt);
                init_page_metadata(&mut alloc);
            }

            let console = ConsoleConfig::from_fdt(fdt).expect("failed to locate console config");
            Console::install(&console).expect("failed to install console");
            unsafe { arch::paging::init_page_table(fdt, &console) };
            PerCore::init(fdt, boot_info.boot_cpu_id);
            ExternalMeta::init(fdt, &console).expect("failed to initialize external interrupts");
        }
    }

    runtime::kernel::init::kernel_init();
}

#[unsafe(link_section = ".init.text")]
fn init_page_metadata(allocator: &mut BumpAllocator) {
    extern crate alloc;
    use alloc::boxed::Box;

    let mut page_meta_map = PageMetaMap::empty();
    for memory in allocator.memories_mut() {
        let memory_region = memory.region();
        let offset = memory_region.start.align_down(PAGE_SIZE).as_raw() / PAGE_SIZE.get();
        let end = memory_region.end.align_up(PAGE_SIZE).as_raw() / PAGE_SIZE.get();
        let len = end - offset;
        let page_meta_items = unsafe {
            let mut items = Box::<[PageMeta], _>::new_uninit_slice_in(len, &*memory);
            for (i, page_meta) in items.iter_mut().enumerate() {
                page_meta.write(PageMeta::uninit(Pa::new((offset + i) * PAGE_SIZE.get())));
            }
            let items = items.assume_init();
            let (page_meta_items, _) = Box::into_raw_with_allocator(items); // never deallocate
            &mut *page_meta_items
        };

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

        page_meta_map.add(PageMetaSection::new(page_meta_items, offset, memory_region));
    }

    // Page-table allocation and destruction may now resolve raw physical
    // addresses back to their metadata.
    PAGE_META_MAP.get_or_init(|| page_meta_map);
}
