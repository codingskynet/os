//! Per-page metadata and ownership-state transitions.
//!
//! Every physical page has one [`PageMeta`] entry. Allocators move pages
//! between states by consuming linear [`OwnedPageMeta`] tokens, which prevents
//! the same page from being owned by two allocator data structures at once.
//!
//! Head-page state graph:
//!
//! ```text
//! boot discovers RAM
//!        |
//!        v
//! +--------+
//! | Uninit |
//! +--------+
//!   |  |
//!   |  +-- owned_uninit().consume_as_reserved() ---> +----------+
//!   |                                                | Reserved |
//!   |                                                +----------+
//!   |                                                     |
//!   |                                                     | owned_reserved().into_buddy()
//!   |                                                     v
//!   | &mut [PageMeta]::owned_buddy(order)            +-------+
//!   +----------------------------------------------> | Buddy |
//!                                                    +-------+
//!                                                       | ^
//!                                                       | |
//!                                      into_slab(size)  | |  into_buddy()
//!                                                       v |
//!                                                    +------+
//!                                                    | Slab |
//!                                                    +------+
//!
//! Multi-page block transitions:
//!
//! split(order n):
//!   [ Buddy head | BuddyReserved tail... ]
//!        |
//!        +--> [ Buddy head | tail... ] + [ Buddy head | tail... ]
//!                                      ^
//!                                      |
//!                         one tail page is promoted to a Buddy head
//!
//! merge(order n):
//!   [ Buddy head | tail... ] + [ Buddy head | tail... ]
//!        |
//!        +--> [ Buddy head | BuddyReserved tail... ]
//!                                      ^
//!                                      |
//!                         the second Buddy head is demoted to a tail page
//! ```
//!
//! `Buddy` and `Slab` are head-page states for a physical block. Every other
//! page covered by that block is marked [`PageMetaState::BuddyReserved`], even
//! while the head is in the `Slab` state. `BuddyReserved` is therefore not an
//! independently ownable state; it changes only as part of splitting or merging
//! a buddy block.
//!
//! Transition summary:
//!
//! - [`PageMeta::uninit`] creates metadata in [`PageMetaState::Uninit`].
//! - [`OwnedPageMeta<Uninit>::consume_as_reserved`] marks boot-reserved pages.
//! - `RefMutSliceOfPageMetaExt::owned_buddy` consumes a contiguous uninit slice,
//!   stores [`PageMetaState::Buddy`] in the first page, and marks the remaining
//!   pages [`PageMetaState::BuddyReserved`].
//! - [`OwnedPageMeta<Reserved>::into_buddy`] releases a single reserved page
//!   back to the buddy allocator.
//! - [`OwnedPageMeta<Buddy>::split`] creates two smaller buddy heads; the new
//!   right-hand head was previously a `BuddyReserved` tail page.
//! - [`OwnedPageMeta<Buddy>::merge`] combines two buddy heads; the second head
//!   becomes `BuddyReserved` and the first head covers the merged block.
//! - [`OwnedPageMeta<Buddy>::into_slab`] turns a buddy head into a slab head
//!   while preserving the buddy metadata needed to return the block later.
//! - [`OwnedPageMeta<Slab>::into_buddy`] is valid only for an empty, unlinked
//!   slab block and restores the saved buddy metadata.

pub use buddy::*;
pub use slab::*;
pub use uninit::*;

mod buddy;
mod reserved;
mod slab;
mod uninit;

use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;

use arrayvec::ArrayVec;

use crate::arch::consts::PAGE_SIZE;
use crate::mm::addr::Pa;
use crate::mm::page_meta::reserved::Reserved;
use crate::mm::region::Region;

/// Collection of page-metadata sections for all discovered RAM ranges.
pub struct PageMetaMap {
    sections: ArrayVec<PageMetaSection, 4>,
}

impl PageMetaMap {
    pub const fn empty() -> Self {
        Self {
            sections: ArrayVec::new_const(),
        }
    }

    pub fn add(&mut self, section: PageMetaSection) {
        self.sections.push(section);
    }

    pub fn sections(&self) -> &[PageMetaSection] {
        &self.sections
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

    pub fn is_same_section(&self, lhs: Pa, rhs: Pa) -> bool {
        self.sections()
            .iter()
            .any(|section| section.region().contains(lhs) && section.region().contains(rhs))
    }
}

/// Contiguous page-metadata array backing one physical memory region.
///
/// `offset` is the physical page-frame number represented by
/// `page_meta_items[0]`.
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

/// Metadata for one physical page frame.
///
/// The address is immutable after boot. The state is internally mutable so
/// allocator-owned tokens can update it while global references to the metadata
/// remain stable.
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
            _ => todo!("Does it need?"),
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

    /// Recreate a linear buddy ownership token from page metadata.
    ///
    /// # Safety
    ///
    /// The caller must ensure no other `OwnedPageMeta<Buddy>` exists for this
    /// page and that allocator locking excludes concurrent state transitions.
    pub unsafe fn owned_buddy(&self) -> OwnedPageMeta<Buddy> {
        if !matches!(**self, PageMetaState::Buddy(_)) {
            panic!("it is not buddy")
        }

        OwnedPageMeta {
            page_meta: NonNull::from(self),
            _marker: PhantomData,
        }
    }

    /// Recreate a linear reserved-page ownership token from page metadata.
    ///
    /// # Safety
    ///
    /// The caller must ensure no other ownership token exists for this page
    /// and that allocator locking excludes concurrent state transitions.
    pub unsafe fn owned_reserved(&self) -> OwnedPageMeta<Reserved> {
        if !matches!(**self, PageMetaState::Reserved) {
            panic!("it is not reserved")
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
        debug_assert!(page_meta.addr().aligned_order(PAGE_SIZE) >= order);

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

/// Current ownership state of a physical page.
///
/// Multi-page buddy and slab blocks use a head page with rich metadata and mark
/// the remaining pages as `BuddyReserved`.
pub enum PageMetaState {
    /// Boot-created metadata that has not yet been handed to an allocator or
    /// marked reserved.
    Uninit,

    /// A page reserved during boot, for example because it overlaps the kernel
    /// image, the DTB, or non-RAM padding around a memory section.
    Reserved,

    /// Head page of a buddy block.
    ///
    /// The embedded [`BuddyPageMeta`] records the block's reserved tail pages
    /// and optional free-list link.
    Buddy(BuddyPageMeta),

    /// Non-head page covered by a multi-page buddy or slab block.
    ///
    /// This state prevents tail pages from being independently owned while the
    /// head page represents the whole block.
    BuddyReserved,

    /// Head page of a slab block borrowed from the buddy allocator.
    ///
    /// The embedded [`SlabPageMeta`] includes the original buddy metadata so an
    /// empty slab block can become [`PageMetaState::Buddy`] again.
    Slab(SlabPageMeta),
}

/// Linear ownership token for a page in state `S`.
///
/// Holding this token means the caller is responsible for eventually consuming
/// it into another state or returning it to the allocator that owns that state.
pub struct OwnedPageMeta<S> {
    page_meta: NonNull<PageMeta>,
    _marker: PhantomData<S>,
}

// SAFETY: the handle is a linear ownership token for boot-allocated page
// metadata. Moving the token to another hart transfers that ownership.
unsafe impl<S> Send for OwnedPageMeta<S> {}

impl<S> OwnedPageMeta<S> {
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
