use core::ptr::NonNull;

use crate::mm::addr::Pa;

pub struct PageMeta {
    pages: &'static mut [Page],
    offset: usize,
}

impl PageMeta {
    pub const fn empty() -> Self {
        Self {
            pages: &mut [],
            offset: 0,
        }
    }

    pub fn new(pages: &'static mut [Page], offset: usize) -> Self {
        Self { pages, offset }
    }

    pub fn pages(&self) -> &[Page] {
        self.pages
    }

    pub fn pages_mut(&mut self) -> &mut [Page] {
        self.pages
    }

    pub fn offset(&self) -> usize {
        self.offset
    }
}

pub struct Page {
    pub addr: Pa,
    pub status: Status,
    pub order: usize,
    pub next: Option<NonNull<Page>>,
}

impl Page {
    pub const fn free(addr: Pa) -> Self {
        Self {
            addr,
            status: Status::Free,
            order: 0,
            next: None,
        }
    }

    pub fn reserve(&mut self) {
        self.status = Status::Reserved;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, derive_more::Display)]
pub enum Status {
    Free,
    Reserved,
}
