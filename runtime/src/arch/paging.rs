use core::mem::{self, MaybeUninit};
use core::num::NonZeroUsize;
use core::ops::{Deref, DerefMut};

use bitflags::bitflags;

use crate::arch::consts::PAGE_SIZE;
use crate::arch::page_table::{
    PageTable as RawPageTable, PageTableEntry as RawPageTableEntry, PteFlags, vpn0, vpn1, vpn2,
};
use crate::mm::Pages;
use crate::mm::addr::{Pa, VarVa};

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
        // A valid PTE without any R/W/X bit is a non-leaf page-table entry in
        // Sv39. Never let an ordinary mapping be mistaken for an owning edge
        // to another page-table page.
        if size != PAGE_SIZE || permissions.is_empty() {
            return false;
        }

        map_in_table(
            unsafe { &mut *self.0.as_mut_ptr::<RawPageTable>() },
            va.into(),
            pa,
            permissions,
        )
    }

    pub fn address(&self) -> Pa {
        self.0.addr()
    }

    pub fn as_ptr(&self) -> &RawPageTable {
        unsafe { &*self.0.as_ptr() }
    }
}

/// Map one supervisor page in the active address space.
///
/// # Safety
///
/// Kernel page-table subtrees are shared by all memory contexts. The caller
/// must serialize this operation with every other mutation of the same subtree.
pub unsafe fn map_kernel_page_to_active(
    va: crate::mm::addr::Va,
    pa: Pa,
    permissions: Permission,
) -> bool {
    let table = unsafe { &mut *crate::arch::asm::page_table::active() };
    map_in_table(table, VarVa::Kernel(va), pa, permissions)
}

/// Remove one 4 KiB leaf mapping from the active address space.
///
/// # Safety
///
/// The caller must serialize this operation with every other mutation of the
/// shared kernel page-table subtree and ensure the mapping is no longer in use.
pub unsafe fn unmap_page_from_active(va: crate::mm::addr::Va) -> Option<Pa> {
    let root = unsafe { &mut *crate::arch::asm::page_table::active() };
    let l1 = root.entry(vpn2(va)).page_table_mut()?;
    let l0 = l1.entry(vpn1(va)).page_table_mut()?;
    let entry = l0.entry(vpn0(va));
    if !entry.is_valid() || !entry.is_leaf() {
        return None;
    }
    let pa = entry.address();
    entry.clear();
    Some(pa)
}

fn map_in_table(table: &mut RawPageTable, va: VarVa, pa: Pa, permissions: Permission) -> bool {
    let mut flags: PteFlags = permissions.into();
    let va = match va {
        VarVa::User(uva) => {
            flags |= PteFlags::U;
            uva.into_va()
        }
        VarVa::Kernel(va) => va,
    };

    let mut entry = PageTableCursor { table }
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
