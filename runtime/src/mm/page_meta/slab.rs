use core::mem;
use core::num::NonZeroUsize;
use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;

use super::*;
use crate::mm::addr::{Pa, Va};
use crate::util::linked_list::{Node, Pointer};

pub enum Slab {}

pub struct SlabPageMeta {
    pub buddy_meta: BuddyPageMeta,
    pub size: NonZeroUsize,
    pub used: usize,
    pub free: Option<Va>,
    pub node: Node<SharedPageMeta>,
}

impl Deref for OwnedPageMeta<Slab> {
    type Target = SlabPageMeta;

    fn deref(&self) -> &Self::Target {
        let PageMetaState::Slab(slab) = self.as_ref() else {
            unreachable!()
        };
        slab
    }
}

impl DerefMut for OwnedPageMeta<Slab> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let PageMetaState::Slab(slab) = self.as_mut() else {
            unreachable!()
        };
        slab
    }
}

impl OwnedPageMeta<Slab> {
    pub fn into_shared(self) -> SharedPageMeta {
        let handle = SharedPageMeta {
            page_meta: self.page_meta,
        };
        mem::forget(self);
        handle
    }

    pub fn into_buddy(mut self) -> OwnedPageMeta<Buddy> {
        let PageMetaState::Slab(SlabPageMeta {
            buddy_meta,
            used,
            free,
            node,
            ..
        }) = mem::replace(self.as_mut(), PageMetaState::Uninit)
        else {
            unreachable!()
        };

        debug_assert!(used == 0);
        debug_assert!(free.is_some());
        debug_assert!(!node.is_linked());

        debug_assert!(buddy_meta.next.is_none());
        *self.as_mut() = PageMetaState::Buddy(buddy_meta);
        unsafe { self.page_meta.as_mut().owned() }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SharedPageMeta {
    page_meta: NonNull<PageMeta>,
}

unsafe impl Send for SharedPageMeta {}

impl Pointer for SharedPageMeta {
    fn node(&mut self) -> &mut Node<Self> {
        &mut self.deref_mut().node
    }
}

impl Deref for SharedPageMeta {
    type Target = SlabPageMeta;

    fn deref(&self) -> &Self::Target {
        let PageMetaState::Slab(slab) = &**(unsafe { self.page_meta.as_ref() }) else {
            unreachable!()
        };
        slab
    }
}

impl DerefMut for SharedPageMeta {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let PageMetaState::Slab(slab) = &mut **(unsafe { self.page_meta.as_mut() }) else {
            unreachable!()
        };
        slab
    }
}

impl SharedPageMeta {
    /// Create a shared slab metadata handle from a page metadata reference.
    ///
    /// # Safety
    ///
    /// `page_meta` must currently be in the `Slab` state and its lifetime must
    /// cover all uses of the returned handle.
    pub unsafe fn new(page_meta: &PageMeta) -> Self {
        Self {
            page_meta: NonNull::from(page_meta),
        }
    }

    /// Convert this shared handle back into a linear slab ownership token.
    ///
    /// # Safety
    ///
    /// The caller must ensure this is the only live handle being converted and
    /// that the slab is not linked into any allocator list.
    pub unsafe fn into_owned(mut self) -> OwnedPageMeta<Slab> {
        unsafe { self.page_meta.as_mut().owned() }
    }

    pub fn addr(&self) -> Pa {
        unsafe { self.page_meta.as_ref().addr() }
    }

    pub fn is_empty(&self) -> bool {
        let SlabPageMeta { used, .. } = self.deref();
        *used == 0
    }

    pub fn is_full(&self) -> bool {
        let SlabPageMeta { free, .. } = self.deref();
        free.is_none()
    }
}
