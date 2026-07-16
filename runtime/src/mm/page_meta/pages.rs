use core::mem;
use core::sync::atomic::AtomicUsize;

use super::*;

pub enum Pages {}

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
