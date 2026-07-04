use core::cmp::{Ordering, min};
use core::num::NonZeroUsize;

use crate::arch::consts::PAGE_SIZE;
use crate::mm::addr::Pa;
use crate::mm::page_meta::{Buddy, OwnedPageMeta, PageMeta, RefMutSliceOfPageMetaExt};

pub struct BuddyAllocator {
    // nodes for 4KiB, 8KiB, 16KiB, ..., 2MiB
    heads: [Option<OwnedPageMeta<Buddy>>; 10],
}

impl BuddyAllocator {
    pub const fn empty() -> Self {
        Self {
            heads: [const { None }; 10],
        }
    }

    pub fn initialize_section(&mut self, page_meta_items: &mut [PageMeta]) {
        fn max_order(page_meta_items: &[PageMeta], max_order: usize) -> usize {
            let mut index = 0;
            let mut best = 0;
            for order in 1..=max_order {
                while index < (1 << order) {
                    let Some(page) = page_meta_items.get(index) else {
                        return best;
                    };
                    if !page.is_uninit() {
                        return best;
                    }
                    index += 1;
                }
                best = order;
            }

            max_order
        }

        let mut i = 0;
        while i < page_meta_items.len() {
            let page_meta = &page_meta_items[i];
            if !page_meta.is_uninit() {
                i += 1;
                continue;
            }

            let order = max_order(
                &page_meta_items[i..],
                min(
                    page_meta.addr().aligned_order(PAGE_SIZE),
                    self.heads.len() - 1,
                ),
            );
            let owned = (&mut page_meta_items[i..(i + (1 << order))]).owned_buddy(order);
            self.push(owned);

            i += 1 << order;
        }
    }

    pub fn alloc(&mut self, size: NonZeroUsize) -> Option<OwnedPageMeta<Buddy>> {
        let pages = size.get().div_ceil(PAGE_SIZE.get());
        let order = pages.checked_next_power_of_two()?.trailing_zeros() as usize;
        let order = (order < self.heads.len()).then_some(order)?;

        match self.pop(order) {
            Some(page) => Some(page),
            None => {
                self.split(order);
                self.pop(order)
            }
        }
    }

    pub fn free(&mut self, mut page: OwnedPageMeta<Buddy>) {
        loop {
            let order = page.order();
            if order == self.heads.len() - 1 {
                break;
            }

            if let Some(buddy_addr) = page.buddy_addr()
                && let Some(buddy) = self.take(order, buddy_addr)
            {
                page = page.merge(buddy);
            } else {
                break;
            }
        }
        self.push(page);
    }

    pub(crate) fn free_lists(
        &self,
    ) -> impl Iterator<Item = (usize, Option<&OwnedPageMeta<Buddy>>)> {
        self.heads
            .iter()
            .enumerate()
            .map(|(order, head)| (order, head.as_ref()))
    }

    fn pop(&mut self, order: usize) -> Option<OwnedPageMeta<Buddy>> {
        let mut owned = self.heads[order].take()?;
        self.heads[order] = owned.next_mut().take();
        Some(owned)
    }

    fn push(&mut self, mut page: OwnedPageMeta<Buddy>) {
        let addr = page.addr();
        let mut node = &mut self.heads[page.order()];
        loop {
            match node.as_ref().map(|node| node.addr().cmp(&addr)) {
                Some(Ordering::Less) => {
                    node = node.as_mut().unwrap().next_mut();
                }
                _ => {
                    *page.next_mut() = node.take();
                    *node = Some(page);
                    return;
                }
            }
        }
    }

    fn take(&mut self, order: usize, addr: Pa) -> Option<OwnedPageMeta<Buddy>> {
        let mut node = &mut self.heads[order];
        loop {
            match node.as_ref().map(|page| page.addr().cmp(&addr)) {
                Some(Ordering::Less) => {
                    node = node.as_mut().unwrap().next_mut();
                }
                Some(Ordering::Equal) => {
                    let mut page = node.take().unwrap();
                    *node = page.next_mut().take();
                    return Some(page);
                }
                Some(Ordering::Greater) | None => return None,
            }
        }
    }

    fn split(&mut self, order: usize) {
        let mut current_order = order;
        while current_order < self.heads.len() && self.heads[current_order].is_none() {
            current_order += 1;
        }
        if current_order == self.heads.len() {
            return;
        }

        while current_order > order {
            let Some(head) = self.pop(current_order) else {
                return;
            };
            current_order -= 1;

            let (head, buddy) = head.split();
            self.push(buddy);
            self.push(head);
        }
    }
}
