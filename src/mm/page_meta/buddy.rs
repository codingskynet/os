use core::mem;
use core::num::NonZeroUsize;
use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;

use crate::mm::page_meta::{OwnedPageMeta, PageMeta, PageMetaState, Slab, SlabPageMeta};
use crate::util::linked_list::Node;

pub enum Buddy {}
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

    pub fn split(mut self) -> (Self, Self) {
        assert!(self.order() > 0, "single page buddy cannot be split");

        let BuddyPageMeta { reserved, next } = self.deref_mut();
        assert!(next.is_none());

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

    pub fn into_slab(mut self, size: NonZeroUsize) -> OwnedPageMeta<Slab> {
        let PageMetaState::Buddy(buddy_meta) = mem::replace(self.as_mut(), PageMetaState::Uninit)
        else {
            unreachable!()
        };

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
