//! Permanent MMIO mappings created during device installation on the boot hart.
//!
//! The MMIO window is separate from the RAM direct map. Mappings are supervisor
//! read/write and non-executable. Device registers use IO mappings. Reserved
//! buffers can request a different memory type in the same window.
//! Hotplug, unmapping and remote TLB shootdowns are not implemented.
mod layout;
use core::marker::PhantomData;
use core::num::NonZeroUsize;

pub use layout::Error;
use layout::Layout;

use crate::arch::consts::*;
use crate::arch::paging::MemoryType;
use crate::arch::{asm, paging};
use crate::mm::addr::{Pa, Va};

/// One boot-time allocator of MMIO virtual addresses. Dropping this object
/// leaves the mappings intact for the lifetime of the kernel.
pub struct Mapper {
    next: Option<usize>,
    _boot_hart: PhantomData<*mut ()>,
}
impl Mapper {
    /// # Safety
    /// Construct once, after activating the final kernel page table and
    /// initializing the page allocator. The MMIO window must be empty. Complete
    /// every mapping before cloning address spaces or releasing other harts.
    /// Keep that root active and local interrupts disabled while using this mapper.
    pub unsafe fn new() -> Self {
        Self {
            next: Some(MMIO_VMA_BASE),
            _boot_hart: PhantomData,
        }
    }

    /// Map a device register resource and return its virtual base, preserving
    /// an unaligned resource's page offset. No hardware registers are accessed.
    ///
    /// # Safety
    /// The resource must describe device memory with suitable platform PMAs.
    /// The caller owns device initialization and must not create conflicting
    /// RAM/cacheable aliases. The boot-time requirements of new() still apply.
    pub unsafe fn map(&mut self, physical: usize, size: NonZeroUsize) -> Result<Va, Error> {
        unsafe { self.map_with_type(physical, size, MemoryType::IO) }
    }

    /// Map a device resource or reserved buffer with the requested access type.
    ///
    /// # Safety
    /// The caller must own the resource and ensure the type is suitable for
    /// every mapped page, including any aliases. If PTE memory types are
    /// unavailable, the physical memory attributes must supply the requested
    /// behavior. The boot-time requirements of [`Self::new`] apply.
    pub unsafe fn map_with_type(
        &mut self,
        physical: usize,
        size: NonZeroUsize,
        memory_type: MemoryType,
    ) -> Result<Va, Error> {
        let last = MMIO_VMA_END - 1;
        let layout = Layout::new(physical, size, self.next, last, PAGE_SIZE.get())?;
        for page in 0..layout.pages {
            let offset = page * PAGE_SIZE.get();
            let va = Va::new(layout.virtual_start + offset);
            let pa = Pa::new(layout.physical_start + offset);
            if !unsafe {
                paging::map_kernel_page_to_active(
                    va,
                    pa,
                    paging::Permission::R | paging::Permission::W,
                    memory_type,
                )
            } {
                // Undo only leaves installed by this call; retain any existing mapping.
                for mapped in 0..page {
                    unsafe {
                        paging::unmap_page_from_active(Va::new(
                            layout.virtual_start + mapped * PAGE_SIZE.get(),
                        ))
                    };
                }
                asm::page_table::flush_tlb();
                return Err(Error::AlreadyMapped);
            }
        }
        asm::page_table::flush_tlb();
        self.next = layout.next;
        Ok(Va::new(layout.address))
    }
}
