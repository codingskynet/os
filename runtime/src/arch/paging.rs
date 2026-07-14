use alloc::alloc::Allocator;
use alloc::boxed::Box;
use core::mem::ManuallyDrop;
use core::ops::{Deref, DerefMut};
use core::ptr;

use crate::arch::page_table::{PageTable, PageTableEntry, PteFlags};
use crate::mm::addr::Va;

pub struct PageTableRoot<A: Allocator>(Box<PageTable, A>);

impl<A: Allocator + Clone> PageTableRoot<A> {
    pub fn new(alloc: A) -> Self {
        unsafe {
            let mut page_table = Box::new_uninit_in(alloc);
            PageTable::init_mut(&mut page_table);
            Self(page_table.assume_init())
        }
    }

    pub fn leak<'a>(root: Self) -> &'a mut PageTable
    where
        A: 'a,
    {
        let root = ManuallyDrop::new(root);
        // SAFETY: `root` will not run `PageTableRoot::drop`, so this moves the
        // inner Box exactly once. `Box::leak` then intentionally retains both
        // the page-table allocation and its allocator.
        let root = unsafe { ptr::read(ptr::addr_of!(root.0)) };
        Box::leak(root)
    }

    /// Reconstruct a page-table root from a raw allocation.
    ///
    /// # Safety
    ///
    /// `raw` must have been allocated by `alloc` (or an equivalent clone) for
    /// a valid `PageTable`, must be uniquely owned, and must not already be
    /// managed by another `Box` or `PageTableRoot`. Its lower-half non-leaf
    /// entries must likewise own page tables allocated by the same allocator.
    pub unsafe fn from_raw_in(raw: *mut PageTable, alloc: A) -> Self {
        unsafe { Self(Box::from_raw_in(raw, alloc)) }
    }

    pub fn cursor(&mut self) -> PageTableCursor<'_, A> {
        let alloc = Box::allocator(&self.0).clone();
        PageTableCursor {
            table: self.0.as_mut(),
            alloc,
        }
    }

    pub fn root(&self) -> &PageTable {
        &self.0
    }
}

// TODO: remove it and capsule by not allowing directly access of PageTable
impl<A: Allocator> From<Box<PageTable, A>> for PageTableRoot<A> {
    fn from(root: Box<PageTable, A>) -> Self {
        Self(root)
    }
}

// TODO: deallocate child page table after thread exit and double free problem if shared page table
// impl<A: Allocator> Drop for PageTableRoot<A> {
//     fn drop(&mut self) {
//         let alloc = Box::allocator(&self.0) as *const A;
//         let user_end = vpn2(Va::new(UPPER_CANONICAL_BASE));

//         // SAFETY: lower-half page-table branches are allocated exclusively for
//         // this root using equivalent clones of `alloc`. Upper-half entries are
//         // shared kernel mappings and are deliberately excluded. The allocator
//         // stored in the root Box remains alive until after this method returns.
//         unsafe {
//             deallocate_tables(&mut self.0[0..user_end], &*alloc);
//         }
//     }
// }

// unsafe fn deallocate_tables<A: Allocator>(entries: &mut [PageTableEntry], alloc: &A) {
//     for entry in entries {
//         {
//             let Some(page_table) = entry.page_table_mut() else {
//                 continue;
//             };

//             unsafe {
//                 deallocate_tables(&mut page_table[..], alloc);
//             }

//             // SAFETY: this non-leaf PTE uniquely owns `page_table`, all descendants
//             // have already been released, and the PTE no longer references it.
//             unsafe {
//                 drop(Box::from_raw_in(page_table, alloc));
//             }
//         }
//         entry.clear();
//     }
// }

pub struct PageTableCursor<'a, A: Allocator> {
    table: &'a mut PageTable,
    alloc: A,
}

impl<'a, A: Allocator> Deref for PageTableCursor<'a, A> {
    type Target = PageTable;

    fn deref(&self) -> &Self::Target {
        self.table
    }
}

impl<'a, A: Allocator + Clone + 'a> PageTableCursor<'a, A> {
    pub fn entry(&'a mut self, index: usize) -> PageTableEntryCursor<'a, A> {
        PageTableEntryCursor {
            entry: self.table.entry(index),
            alloc: self.alloc.clone(),
        }
    }
}

pub struct PageTableEntryCursor<'a, A: Allocator> {
    entry: &'a mut PageTableEntry,
    alloc: A,
}

impl<'a, A: Allocator> Deref for PageTableEntryCursor<'a, A> {
    type Target = PageTableEntry;

    fn deref(&self) -> &Self::Target {
        self.entry
    }
}

impl<'a, A: Allocator> DerefMut for PageTableEntryCursor<'a, A> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.entry
    }
}

impl<'a, A: Allocator + Clone + 'a> PageTableEntryCursor<'a, A> {
    // TODO: change directly or_insert through checking if it is page table
    pub fn or_insert(self) -> PageTableCursor<'a, A> {
        let table = if self.entry.flags().contains(PteFlags::V) {
            unsafe { &mut *(self.entry.address().into_va().as_mut_ptr()) }
        } else {
            let (table, alloc) =
                Box::into_raw_with_allocator(Box::new_uninit_in(self.alloc.clone()));
            drop(alloc);
            // SAFETY: `table` came from a uniquely owned Box allocation. The
            // allocation remains live, and `self.alloc` retains an equivalent
            // allocator instance for its eventual reclamation.
            let table = PageTable::init_mut(unsafe { &mut *table });
            self.entry
                .mut_address(Va::from(&mut *table).into_pa())
                .mut_flags(PteFlags::V);
            table
        };

        PageTableCursor {
            table,
            alloc: self.alloc,
        }
    }

    // TODO: drop table if it is unique
    pub fn clear(&mut self) {
        self.entry.clear();
    }
}
