//! Fixed-size slab allocator used by the global allocator.
//!
//! Each allocator instance serves one power-of-two size class. Slab blocks are
//! borrowed from the buddy allocator and returned once all objects in the block
//! have been freed.

use core::alloc::Layout;
use core::num::NonZeroUsize;
use core::ops::DerefMut;
use core::ptr;

use crate::arch::consts::PAGE_SIZE;
use crate::mm::addr::Va;
use crate::mm::page_meta::{SharedPageMeta, Slab};
use crate::mm::{BUDDY, page_meta_at};
use crate::util::linked_list::Pointer;

/// Allocator for one small-object size class.
///
/// The free list is stored inside freed objects. The allocator's block list
/// contains only blocks that currently have at least one free object.
pub struct SlabAllocator {
    slab_size: NonZeroUsize,
    block_size: NonZeroUsize,
    head: Option<SharedPageMeta<Slab>>,
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
            head: None,
        }
    }

    pub fn alloc(&mut self, layout: Layout) -> *mut u8 {
        debug_assert!(layout.size() <= self.slab_size.get());
        debug_assert!(layout.align() <= self.slab_size.get());

        if let Some(mut block) = self.head {
            let ptr = self.alloc_from_block(block);
            if block.is_full() {
                self.head = block.node().next();
                block.pop();
            }
            return ptr;
        }

        let block = match self.request_block() {
            Some(page) => page,
            None => return ptr::null_mut(),
        };

        let ptr = self.alloc_from_block(block);
        if !block.is_full() {
            self.insert(block);
        }

        ptr
    }

    /// Return a previously allocated slab object to this allocator.
    ///
    /// # Safety
    ///
    /// `ptr` must have been returned by this slab allocator for the same size
    /// class and must not have been freed already.
    pub unsafe fn dealloc(&mut self, ptr: *mut u8) {
        let slab = Va::new(ptr as usize);
        let block = unsafe { self.block_from_slab(slab) };
        let base = block.addr().into_va();

        debug_assert_eq!((slab.as_raw() - base.as_raw()) % self.slab_size.get(), 0);
        debug_assert!(slab.as_raw() < base.as_raw() + self.block_size.get());

        let was_full = block.is_full();
        self.free_to_block(block, slab);

        if block.is_empty() {
            self.remove(block);
            let page = unsafe { block.into_owned() }.into_buddy();
            BUDDY.lock().free(page);
        } else if was_full {
            self.insert(block);
        }
    }

    fn insert(&mut self, block: SharedPageMeta<Slab>) {
        if let Some(head) = self.head {
            head.push_front(block);
        }
        self.head = Some(block);
    }

    fn remove(&mut self, mut block: SharedPageMeta<Slab>) {
        if self.head == Some(block) {
            self.head = block.node().next();
        }
        block.pop();
    }

    fn alloc_from_block(&self, mut block: SharedPageMeta<Slab>) -> *mut u8 {
        let meta = block.deref_mut();
        debug_assert_eq!(meta.size, self.slab_size);
        debug_assert!(meta.used < self.capacity());

        let slab = meta.free.take().unwrap();
        meta.free = unsafe { slab.as_ptr::<Option<Va>>().read() };
        meta.used += 1;
        slab.as_mut_ptr()
    }

    fn free_to_block(&self, mut block: SharedPageMeta<Slab>, slab: Va) {
        let meta = block.deref_mut();
        debug_assert_eq!(meta.size, self.slab_size);
        debug_assert!(meta.used <= self.capacity());

        let used = meta.used.checked_sub(1).expect("invalid slab dealloc");
        unsafe { slab.as_mut_ptr::<Option<Va>>().write(meta.free.take()) };
        meta.free = Some(slab);
        meta.used = used;
    }

    unsafe fn block_from_slab(&self, slab: Va) -> SharedPageMeta<Slab> {
        let base = Va::new(slab.as_raw() & !(self.block_size.get() - 1));
        let page_meta = page_meta_at(base.into_pa());
        let meta = unsafe { SharedPageMeta::<Slab>::new(page_meta) };

        debug_assert_eq!(meta.addr().into_va(), base);
        debug_assert_eq!(meta.size, self.slab_size);

        meta
    }

    fn capacity(&self) -> usize {
        self.block_size.get() / self.slab_size.get()
    }

    fn request_block(&mut self) -> Option<SharedPageMeta<Slab>> {
        let mut owned = BUDDY
            .lock()
            .alloc(self.block_size)?
            .into_slab(self.slab_size);

        let start = owned.addr().into_va();
        let end = start.offset(self.block_size.get());

        owned.used = 0;
        owned.free = Some(start);

        let mut node = start;
        let mut next = node.offset(self.slab_size.get());
        while next < end {
            unsafe { node.as_mut_ptr::<Option<Va>>().write(Some(next)) };
            node = next;
            next = node.offset(self.slab_size.get());
        }
        unsafe { node.as_mut_ptr::<Option<Va>>().write(None) };

        Some(SharedPageMeta::from_owned(owned))
    }
}
