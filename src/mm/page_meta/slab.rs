use core::mem;
use core::num::NonZeroUsize;
use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;

use crate::mm::addr::{Pa, Va};
use crate::mm::page_meta::{BuddyPageMeta, OwnedPageMeta, PageMeta, PageMetaState};

pub enum Slab {}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SlabPageHandle {
    page_meta: NonNull<PageMeta>,
}

pub struct SlabPageMeta {
    pub buddy_meta: BuddyPageMeta,
    pub size: NonZeroUsize,
    pub used: usize,
    pub free: Option<Va>,
    pub listed: bool,
    pub prev: Option<SlabPageHandle>,
    pub next: Option<SlabPageHandle>,
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
    pub fn is_empty(&self) -> bool {
        let SlabPageMeta { used, .. } = self.deref();
        *used == 0
    }

    pub fn is_full(&self) -> bool {
        let SlabPageMeta { free, .. } = self.deref();
        free.is_none()
    }

    pub fn next_mut(&mut self) -> &mut Option<SlabPageHandle> {
        let SlabPageMeta { next, .. } = self.deref_mut();
        next
    }

    pub fn into_handle(self) -> SlabPageHandle {
        let handle = SlabPageHandle {
            page_meta: self.page_meta,
        };
        mem::forget(self);
        handle
    }
}

impl SlabPageHandle {
    pub unsafe fn new_unchecked(page_meta: &PageMeta) -> Self {
        Self {
            page_meta: NonNull::from(page_meta),
        }
    }

    pub fn addr(&self) -> Pa {
        unsafe { self.page_meta.as_ref().addr() }
    }

    pub fn slab(&self) -> &SlabPageMeta {
        let PageMetaState::Slab(slab) = &**(unsafe { self.page_meta.as_ref() }) else {
            unreachable!()
        };
        slab
    }

    pub unsafe fn slab_mut(&mut self) -> &mut SlabPageMeta {
        let PageMetaState::Slab(slab) = &mut **(unsafe { self.page_meta.as_mut() }) else {
            unreachable!()
        };
        slab
    }

    pub fn is_empty(&self) -> bool {
        self.slab().used == 0
    }

    pub fn is_full(&self) -> bool {
        self.slab().free.is_none()
    }
}
