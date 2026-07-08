use core::arch::asm;
use core::fmt;

use crate::arch::consts::PAGE_SIZE;
use crate::mm::PAGE_META_MAP;
use crate::mm::addr::Pa;
use crate::mm::buddy::BuddyAllocator;
use crate::mm::page_meta::{Buddy, OwnedPageMeta, PageMeta, PageMetaSection, PageMetaState};
use crate::printlnk;

pub mod smoke;

pub fn smoke() {
    #[cfg(feature = "smoke-allocator")]
    smoke::allocator::smoke();
    #[cfg(feature = "smoke-page-fault")]
    smoke::page_fault::smoke();
    #[cfg(feature = "smoke-kernel-thread")]
    smoke::kernel_thread::smoke();
}

pub fn dump_page_list() {
    let sections = PAGE_META_MAP.sections();

    if sections.is_empty() {
        printlnk!("page metadata: empty");
        return;
    }

    let pages = sections
        .iter()
        .fold(0, |pages, section| pages + section.page_meta_items().len());
    printlnk!(
        "page metadata: {} sections, {} pages",
        sections.len(),
        pages
    );

    for (index, section) in sections.iter().enumerate() {
        dump_page_section(index, section);
    }
}

fn dump_page_section(index: usize, page_meta: &PageMetaSection) {
    let pages = page_meta.page_meta_items();
    if pages.is_empty() {
        printlnk!(
            "  section {}: region {}..{} (offset {}): empty",
            index,
            page_meta.region().start,
            page_meta.region().end,
            page_meta.offset(),
        );
        return;
    }

    printlnk!(
        "  section {}: region {}..{} (offset {}, {} pages)",
        index,
        page_meta.region().start,
        page_meta.region().end,
        page_meta.offset(),
        pages.len(),
    );

    let mut start = pages[0].addr();
    let mut status = page_status(&pages[0]);
    for page in pages.iter().skip(1) {
        let next_status = page_status(page);
        if next_status != status {
            dump_page_range(start, page.addr(), status);
            start = page.addr();
            status = next_status;
        }
    }
    dump_page_range(
        start,
        pages[pages.len() - 1]
            .addr()
            .checked_offset(PAGE_SIZE.get())
            .unwrap(),
        status,
    );
}

fn dump_page_range(start: Pa, end: Pa, status: PageMetaStatus) {
    printlnk!(
        "  addr {}..{}: {} ({} pages)",
        start,
        end,
        status,
        (end.as_raw() - start.as_raw()) / PAGE_SIZE.get()
    );
}

fn page_status(page: &PageMeta) -> PageMetaStatus {
    match &**page {
        PageMetaState::Uninit => PageMetaStatus::Uninit,
        PageMetaState::Reserved => PageMetaStatus::Reserved,
        PageMetaState::Buddy(buddy) => PageMetaStatus::Buddy {
            order: (buddy.reserved.len() + 1).trailing_zeros() as usize,
        },
        PageMetaState::BuddyReserved => PageMetaStatus::BuddyReserved,
        PageMetaState::Slab(_) => PageMetaStatus::Slab,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PageMetaStatus {
    Uninit,
    Reserved,
    Buddy { order: usize },
    BuddyReserved,
    Slab,
}

impl fmt::Display for PageMetaStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uninit => f.write_str("Uninit"),
            Self::Reserved => f.write_str("Reserved"),
            Self::Buddy { order } => write!(f, "Buddy(order={order})"),
            Self::BuddyReserved => f.write_str("BuddyReserved"),
            Self::Slab => f.write_str("Slab"),
        }
    }
}

impl fmt::Debug for BuddyAllocator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BuddyAllocator")
            .field("heads", &BuddyHeads(self))
            .finish()
    }
}

struct BuddyHeads<'a>(&'a BuddyAllocator);

impl fmt::Debug for BuddyHeads<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut list = f.debug_list();
        for (order, head) in self.0.free_lists() {
            list.entry(&BuddyHead { order, head });
        }
        list.finish()
    }
}

struct BuddyHead<'a> {
    order: usize,
    head: Option<&'a OwnedPageMeta<Buddy>>,
}

impl fmt::Debug for BuddyHead<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.head {
            Some(page) => write!(
                f,
                "order {}: {}, len={}",
                self.order,
                page.addr(),
                buddy_list_len(page)
            ),
            None => write!(f, "order {}: None", self.order),
        }
    }
}

fn buddy_list_len(head: &OwnedPageMeta<Buddy>) -> usize {
    let mut count = 1;
    let mut current = head.next();
    while let Some(page) = current {
        count += 1;
        current = page.next();
    }
    count
}
