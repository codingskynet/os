//! Page metadata for buddy-owned blocks.

use core::num::NonZeroUsize;
use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;
use core::{mem, slice};

use super::*;
use crate::arch::consts::PAGE_SIZE;
use crate::mm::addr::Pa;
use crate::mm::is_same_page_meta_section;
use crate::util::linked_list::Node;

/// Marker type for pages owned by the buddy allocator.
pub enum Buddy {}

/// Metadata stored in the head page of a buddy block.
///
/// `reserved` points at the remaining `2^order - 1` page metadata entries in
/// the block. `next` links the block into a free list when it is not allocated.
pub struct BuddyPageMeta {
    pub reserved: NonNull<[PageMeta]>,
    pub next: Option<OwnedPageMeta<Buddy>>,
}

impl Deref for OwnedPageMeta<Buddy> {
    type Target = BuddyPageMeta;

    fn deref(&self) -> &Self::Target {
        let PageMetaState::Buddy(buddy) = self.as_ref() else {
            unreachable!()
        };
        buddy
    }
}

impl DerefMut for OwnedPageMeta<Buddy> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let PageMetaState::Buddy(buddy) = self.as_mut() else {
            unreachable!()
        };
        buddy
    }
}

impl OwnedPageMeta<Buddy> {
    pub fn order(&self) -> usize {
        let BuddyPageMeta { reserved, .. } = self.deref();
        (reserved.len() + 1).trailing_zeros() as usize
    }

    pub fn next(&self) -> Option<&OwnedPageMeta<Buddy>> {
        let BuddyPageMeta { next, .. } = self.deref();
        next.as_ref()
    }

    pub fn next_mut(&mut self) -> &mut Option<OwnedPageMeta<Buddy>> {
        let BuddyPageMeta { next, .. } = self.deref_mut();
        next
    }

    pub fn buddy_addr(&self) -> Option<Pa> {
        let block_size = PAGE_SIZE.get() << self.order();
        let buddy = Pa::new(self.addr().as_raw() ^ block_size);
        is_same_page_meta_section(self.addr(), buddy).then_some(buddy)
    }

    pub fn split(mut self) -> (Self, Self) {
        debug_assert!(self.order() > 0, "single page buddy cannot be split");

        let BuddyPageMeta { reserved, next } = self.deref_mut();
        debug_assert!(next.is_none());

        let buddy = {
            let len = reserved.len();
            let (reserved, buddy) = unsafe { reserved.as_mut().split_at_mut(len / 2) };

            self.reserved = NonNull::from(reserved);
            buddy
        };

        let (page_meta, reserved) = buddy.split_first_mut().unwrap();
        **page_meta = PageMetaState::Buddy(BuddyPageMeta {
            reserved: NonNull::from(reserved),
            next: None,
        });

        (self, page_meta.owned())
    }

    pub fn merge(self, other: Self) -> Self {
        let order = self.order();
        debug_assert_eq!(order, other.order());
        debug_assert_eq!(self.buddy_addr(), Some(other.addr()));

        let (mut first, mut second) = if self.addr() < other.addr() {
            (self, other)
        } else {
            (other, self)
        };

        debug_assert!(first.next().is_none());
        debug_assert!(second.next().is_none());

        *second.as_mut() = PageMetaState::BuddyReserved;

        // `first` and `second` are order-`order` buddies within the same
        // section (guaranteed by `buddy_addr`), so their `PageMeta` entries are
        // laid out contiguously by frame index. The merged order-`order + 1`
        // block therefore spans `2^(order + 1)` consecutive `PageMeta`s starting
        // at `first`, of which all but the head are reserved: `first`'s original
        // reserved tail, then `second`'s head (just marked above), then
        // `second`'s reserved tail.
        let reserved_len = (1 << (order + 1)) - 1;
        let reserved =
            unsafe { slice::from_raw_parts_mut(first.page_meta.as_ptr().add(1), reserved_len) };
        for page_meta in reserved.iter() {
            debug_assert!(matches!(page_meta.deref(), PageMetaState::BuddyReserved));
        }

        *first.as_mut() = PageMetaState::Buddy(BuddyPageMeta {
            reserved: NonNull::from(reserved),
            next: None,
        });

        first
    }

    pub fn into_slab(mut self, size: NonZeroUsize) -> OwnedPageMeta<Slab> {
        let PageMetaState::Buddy(buddy_meta) = mem::replace(self.as_mut(), PageMetaState::Uninit)
        else {
            unreachable!()
        };
        debug_assert!(buddy_meta.next.is_none());

        *self.as_mut() = PageMetaState::Slab(SlabPageMeta {
            buddy_meta,
            size,
            used: 0,
            free: None,
            node: Node::new(),
        });
        unsafe { self.page_meta.as_mut().owned() }
    }
}
