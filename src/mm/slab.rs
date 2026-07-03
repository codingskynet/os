use core::alloc::Layout;
use core::num::NonZeroUsize;
use core::ptr;

use crate::arch::consts::PAGE_SIZE;
use crate::mm::addr::Va;
use crate::mm::page_meta::SlabPageHandle;
use crate::mm::{BUDDY, page_meta_at};

const OBJECTS_PER_BLOCK: usize = 4;

pub struct SlabAllocator {
    size: NonZeroUsize,
    block: NonZeroUsize,
    available: Option<SlabPageHandle>,
}

// TODO
unsafe impl Send for SlabAllocator {}

// TODO
unsafe impl Sync for SlabAllocator {}

impl SlabAllocator {
    pub const fn new(size: usize) -> Self {
        assert!(size.count_ones() == 1);
        Self {
            size: NonZeroUsize::new(size).unwrap(),
            block: NonZeroUsize::new(block_size(size)).unwrap(),
            available: None,
        }
    }

    pub fn alloc(&mut self, layout: Layout) -> *mut u8 {
        debug_assert!(layout.size() <= self.size.get());
        debug_assert!(layout.align() <= self.size.get());

        if let Some(page) = self.available {
            let ptr = self.alloc_from_page(page);
            if page.is_full() {
                self.remove(page);
            }
            return ptr;
        }

        let page = match self.request_block() {
            Some(page) => page,
            None => return ptr::null_mut(),
        };

        let ptr = self.alloc_from_page(page);
        if !page.is_full() {
            self.push_front(page);
        }

        ptr
    }

    pub unsafe fn dealloc(&mut self, ptr: *mut u8) {
        let node = Va::new(ptr as usize);
        let page = self.page_from_object(node);
        let base = page.addr().into_va();

        debug_assert_eq!((node.as_raw() - base.as_raw()) % self.size.get(), 0);
        debug_assert!(node.as_raw() < base.as_raw() + self.block.get());

        let was_full = page.is_full();
        self.free_to_page(page, node);

        if was_full {
            self.push_front(page);
        }
    }

    fn alloc_from_page(&self, mut page: SlabPageHandle) -> *mut u8 {
        let node = {
            let slab = unsafe { page.slab_mut() };
            debug_assert_eq!(slab.size, self.size);
            debug_assert!(slab.used < self.capacity());

            let node = slab.free.take().unwrap();
            slab.free = unsafe { node.as_ptr::<Option<Va>>().read() };
            slab.used += 1;
            node
        };

        node.as_mut_ptr()
    }

    fn free_to_page(&self, mut page: SlabPageHandle, node: Va) {
        let slab = unsafe { page.slab_mut() };
        debug_assert_eq!(slab.size, self.size);
        debug_assert!(slab.used <= self.capacity());

        let used = slab.used.checked_sub(1).expect("invalid slab dealloc");
        unsafe { node.as_mut_ptr::<Option<Va>>().write(slab.free.take()) };
        slab.free = Some(node);
        slab.used = used;
    }

    fn page_from_object(&self, node: Va) -> SlabPageHandle {
        let base = self.block_base(node);
        let page_meta = page_meta_at(base.into_pa());
        let page = unsafe { SlabPageHandle::new_unchecked(page_meta) };

        debug_assert_eq!(page.addr().into_va(), base);
        debug_assert_eq!(page.slab().size, self.size);

        page
    }

    fn block_base(&self, addr: Va) -> Va {
        let block = self.block.get();
        debug_assert!(block.is_power_of_two());
        Va::new(addr.as_raw() & !(block - 1))
    }

    fn capacity(&self) -> usize {
        self.block.get() / self.size.get()
    }

    fn request_block(&mut self) -> Option<SlabPageHandle> {
        let mut owned = BUDDY.lock().alloc(self.block)?.into_slab(self.size);

        let start = owned.addr().into_va();
        let end = start.checked_offset(self.block.get()).unwrap();

        owned.used = 0;
        owned.free = Some(start);
        owned.listed = false;
        owned.prev = None;
        owned.next = None;

        let mut node = start;
        let mut next = node.checked_offset(self.size.get()).unwrap();
        while next < end {
            unsafe { node.as_mut_ptr::<Option<Va>>().write(Some(next)) };
            node = next;
            next = node.checked_offset(self.size.get()).unwrap();
        }
        unsafe { node.as_mut_ptr::<Option<Va>>().write(None) };

        Some(owned.into_handle())
    }

    fn push_front(&mut self, mut page: SlabPageHandle) {
        let head = self.available;

        {
            let slab = unsafe { page.slab_mut() };
            debug_assert!(!slab.listed);
            debug_assert!(slab.prev.is_none());
            debug_assert!(slab.next.is_none());

            slab.listed = true;
            slab.prev = None;
            slab.next = head;
        }

        if let Some(mut head) = head {
            let slab = unsafe { head.slab_mut() };
            debug_assert!(slab.prev.is_none());
            slab.prev = Some(page);
        }

        self.available = Some(page);
    }

    fn remove(&mut self, mut page: SlabPageHandle) {
        let (prev, next) = {
            let slab = unsafe { page.slab_mut() };
            debug_assert!(slab.listed);
            (slab.prev, slab.next)
        };

        if let Some(mut prev) = prev {
            let slab = unsafe { prev.slab_mut() };
            debug_assert!(slab.next == Some(page));
            slab.next = next;
        } else {
            debug_assert!(self.available == Some(page));
            self.available = next;
        }

        if let Some(mut next) = next {
            let slab = unsafe { next.slab_mut() };
            debug_assert!(slab.prev == Some(page));
            slab.prev = prev;
        }

        let slab = unsafe { page.slab_mut() };
        slab.listed = false;
        slab.prev = None;
        slab.next = None;
    }
}

const fn block_size(size: usize) -> usize {
    let requested = size * OBJECTS_PER_BLOCK;
    if requested < PAGE_SIZE.get() {
        PAGE_SIZE.get()
    } else {
        requested
    }
}
