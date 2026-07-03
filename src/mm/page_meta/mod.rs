pub use buddy::*;
pub use slab::*;
pub use uninit::*;

mod buddy;
mod slab;
mod uninit;

use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;

use arrayvec::ArrayVec;

use crate::arch::consts::PAGE_SIZE;
use crate::mm::addr::Pa;
use crate::mm::region::Region;

pub struct PageMetaMap {
    sections: UnsafeCell<ArrayVec<PageMetaSection, 4>>,
}

// SAFETY: sections are appended only during single-threaded boot, before the
// map is used by allocator hot paths. After boot the section list is read-only.
unsafe impl Sync for PageMetaMap {}

impl PageMetaMap {
    pub const fn empty() -> Self {
        Self {
            sections: UnsafeCell::new(ArrayVec::new_const()),
        }
    }

    pub unsafe fn add(&self, section: PageMetaSection) {
        unsafe { (*self.sections.get()).push(section) }
    }

    pub fn sections(&self) -> &[PageMetaSection] {
        unsafe { &*self.sections.get() }
    }

    pub fn page_meta(&self, addr: Pa) -> &PageMeta {
        let page_frame = addr.as_raw() / PAGE_SIZE.get();
        for section in self.sections() {
            if !section.region().contains(addr) {
                continue;
            }

            let index = page_frame.checked_sub(section.offset()).unwrap();
            return &section.page_meta_items()[index];
        }

        panic!()
    }
}

pub struct PageMetaSection {
    page_meta_items: &'static [PageMeta],
    offset: usize,
    region: Region,
}

impl PageMetaSection {
    pub fn new(page_meta_items: &'static [PageMeta], offset: usize, region: Region) -> Self {
        Self {
            page_meta_items,
            offset,
            region,
        }
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn region(&self) -> Region {
        self.region
    }

    pub fn page_meta_items(&self) -> &[PageMeta] {
        self.page_meta_items
    }
}

pub struct PageMeta {
    addr: Pa,
    state: UnsafeCell<PageMetaState>,
}

impl Deref for PageMeta {
    type Target = PageMetaState;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.state.get() }
    }
}

impl DerefMut for PageMeta {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.state.get_mut()
    }
}

// SAFETY: page metadata is allocated during boot and never moved afterward.
// Mutable state transitions are performed only by boot initialization or by
// linear `OwnedPageMeta` handles owned by allocator state.
unsafe impl Send for PageMeta {}

// SAFETY: shared references to `PageMeta` expose only read-only inspection
// methods. Internal mutation goes through `UnsafeCell` and is serialized by the
// owning allocator's lock plus the linear handle discipline.
unsafe impl Sync for PageMeta {}

impl PageMeta {
    pub const fn uninit(addr: Pa) -> Self {
        Self {
            addr,
            state: UnsafeCell::new(PageMetaState::Uninit),
        }
    }

    pub fn addr(&self) -> Pa {
        self.addr
    }

    pub fn region(&self) -> Region {
        match **self {
            PageMetaState::Uninit => Region::from_size(self.addr, PAGE_SIZE).unwrap(),
            _ => todo!("Does it need?: {}", unsafe { &*self.state.get() }),
        }
    }

    pub fn is_uninit(&self) -> bool {
        matches!(**self, PageMetaState::Uninit)
    }

    pub fn owned_uninit(&mut self) -> OwnedPageMeta<Uninit> {
        if self.is_uninit() {
            self.owned::<Uninit>()
        } else {
            panic!("Only uninitialized PageMeta can be owned")
        }
    }

    pub unsafe fn owned_buddy(&self) -> OwnedPageMeta<Buddy> {
        if !matches!(**self, PageMetaState::Buddy(_)) {
            panic!("it is not buddy")
        }

        OwnedPageMeta {
            page_meta: NonNull::from(self),
            _marker: PhantomData,
        }
    }

    fn owned<S>(&mut self) -> OwnedPageMeta<S> {
        OwnedPageMeta {
            page_meta: NonNull::from(self),
            _marker: PhantomData,
        }
    }
}

#[extend::ext]
pub impl &mut [PageMeta] {
    fn owned_buddy(self, order: usize) -> OwnedPageMeta<Buddy> {
        assert_eq!(self.len(), 1 << order);
        for page_meta in &*self {
            if !page_meta.is_uninit() {
                panic!("Only uninitialized PageMeta can be owned")
            }
        }
        let (page_meta, reserved) = self.split_first_mut().unwrap();
        assert!(page_meta.addr().aligned_order(PAGE_SIZE) >= order);

        for page_meta in reserved.iter_mut() {
            **page_meta = PageMetaState::BuddyReserved;
        }
        **page_meta = PageMetaState::Buddy(BuddyPageMeta {
            reserved: NonNull::from(reserved),
            next: None,
        });

        page_meta.owned::<Buddy>()
    }
}

#[derive(derive_more::Display)]

pub enum PageMetaState {
    #[display("uninit")]
    Uninit,
    #[display("reserved")]
    Reserved,
    #[display("buddy")]
    Buddy(BuddyPageMeta),
    #[display("buddyreserved")]
    BuddyReserved,
    #[display("slab")]
    Slab(SlabPageMeta),
}

pub struct OwnedPageMeta<S> {
    page_meta: NonNull<PageMeta>,
    _marker: PhantomData<S>,
}

// SAFETY: the handle is a linear ownership token for boot-allocated page
// metadata. Moving the token to another hart transfers that ownership.
unsafe impl<S> Send for OwnedPageMeta<S> {}

// TODO?
unsafe impl<S> Sync for OwnedPageMeta<S> {}

impl<S> OwnedPageMeta<S> {
    fn new(page_meta: &PageMeta) -> Self {
        Self {
            page_meta: NonNull::from(page_meta),
            _marker: PhantomData,
        }
    }

    pub fn addr(&self) -> Pa {
        unsafe { self.page_meta.as_ref().addr() }
    }

    fn as_ref(&self) -> &PageMetaState {
        unsafe { self.page_meta.as_ref() }
    }

    fn as_mut(&mut self) -> &mut PageMetaState {
        unsafe { self.page_meta.as_mut() }
    }
}
