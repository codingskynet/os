//! Runtime memory management.
//!
//! The memory subsystem owns physical page metadata, the buddy page allocator,
//! slab allocators for small objects, and the global allocator entry point used
//! by `alloc`.

pub use context::MmContext;
pub use pages::Pages;

pub mod addr;
pub mod buddy;
mod context;
pub mod page_meta;
mod pages;
pub mod region;
pub mod slab;

use core::alloc::{GlobalAlloc, Layout};
use core::num::NonZeroUsize;
use core::{mem, ptr};

use crate::arch;
use crate::arch::consts::PAGE_SIZE;
use crate::kernel::sync::SpinLock;
use crate::kernel::sync::freezable::Freezable;
use crate::mm::addr::{Pa, Va};
use crate::mm::buddy::BuddyAllocator;
use crate::mm::page_meta::{PageMeta, PageMetaMap};
use crate::mm::slab::SlabAllocator;

pub static PAGE_META_MAP: Freezable<PageMetaMap> = Freezable::new(PageMetaMap::empty());

pub static BUDDY: SpinLock<BuddyAllocator> = SpinLock::new(BuddyAllocator::empty());

#[global_allocator]
pub static GLOBAL: Allocator = Allocator::new();

pub fn page_meta_at(addr: Pa) -> &'static PageMeta {
    PAGE_META_MAP.page_meta(addr)
}

pub fn is_same_page_meta_section(lhs: Pa, rhs: Pa) -> bool {
    PAGE_META_MAP.is_same_section(lhs, rhs)
}

pub fn free_init() {
    let init = crate::arch::region::init();
    assert_eq!(init.start.align_down(PAGE_SIZE), init.start);
    assert_eq!(init.end.align_down(PAGE_SIZE), init.end);

    let mut pa = init.start;
    while pa < init.end {
        let va = pa.into_kernel_va();
        arch::page_table::unmap_from_active(va);
        pa = pa.offset(PAGE_SIZE);
    }

    arch::asm::page_table::flush_tlb();

    let mut buddy = BUDDY.lock();
    let mut pa = init.start;
    while pa < init.end {
        let page = unsafe { page_meta_at(pa).owned_reserved().into_buddy() };
        buddy.free(page);
        pa = pa.offset(PAGE_SIZE);
    }
}

const SLAB_MIN_SIZE: usize = 32;
const SLAB_MAX_SIZE: usize = 4096;
const SLAB_MIN_ORDER: usize = SLAB_MIN_SIZE.trailing_zeros() as usize;
const SLAB_COUNT: usize = 8;

pub struct Allocator {
    // TODO: All slab allocators must be per core
    slabs: [SpinLock<SlabAllocator>; SLAB_COUNT],
}

impl Allocator {
    // This constructor is intentionally used only by the `GLOBAL` static
    // initializer below. Because it is a private `const fn`, the slab table is
    // materialized into the kernel image at compile time instead of being built
    // on a runtime stack. If this ever needs to be called at runtime, revisit
    // this allow and initialize the object in-place instead.
    #[allow(clippy::large_stack_frames, clippy::large_stack_arrays)]
    const fn new() -> Self {
        Self {
            slabs: [
                SpinLock::new(SlabAllocator::new(32)),
                SpinLock::new(SlabAllocator::new(64)),
                SpinLock::new(SlabAllocator::new(128)),
                SpinLock::new(SlabAllocator::new(256)),
                SpinLock::new(SlabAllocator::new(512)),
                SpinLock::new(SlabAllocator::new(1024)),
                SpinLock::new(SlabAllocator::new(2048)),
                SpinLock::new(SlabAllocator::new(4096)),
            ],
        }
    }

    fn slab_index(layout: Layout) -> Option<usize> {
        let size = layout.size().max(layout.align()).max(SLAB_MIN_SIZE);
        if size > SLAB_MAX_SIZE {
            return None;
        }

        Some(size.next_power_of_two().trailing_zeros() as usize - SLAB_MIN_ORDER)
    }
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match Self::slab_index(layout) {
            Some(index) => self.slabs[index].lock().alloc(layout),
            None => {
                let size = layout.size().max(layout.align()).max(PAGE_SIZE.get());
                let pages = size.div_ceil(PAGE_SIZE.get());
                let order = pages.next_power_of_two().trailing_zeros() as usize;
                let size = NonZeroUsize::new(size).unwrap();

                match BUDDY.lock().alloc(size) {
                    Some(page) => {
                        debug_assert_eq!(page.order(), order);
                        let ptr = page.addr().into_va().as_mut_ptr();
                        mem::forget(page);
                        ptr
                    }
                    None => ptr::null_mut(),
                }
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() {
            return;
        }

        match Self::slab_index(layout) {
            Some(index) => unsafe { self.slabs[index].lock().dealloc(ptr) },
            None => {
                let page_meta = page_meta_at(Va::new(ptr as usize).into_pa());
                let page = unsafe { page_meta.owned_buddy() };

                BUDDY.lock().free(page);
            }
        }
    }
}
