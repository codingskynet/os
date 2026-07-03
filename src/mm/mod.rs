pub mod addr;
pub mod buddy;
pub mod page_meta;
pub mod region;
pub mod slab;

use core::alloc::{GlobalAlloc, Layout};
use core::num::NonZeroUsize;
use core::{mem, ptr};

use crate::arch::consts::PAGE_SIZE;
use crate::mm::addr::{Pa, Va};
use crate::mm::buddy::BuddyAllocator;
use crate::mm::page_meta::{PageMeta, PageMetaMap};
use crate::mm::slab::SlabAllocator;
use crate::sync::SpinLock;

pub static PAGE_META_MAP: PageMetaMap = PageMetaMap::empty();

pub static BUDDY: SpinLock<BuddyAllocator> = SpinLock::new(BuddyAllocator::empty());

#[global_allocator]
pub static GLOBAL: Allocator = Allocator::new();

pub fn page_meta_at(addr: Pa) -> &'static PageMeta {
    PAGE_META_MAP.page_meta(addr)
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

    fn alloc_buddy(layout: Layout) -> *mut u8 {
        let size = layout.size().max(layout.align()).max(PAGE_SIZE.get());
        let Some(size) = NonZeroUsize::new(size) else {
            return ptr::null_mut();
        };

        match BUDDY.lock().alloc(size) {
            Some(page) => {
                let ptr = page.addr().into_va().as_mut_ptr();
                mem::forget(page);
                ptr
            }
            None => ptr::null_mut(),
        }
    }

    unsafe fn dealloc_buddy(ptr: *mut u8) {
        let page_meta = page_meta_at(Va::new(ptr as usize).into_pa());
        let page = unsafe { page_meta.owned_buddy() };

        BUDDY.lock().free(page);
    }
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match Self::slab_index(layout) {
            Some(index) => self.slabs[index].lock().alloc(layout),
            None => Self::alloc_buddy(layout),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() {
            return;
        }

        match Self::slab_index(layout) {
            Some(index) => unsafe { self.slabs[index].lock().dealloc(ptr) },
            None => unsafe { Self::dealloc_buddy(ptr) },
        }
    }
}
