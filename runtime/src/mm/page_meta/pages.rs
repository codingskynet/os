//! Page metadata for reference-counted physical blocks.
//!
//! The public [`crate::mm::Pages`] handle owns strong references. Page-table
//! entries can also hold a strong reference in raw form; reconstructing that
//! reference is the responsibility of the page-table ownership code.

use core::mem;
use core::sync::atomic::AtomicUsize;

use super::*;

/// Marker type for blocks managed through reference-counted `Pages` handles.
pub enum Pages {}

/// Metadata stored in the head page of a reference-counted physical block.
///
/// `reserved` covers the remaining pages inherited from the buddy block.
/// `strong` counts both live [`crate::mm::Pages`] values and owning raw
/// references embedded in page-table entries.
pub struct PagesMeta {
    pub reserved: NonNull<[PageMeta]>,
    pub strong: AtomicUsize,
}

impl Deref for OwnedPageMeta<Pages> {
    type Target = PagesMeta;

    fn deref(&self) -> &Self::Target {
        let PageMetaState::Pages(pages) = self.as_ref() else {
            unreachable!()
        };
        pages
    }
}

impl DerefMut for OwnedPageMeta<Pages> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let PageMetaState::Pages(pages) = self.as_mut() else {
            unreachable!()
        };
        pages
    }
}

impl OwnedPageMeta<Pages> {
    /// Return a block with no remaining strong references to buddy ownership.
    pub fn into_buddy(mut self) -> OwnedPageMeta<Buddy> {
        let PageMetaState::Pages(pages) = mem::replace(self.as_mut(), PageMetaState::Uninit) else {
            unreachable!()
        };

        *self.as_mut() = PageMetaState::Buddy(BuddyPageMeta {
            reserved: pages.reserved,
            next: None,
        });
        unsafe { self.page_meta.as_mut().owned() }
    }
}

impl Deref for SharedPageMeta<Pages> {
    type Target = PagesMeta;

    fn deref(&self) -> &Self::Target {
        let PageMetaState::Pages(pages) = self.as_ref() else {
            unreachable!()
        };
        pages
    }
}
