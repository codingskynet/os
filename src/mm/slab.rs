use core::alloc::Layout;
use core::num::NonZeroUsize;
use core::ops::DerefMut;
use core::ptr;

use crate::arch::consts::PAGE_SIZE;
use crate::mm::addr::Va;
use crate::mm::page_meta::SharedPageMeta;
use crate::mm::{BUDDY, page_meta_at};
use crate::util::linked_list::LinkedList;

pub struct SlabAllocator {
    slab_size: NonZeroUsize,
    block_size: NonZeroUsize,
    available: LinkedList<SharedPageMeta>,
}

impl SlabAllocator {
    pub const fn new(size: usize) -> Self {
        assert!(size.count_ones() == 1);
        let block = if size * 4 > PAGE_SIZE.get() {
            size * 4
        } else {
            PAGE_SIZE.get()
        };
        Self {
            slab_size: NonZeroUsize::new(size).unwrap(),
            block_size: NonZeroUsize::new(block).unwrap(),
            available: LinkedList::new(),
        }
    }

    pub fn alloc(&mut self, layout: Layout) -> *mut u8 {
        debug_assert!(layout.size() <= self.slab_size.get());
        debug_assert!(layout.align() <= self.slab_size.get());

        if let Some(block) = self.available.head() {
            let ptr = self.alloc_from_block(block);
            if block.is_full() {
                unsafe { self.available.remove(block) };
            }
            return ptr;
        }

        let block = match self.request_block() {
            Some(page) => page,
            None => return ptr::null_mut(),
        };

        let ptr = self.alloc_from_block(block);
        if !block.is_full() {
            unsafe { self.available.push_front(block) };
        }

        ptr
    }

    pub unsafe fn dealloc(&mut self, ptr: *mut u8) {
        let slab = Va::new(ptr as usize);
        let block = unsafe { self.block_from_slab(slab) };
        let base = block.addr().into_va();

        debug_assert_eq!((slab.as_raw() - base.as_raw()) % self.slab_size.get(), 0);
        debug_assert!(slab.as_raw() < base.as_raw() + self.block_size.get());

        let was_full = block.is_full();
        self.free_to_block(block, slab);

        if was_full {
            unsafe { self.available.push_front(block) };
        }
    }

    fn alloc_from_block(&self, mut block: SharedPageMeta) -> *mut u8 {
        let meta = block.deref_mut();
        debug_assert_eq!(meta.size, self.slab_size);
        debug_assert!(meta.used < self.capacity());

        let slab = meta.free.take().unwrap();
        meta.free = unsafe { slab.as_ptr::<Option<Va>>().read() };
        meta.used += 1;
        slab.as_mut_ptr()
    }

    fn free_to_block(&self, mut block: SharedPageMeta, slab: Va) {
        let meta = block.deref_mut();
        debug_assert_eq!(meta.size, self.slab_size);
        debug_assert!(meta.used <= self.capacity());

        let used = meta.used.checked_sub(1).expect("invalid slab dealloc");
        unsafe { slab.as_mut_ptr::<Option<Va>>().write(meta.free.take()) };
        meta.free = Some(slab);
        meta.used = used;
    }

    unsafe fn block_from_slab(&self, slab: Va) -> SharedPageMeta {
        let base = Va::new(slab.as_raw() & !(self.block_size.get() - 1));
        let page_meta = page_meta_at(base.into_pa());
        let meta = unsafe { SharedPageMeta::new(page_meta) };

        debug_assert_eq!(meta.addr().into_va(), base);
        debug_assert_eq!(meta.size, self.slab_size);

        meta
    }

    fn capacity(&self) -> usize {
        self.block_size.get() / self.slab_size.get()
    }

    fn request_block(&mut self) -> Option<SharedPageMeta> {
        let mut owned = BUDDY
            .lock()
            .alloc(self.block_size)?
            .into_slab(self.slab_size);

        let start = owned.addr().into_va();
        let end = start.checked_offset(self.block_size.get()).unwrap();

        owned.used = 0;
        owned.free = Some(start);

        let mut node = start;
        let mut next = node.checked_offset(self.slab_size.get()).unwrap();
        while next < end {
            unsafe { node.as_mut_ptr::<Option<Va>>().write(Some(next)) };
            node = next;
            next = node.checked_offset(self.slab_size.get()).unwrap();
        }
        unsafe { node.as_mut_ptr::<Option<Va>>().write(None) };

        Some(owned.into_shared())
    }
}
