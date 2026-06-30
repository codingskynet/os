use core::alloc::{GlobalAlloc, Layout};

use crate::mm::addr::Pa;

#[global_allocator]
pub static GLOBAL: SlabAllocator = SlabAllocator::uninit();

pub struct SlabAllocator {
    // nodes for 64B, 128B, 256B, 512B, 1KiB, 2KiB, 4KiB
    heads: [Option<Pa>; 7],
}

impl SlabAllocator {
    const fn uninit() -> Self {
        Self { heads: [None; 7] }
    }

    pub unsafe fn init(&self) {}
}

unsafe impl GlobalAlloc for SlabAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        todo!()
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        todo!()
    }
}
