use core::mem;
use core::num::NonZeroUsize;
use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;

use crate::mm::addr::{Pa, Va};
use crate::mm::page_meta::{BuddyPageMeta, OwnedPageMeta, PageMeta, PageMetaState};
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
    pub fn is_empty(&self) -> bool {
        let SlabPageMeta { used, .. } = self.deref();
        *used == 0
    }

    pub fn is_full(&self) -> bool {
        let SlabPageMeta { free, .. } = self.deref();
        free.is_none()
    }

    pub fn into_shared(self) -> SharedPageMeta {
        let handle = SharedPageMeta {
            page_meta: self.page_meta,
        };
        mem::forget(self);
        handle
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SharedPageMeta {
    page_meta: NonNull<PageMeta>,
}

unsafe impl Send for SharedPageMeta {}

impl Pointer for SharedPageMeta {
    unsafe fn node(&self) -> *mut Node<Self> {
        &self.deref().node as *const _ as *mut _
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
    pub unsafe fn new(page_meta: &PageMeta) -> Self {
        Self {
            page_meta: NonNull::from(page_meta),
        }
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
