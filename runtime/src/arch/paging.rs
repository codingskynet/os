use core::mem::{self, MaybeUninit};
use core::num::NonZeroUsize;
use core::ops::{Deref, DerefMut};

use bitflags::bitflags;

use crate::arch::consts::PAGE_SIZE;
use crate::arch::page_table::{
    PageTable as RawPageTable, PageTableEntry as RawPageTableEntry, PteFlags, vpn0, vpn1, vpn2,
};
use crate::mm::Pages;
use crate::mm::addr::{Pa, Va, VarVa};

pub struct PageTable(Pages);

impl Default for PageTable {
    fn default() -> Self {
        Self::new()
    }
}

impl PageTable {
    pub fn new() -> Self {
        let pages = Pages::new(PAGE_SIZE).unwrap();
        unsafe {
            RawPageTable::init_mut(&mut *pages.as_mut_ptr::<MaybeUninit<RawPageTable>>());
        }
        Self(pages)
    }

    pub fn new_from_active() -> Self {
        let pages = Pages::new(PAGE_SIZE).unwrap();
        unsafe {
            let table = &mut *pages.as_mut_ptr::<MaybeUninit<RawPageTable>>();
            RawPageTable::init_from_root(table);
        }
        Self(pages)
    }

    pub fn leak<'a>(root: Self) -> &'a mut RawPageTable {
        let table = root.0.as_mut_ptr();
        mem::forget(root);
        unsafe { &mut *table }
    }

    pub fn map(
        &mut self,
        va: impl Into<VarVa>,
        pa: Pa,
        size: NonZeroUsize,
        permissions: Permission,
    ) -> bool {
        if size != PAGE_SIZE {
            return false;
        }

        let mut flags: PteFlags = permissions.into();
        let va = match va.into() {
            VarVa::User(uva) => {
                flags |= PteFlags::U;
                uva.into_va()
            }
            VarVa::Kernel(va) => va,
        };

        let mut entry = self
            .cursor()
            .into_entry(vpn2(va))
            .or_insert()
            .into_entry(vpn1(va))
            .or_insert()
            .into_entry(vpn0(va));
        if entry.is_valid() {
            return false;
        }

        entry.mut_address(pa).mut_flags(flags | PteFlags::V);
        true
    }

    pub fn address(&self) -> Pa {
        self.0.addr()
    }

    pub fn as_ptr(&self) -> &RawPageTable {
        unsafe { &*self.0.as_ptr() }
    }

    fn cursor(&mut self) -> PageTableCursor<'_> {
        PageTableCursor {
            table: unsafe { &mut *self.0.as_mut_ptr() },
        }
    }
}

impl Drop for PageTable {
    fn drop(&mut self) {
        let table = unsafe { &mut *self.0.as_mut_ptr::<RawPageTable>() };
        for entry in table.iter_mut() {
            drop_nonleaf(entry);
        }
    }
}

fn drop_nonleaf(entry: &mut RawPageTableEntry) {
    if !entry.is_valid() || entry.is_leaf() {
        return;
    }

    let addr = entry.address();
    entry.clear();

    // SAFETY: a valid non-leaf PTE owns one raw `Pages` strong reference.
    let pages = unsafe { Pages::from_raw(addr) };
    if pages.is_unique() {
        let table = unsafe { &mut *pages.as_mut_ptr::<RawPageTable>() };
        for child in table.iter_mut() {
            drop_nonleaf(child);
        }
    }
    drop(pages);
}

pub struct PageTableCursor<'a> {
    table: &'a mut RawPageTable,
}

impl Deref for PageTableCursor<'_> {
    type Target = RawPageTable;

    fn deref(&self) -> &Self::Target {
        self.table
    }
}

impl<'a> PageTableCursor<'a> {
    pub fn into_entry(self, index: usize) -> PageTableEntryCursor<'a> {
        PageTableEntryCursor {
            entry: self.table.entry(index),
        }
    }
}

pub struct PageTableEntryCursor<'a> {
    entry: &'a mut RawPageTableEntry,
}

impl Deref for PageTableEntryCursor<'_> {
    type Target = RawPageTableEntry;

    fn deref(&self) -> &Self::Target {
        self.entry
    }
}

impl DerefMut for PageTableEntryCursor<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.entry
    }
}

impl<'a> PageTableEntryCursor<'a> {
    // TODO: change directly or_insert through checking if it is page table
    pub fn or_insert(self) -> PageTableCursor<'a> {
        let table = if self.entry.flags().contains(PteFlags::V) {
            self.entry
                .page_table_mut()
                .expect("valid leaf PTE cannot be used as a page table")
        } else {
            let table = PageTable::new();
            let address = table.address();
            self.entry.mut_address(address).mut_flags(PteFlags::V);
            mem::forget(table);
            unsafe { &mut *address.into_va().as_mut_ptr() }
        };

        PageTableCursor { table }
    }

    pub fn clear(&mut self) {
        if self.entry.is_leaf() || !self.entry.is_valid() {
            self.entry.clear();
        } else {
            drop_nonleaf(self.entry);
        }
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Permission: u8 {
        const R = 1 << 0;
        const W = 1 << 1;
        const X = 1 << 2;
    }
}
