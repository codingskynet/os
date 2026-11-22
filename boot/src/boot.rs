//! Common boot handoff into the runtime kernel.
//!
//! Architecture-specific entry code builds [`BootInfo`], enables the initial
//! address space, and then calls [`kernel_boot`] to initialize runtime-owned
//! global state.

use runtime::arch::consts::*;
use runtime::dev::dt::{Fdt, prop};
use runtime::kernel::clock::ClockMeta;
use runtime::kernel::per_core::PerCore;
use runtime::mm::addr::Pa;
use runtime::mm::page_meta::{PageMeta, PageMetaMap, PageMetaSection};
use runtime::mm::region::Region;
use runtime::mm::{BUDDY, PAGE_META_MAP};
use runtime::printlnk;

use crate::arch;
use crate::bump::{BUMP_ALLOCATOR, BumpAllocator, MAX_RAM_BANKS, MemoryMap};

/// Information passed from architecture-specific boot code to common boot.
pub struct BootInfo {
    /// Hardware identifier of the CPU that entered the common kernel path.
    pub boot_cpu_id: usize,
    /// Platform description handed over by firmware or the bootloader.
    pub boot_data: BootData,
    /// Root filesystem archive retained for the lifetime of the kernel.
    pub initarfs: &'static [u8],
    /// First userspace program to execute.
    pub init_path: &'static str,
}

/// Platform data supplied by firmware or the bootloader.
pub enum BootData {
    /// Flattened Device Tree pointer, commonly used by RISC-V and ARM systems.
    DeviceTree(Fdt),
}

/// Build the direct RAM map and both aliases of the executing kernel image.
///
/// Called before translation is enabled, while linker symbols resolve to physical addresses.
#[unsafe(link_section = ".init.text")]
pub(crate) fn build_bootstrap_mappings(fdt: &Fdt, mut map: impl FnMut(Region, usize)) {
    let kernel = Region::new(
        Pa::new(&raw const _kernel_start as usize),
        Pa::new(&raw const _kernel_end as usize),
    )
    .unwrap();
    let dtb_start = fdt.as_ptr() as usize;
    let dtb_end = dtb_start
        .checked_add(fdt.total_size())
        .expect("DTB range overflow");
    let mut kernel_in_ram = false;
    let mut dtb_in_ram = false;
    MemoryMap::new(fdt).for_each_mappable_ram(|ram| {
        map(ram, DIRECT_VMA_BASE);
        kernel_in_ram |= ram.start <= kernel.start && kernel.end <= ram.end;
        dtb_in_ram |= ram.start <= dtb_start && dtb_end <= ram.end;
    });
    assert!(
        kernel_in_ram && dtb_in_ram,
        "kernel or DTB outside mappable RAM"
    );
    map(kernel, 0);
    map(kernel, kernel_vma_offset());
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
pub unsafe fn kernel_boot<D: crate::BootDevice>(boot_info: BootInfo) -> ! {
    match &boot_info.boot_data {
        BootData::DeviceTree(fdt) => {
            let model = fdt
                .query()
                .prop("model")
                .and_then(prop::Value::into_str)
                .unwrap_or("(unknown)");
            printlnk!("dtb: FDT detected, model = \"{model}\"");

            {
                let mut alloc = BUMP_ALLOCATOR.lock();
                alloc.init(fdt);
                PAGE_META_MAP.get_or_init(|| init_page_metadata(&alloc));
            }

            unsafe { arch::paging::init_page_table(fdt) };

            {
                // TODO: mmio mapping info must be maintained
                // Install boot devices before cloning kernel mappings.
                let mut mmio = unsafe { runtime::mm::mmio::Mapper::new() };
                unsafe { D::DRIVERS.install(fdt, &mut mmio) }.expect("failed to install devices");
                unsafe { D::install_power(fdt, &mut mmio) }.expect("failed to install board power");
                ClockMeta::init();
            }

            // TODO: since after MMIO mapped, the page table cannot be changed right now.
            PerCore::init(fdt, boot_info.boot_cpu_id);
        }
    }

    runtime::kernel::init::kernel_init(boot_info.initarfs, boot_info.init_path);
}

#[unsafe(link_section = ".init.text")]
fn init_page_metadata(allocator: &BumpAllocator) -> PageMetaMap {
    // A fully reserved bank may need its metadata to live in another bank.
    // Allocate every section before handing any of the remaining pages to the buddy.
    let mut sections = arrayvec::ArrayVec::<_, MAX_RAM_BANKS>::new();
    for bank in allocator.banks() {
        let bank_region = bank.region();
        let offset = bank_region.start.align_down(PAGE_SIZE).as_raw() / PAGE_SIZE.get();
        let end = bank_region.end.align_up(PAGE_SIZE).as_raw() / PAGE_SIZE.get();
        let len = end - offset;
        let page_meta_items = allocate_page_metadata(allocator, offset, len);
        sections.push((page_meta_items, offset));
    }

    let mut page_meta_map = PageMetaMap::empty();
    for (bank, (page_meta_items, offset)) in allocator.banks().iter().zip(sections) {
        let bank_region = bank.region();
        let reserved = bank.reserved();
        for page in &mut *page_meta_items {
            let span = page.region();
            if span.start < bank_region.start
                || bank_region.end < span.end
                || reserved.iter().any(|r| r.overlap(span))
            {
                page.owned_uninit().consume_as_reserved();
            }
        }

        BUDDY.lock().initialize_section(&mut *page_meta_items);

        page_meta_map.add(PageMetaSection::new(page_meta_items, offset, bank_region));
    }

    // Page-table allocation and destruction may now resolve raw physical
    // addresses back to their metadata.
    page_meta_map
}

#[unsafe(link_section = ".init.text")]
fn allocate_page_metadata(
    allocator: &BumpAllocator,
    offset: usize,
    len: usize,
) -> &'static mut [PageMeta] {
    extern crate alloc;
    use alloc::boxed::Box;

    let mut items = Box::<[PageMeta], _>::new_uninit_slice_in(len, allocator);
    for (i, page) in items.iter_mut().enumerate() {
        page.write(PageMeta::uninit(Pa::new((offset + i) * PAGE_SIZE.get())));
    }
    // Initialized above. Boot never frees the storage backing the metadata map.
    unsafe {
        let (items, _) = Box::into_raw_with_allocator(items.assume_init());
        &mut *items
    }
}
