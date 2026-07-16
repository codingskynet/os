//! Page metadata for slab-owned blocks.

use core::mem;
use core::num::NonZeroUsize;
use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;

use super::*;
use crate::mm::addr::{Pa, Va};
use crate::util::linked_list::{Node, Pointer};

/// Marker type for pages owned by the slab allocator.
pub enum Slab {}

/// Metadata stored in the head page of a slab block.
///
/// `buddy_meta` remembers the original buddy block so the block can be returned
/// to the buddy allocator when it becomes empty.
pub struct SlabPageMeta {
    pub reserved: NonNull<[PageMeta]>,
    pub size: NonZeroUsize,
    pub used: usize,
    pub free: Option<Va>,
    pub node: Node<SharedPageMeta<Slab>>,
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
    pub fn into_buddy(mut self) -> OwnedPageMeta<Buddy> {
        let PageMetaState::Slab(slab) = mem::replace(self.as_mut(), PageMetaState::Uninit) else {
            unreachable!()
        };

        debug_assert!(slab.used == 0);
        debug_assert!(slab.free.is_some());
        debug_assert!(!slab.node.is_linked());

        *self.as_mut() = PageMetaState::Buddy(BuddyPageMeta {
            reserved: slab.reserved,
            next: None,
        });
        unsafe { self.page_meta.as_mut().owned() }
    }
}

impl Pointer for SharedPageMeta<Slab> {
    fn node(&mut self) -> &mut Node<Self> {
        &mut self.deref_mut().node
    }
}

impl Deref for SharedPageMeta<Slab> {
    type Target = SlabPageMeta;

    fn deref(&self) -> &Self::Target {
        let PageMetaState::Slab(slab) = &**(unsafe { self.page_meta.as_ref() }) else {
            unreachable!()
        };
        slab
    }
}

impl DerefMut for SharedPageMeta<Slab> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let PageMetaState::Slab(slab) = &mut **(unsafe { self.page_meta.as_mut() }) else {
            unreachable!()
        };
        slab
    }
}

impl SharedPageMeta<Slab> {
    pub fn is_empty(&self) -> bool {
        let SlabPageMeta { used, .. } = self.deref();
        *used == 0
    }

    pub fn is_full(&self) -> bool {
        let SlabPageMeta { free, .. } = self.deref();
        free.is_none()
    }
}
