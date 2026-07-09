use core::marker::PhantomData;
use core::ptr::NonNull;

use super::*;

pub enum Reserved {}

impl OwnedPageMeta<Reserved> {
    pub fn into_buddy(mut self) -> OwnedPageMeta<Buddy> {
        let reserved: &mut [PageMeta] = &mut [];
        *self.as_mut() = PageMetaState::Buddy(BuddyPageMeta {
            reserved: NonNull::from(reserved),
            next: None,
        });

        OwnedPageMeta {
            page_meta: self.page_meta,
            _marker: PhantomData,
        }
    }
}
