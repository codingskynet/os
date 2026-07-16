//! Reference-counted ownership of contiguous physical pages.
//!
//! Blocks come from the buddy allocator and return to it when their final
//! strong reference is dropped. The allocation size is rounded up to a buddy
//! order, so [`Pages::size`] reports the actual block size rather than the
//! original request.

use core::mem;
use core::num::NonZeroUsize;
use core::ops::Deref;
use core::sync::atomic::{Ordering, fence};

use crate::arch::consts::PAGE_SIZE;
use crate::mm::addr::Pa;
use crate::mm::page_meta::SharedPageMeta;
use crate::mm::{BUDDY, page_meta, page_meta_at};

/// Strong handle to one contiguous, power-of-two-sized physical block.
pub struct Pages {
    meta: page_meta::SharedPageMeta<page_meta::Pages>,
}

impl Clone for Pages {
    fn clone(&self) -> Self {
        self.meta.deref().strong.fetch_add(1, Ordering::Relaxed);
        Self { meta: self.meta }
    }
}

impl Pages {
    /// Allocate a block large enough for `size` bytes.
    ///
    /// The returned block is page-aligned and rounded up to a supported buddy
    /// order. Returns `None` if the requested order is unsupported or no block
    /// is available.
    pub fn new(size: NonZeroUsize) -> Option<Self> {
        let owned = BUDDY.lock().alloc(size)?.into_pages();
        Some(Self {
            meta: SharedPageMeta::from_owned(owned),
        })
    }

    pub fn addr(&self) -> Pa {
        self.meta.addr()
    }

    pub fn as_ptr<T>(&self) -> *const T {
        self.addr().into_va().as_ptr()
    }

    pub fn as_mut_ptr<T>(&self) -> *mut T {
        self.addr().into_va().as_mut_ptr()
    }

    pub fn is_unique(&self) -> bool {
        self.meta.strong.load(Ordering::Acquire) == 1
    }

    pub fn size(&self) -> NonZeroUsize {
        NonZeroUsize::new((self.meta.reserved.len() + 1) * PAGE_SIZE.get()).unwrap()
    }

    /// Reconstruct a handle for a strong reference stored in raw form.
    ///
    /// Raw references are created by deliberately forgetting a `Pages` value,
    /// for example when transferring its ownership into a non-leaf page-table
    /// entry. This function consumes that raw reference; it does not increment
    /// the strong count.
    ///
    /// # Safety
    ///
    /// `addr` must identify the head of a block in the `Pages` state, and the
    /// caller must own exactly one raw strong reference for that block.
    pub unsafe fn from_raw(addr: Pa) -> Self {
        Self {
            meta: unsafe { SharedPageMeta::new(page_meta_at(addr)) },
        }
    }
}

impl Drop for Pages {
    fn drop(&mut self) {
        let strong = self.meta.strong.fetch_sub(1, Ordering::Release);
        debug_assert!(strong > 0, "Pages reference count underflow");
        if strong == 1 {
            fence(Ordering::Acquire);
            let owned = unsafe { SharedPageMeta::into_owned(self.meta) };
            BUDDY.lock().free(owned.into_buddy());
        }
    }
}
