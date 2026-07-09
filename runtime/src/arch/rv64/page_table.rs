//! Sv39 paging.
//!
//! Sv39 virtual address, 64-bit register value:
//!
//! ```text
//!   63            39 38            30 29            21 20            12 11     0
//!  +----------------+----------------+----------------+----------------+--------+
//!  | sign extension |     VPN[2]     |     VPN[1]     |     VPN[0]     | offset |
//!  +----------------+----------------+----------------+----------------+--------+
//!       25 bits            9 bits           9 bits           9 bits      12 bits
//! ```
//!
//! Canonical rule: bits 63..39 must all equal bit 38.
//!
//! Page-table walk:
//!
//! ```text
//!   satp.PPN
//!      |
//!      v
//!   root page table, level 2              selected by VPN[2]
//!      |
//!      |-- leaf PTE: maps 1GiB if R/W/X != 0
//!      |
//!      v
//!   next page table, level 1              selected by VPN[1]
//!      |
//!      |-- leaf PTE: maps 2MiB if R/W/X != 0
//!      |
//!      v
//!   next page table, level 0              selected by VPN[0]
//!      |
//!      |-- leaf PTE: maps 4KiB if R/W/X != 0
//!      v
//!   physical page number + page offset
//! ```
//!
//! Sv39 page-table entry, 64-bit value:
//!
//! ```text
//!   63      54 53          28 27          19 18          10 9   8 7 6 5 4 3 2 1 0
//!  +----------+--------------+--------------+--------------+-----+-+-+-+-+-+-+-+-+
//!  | reserved |    PPN[2]    |    PPN[1]    |    PPN[0]    | RSW |D|A|G|U|X|W|R|V|
//!  +----------+--------------+--------------+--------------+-----+-+-+-+-+-+-+-+-+
//!     10 bits      26 bits        9 bits         9 bits     2 bits
//! ```
//!
//! Bits 0..7 are the hardware-defined flags represented by `PteFlags`.
//! Bits 8..9 are reserved for supervisor software and are currently unused.
//! Bits 10..53 hold the physical page number of either the next page table or
//! the mapped physical page.
//!
//! PTE kind:
//!
//! - `V=0`: invalid PTE.
//! - `V=1, R/W/X all zero`: pointer to the next page-table level.
//! - `V=1, any R/W/X set`: leaf PTE mapping memory.
//!
//! Accessed/dirty bits:
//!
//! A and D are meaningful for leaf PTEs. A records that the virtual page has
//! been read, written, or fetched since A was last cleared. D records that the
//! virtual page has been written since D was last cleared. Non-leaf PTEs should
//! keep A, D, and U clear.
//!
//! RISC-V allows two A/D management schemes:
//!
//! - Svadu, or the legacy no-Svade behavior: the MMU page-table walker updates
//!   the in-memory leaf PTE. It sets A on load, store, or fetch, and sets D on
//!   store.
//! - Svade: the MMU raises a page-fault exception instead when A must be set,
//!   or when a store needs D set. The kernel fault handler can then update the
//!   PTE and retry the faulting instruction.
//!
//! QEMU RISC-V behavior:
//!
//! QEMU models this with the effective ADUE setting. If ADUE is enabled, QEMU
//! sets PTE_A and, for stores, PTE_D during the page-table walk before filling
//! the TLB. The write-back is an atomic compare-and-swap when the PTE lives in
//! RAM; if the PTE is in ROM or MMIO and cannot be updated, translation fails
//! with a page fault. If ADUE is disabled, QEMU does not update the PTE and
//! instead faults when A or D is required but clear.
//!
//! On QEMU 11.0.1, `qemu-system-riscv64 -machine virt` defaults to an rv64 CPU
//! whose device tree advertises `svadu` and not `svade`; reset therefore starts
//! with `menvcfg.ADUE=1`, so A/D bits are automatically updated. To test
//! software-managed A/D faults, run with a CPU such as
//! `-cpu rv64,svadu=false,svade=true`. If both `svadu` and `svade` are enabled,
//! QEMU reset leaves ADUE clear, so M-mode software must set `menvcfg.ADUE` to
//! opt back into hardware-style A/D updates.

use core::mem::MaybeUninit;
use core::ptr;

use bitflags::bitflags;

use crate::mm::addr::{Pa, Va};

pub const SATP_MODE_SV39: usize = 8 << 60;

/// One 4 KiB Sv39 page table.
#[repr(C, align(4096))]
pub struct PageTable([PageTableEntry; 512]);

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

    pub fn entry(&mut self, index: usize) -> &mut PageTableEntry {
        &mut self.0[index]
    }
}

bitflags! {
    /// Hardware-defined bits 0..7 of an Sv39 page-table entry.
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

/// Raw Sv39 page-table entry.
#[derive(Clone, Copy, Default)]
#[repr(transparent)]
pub struct PageTableEntry(usize);

impl PageTableEntry {
    const RSW_MASK: usize = 0x3 << 8;
    const PPN_MASK: usize = ((1 << 44) - 1) << 10;

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

    pub fn or_insert_with(
        &mut self,
        alloc: impl FnOnce() -> &'static mut MaybeUninit<PageTable>,
    ) -> &mut PageTable {
        if self.flags().contains(PteFlags::V) {
            return unsafe { &mut *(self.address().into_va().as_mut_ptr()) };
        }

        let page_table = PageTable::init_mut(alloc());
        self.mut_address(Va::from(&mut *page_table).into_pa())
            .mut_flags(PteFlags::V);
        page_table
    }

    pub fn flags(&self) -> PteFlags {
        PteFlags::from_bits_truncate(self.0)
    }
}

const MASK: usize = (1 << 9) - 1;

pub fn ppn(pa: Pa) -> usize {
    pa.as_raw() >> 12
}

pub fn vpn2(va: Va) -> usize {
    (va.as_raw() >> 30) & MASK
}

pub fn vpn1(va: Va) -> usize {
    (va.as_raw() >> 21) & MASK
}

pub fn vpn0(va: Va) -> usize {
    (va.as_raw() >> 12) & MASK
}
