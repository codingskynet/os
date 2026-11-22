use core::mem::{self, MaybeUninit};
use core::ops::{Deref, DerefMut};
use core::ptr;

use bitflags::bitflags;

use super::asm;
use super::consts::UPPER_CANONICAL_BASE;
use crate::arch::paging::{MemoryType, Permission};
use crate::mm::Pages;
use crate::mm::addr::{Pa, Va};

pub const SATP_MODE_SV32: usize = 1 << 31;

/// One 4 KiB Sv32 page table.
#[repr(C, align(4096))]
pub struct PageTable([PageTableEntry; 1024]);

impl Deref for PageTable {
    type Target = [PageTableEntry; 1024];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for PageTable {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl PageTable {
    pub fn init_mut(page_table: &mut MaybeUninit<Self>) -> &mut Self {
        unsafe { &mut *Self::init_raw_mut(page_table) }
    }

    /// Zero-initialize a page-table slot without materializing a 4 KiB
    /// `PageTable` temporary on the current stack.
    ///
    /// # Safety
    ///
    /// `page_table` must be non-null, properly aligned, valid for writes of
    /// `PageTable`, and uniquely owned for the duration of initialization.
    pub unsafe fn init_raw_mut(page_table: *mut MaybeUninit<Self>) -> *mut Self {
        let page_table = page_table.cast::<Self>();
        unsafe {
            ptr::write_bytes(page_table, 0, 1);
        }
        page_table
    }

    /// Initialize a root by sharing the active root's upper-half mappings.
    ///
    /// Every copied non-leaf PTE is an owning edge to another page-table page,
    /// so this method also acquires the corresponding raw [`Pages`] reference.
    pub fn init_from_root(page_table: &mut MaybeUninit<Self>) -> &mut Self {
        let upper_start = vpn1(Va::new(UPPER_CANONICAL_BASE));
        let destination = page_table.as_mut_ptr().cast::<PageTableEntry>();

        unsafe {
            let root = &*asm::page_table::active();
            ptr::write_bytes(destination, 0, upper_start);
            ptr::copy_nonoverlapping(
                root.0.as_ptr().add(upper_start),
                destination.add(upper_start),
                root.0.len() - upper_start,
            );

            let page_table = page_table.assume_init_mut();
            for entry in page_table[upper_start..].iter() {
                if entry.is_valid() && !entry.is_leaf() {
                    // Reconstitute the active root's raw reference, clone it
                    // for the copied PTE, then return both handles to raw
                    // ownership so neither PTE loses its strong reference.
                    let pages = Pages::from_raw(entry.address());
                    mem::forget(pages.clone());
                    mem::forget(pages);
                }
            }
            page_table
        }
    }

    pub fn entry(&mut self, index: usize) -> &mut PageTableEntry {
        &mut self.0[index]
    }
}

bitflags! {
    /// Hardware-defined bits 0..7 of an Sv32 page-table entry.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PteFlags: usize {
        /// Valid. If clear, the PTE is invalid and other bits are ignored.
        const V = 1 << 0;

        /// Readable leaf mapping.
        const R = 1 << 1;

        /// Writable leaf mapping. The spec reserves W=1,R=0 as invalid.
        const W = 1 << 2;

        /// Executable leaf mapping.
        const X = 1 << 3;

        /// User-accessible when set; supervisor-only when clear.
        const U = 1 << 4;

        /// Global mapping, shared across address spaces.
        const G = 1 << 5;

        /// Accessed. Set by hardware, or pre-set by the kernel.
        const A = 1 << 6;

        /// Dirty. Set by hardware on writes, or pre-set by the kernel.
        const D = 1 << 7;
    }
}

impl PteFlags {
    pub(in crate::arch::rv) fn memory_attributes(_memory_type: MemoryType) -> Self {
        // Sv32 leaves have no memory-type field.
        Self::empty()
    }
}

impl From<Permission> for PteFlags {
    fn from(value: Permission) -> Self {
        let mut flags = Self::empty();

        if value.contains(Permission::R) {
            flags |= Self::R;
        }

        if value.contains(Permission::W) {
            flags |= Self::R | Self::W;
        }

        if value.contains(Permission::X) {
            flags |= Self::X;
        }

        flags | Self::A | Self::D
    }
}

/// Raw Sv32 page-table entry.
#[derive(Clone, Copy, Default)]
#[repr(transparent)]
pub struct PageTableEntry(usize);

impl PageTableEntry {
    const RSW_MASK: usize = 0x3 << 8;
    const PPN_MASK: usize = ((1usize << 22) - 1) << 10;

    /// Store a page-aligned physical address, preserving flags and RSW bits.
    pub fn mut_address(&mut self, addr: Pa) -> &mut Self {
        let low = self.0 & (PteFlags::all().bits() | Self::RSW_MASK);
        self.0 = ((addr.as_raw() >> 12) << 10) | low;
        self
    }

    /// OR in PTE flags. This intentionally preserves existing
    /// flags; add a separate setter if a caller needs to clear flags.
    pub fn mut_flags(&mut self, flags: PteFlags) -> &mut Self {
        self.0 |= flags.bits();
        self
    }

    /// Decode the page-aligned physical address.
    pub fn address(&self) -> Pa {
        Pa::new(((self.0 & Self::PPN_MASK) >> 10) << 12)
    }

    pub fn clear(&mut self) {
        self.0 = 0;
    }

    pub fn is_valid(&self) -> bool {
        self.flags().contains(PteFlags::V)
    }

    pub fn is_leaf(&self) -> bool {
        self.flags()
            .intersects(PteFlags::R | PteFlags::W | PteFlags::X)
    }

    pub fn page_table_mut(&mut self) -> Option<&mut PageTable> {
        if self.is_valid() && !self.is_leaf() {
            Some(unsafe { &mut *(self.address().into_va().as_mut_ptr()) })
        } else {
            None
        }
    }

    pub fn flags(&self) -> PteFlags {
        PteFlags::from_bits_truncate(self.0)
    }
}

const MASK: usize = (1 << 10) - 1;

pub fn ppn(pa: Pa) -> usize {
    pa.as_raw() >> 12
}

pub fn vpn1(va: Va) -> usize {
    (va.as_raw() >> 22) & MASK
}

pub fn vpn0(va: Va) -> usize {
    (va.as_raw() >> 12) & MASK
}

pub(in crate::arch::rv) fn branch_indices(va: Va) -> [usize; 1] {
    [vpn1(va)]
}

pub fn unmap_from_active(va: Va) {
    let root = unsafe { &mut *asm::page_table::active() };

    // TODO: pte is not guaranteed that it is always on L0
    let pte = root
        .entry(vpn1(va))
        .page_table_mut()
        .unwrap()
        .entry(vpn0(va));
    debug_assert!(pte.is_valid(), "unmapping an unmapped page");
    debug_assert!(pte.is_leaf(), "unmapping a non-leaf page table entry");
    debug_assert_eq!(pte.address(), va.into_pa());

    pte.clear();
}
