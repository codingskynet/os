//! Sv39 paging.
//!
//! Sv39 virtual address, 64-bit register value:
//!
//!   63            39 38            30 29            21 20            12 11     0
//!  +----------------+----------------+----------------+----------------+--------+
//!  | sign extension |     VPN[2]     |     VPN[1]     |     VPN[0]     | offset |
//!  +----------------+----------------+----------------+----------------+--------+
//!       25 bits            9 bits           9 bits           9 bits      12 bits
//!
//!   Canonical rule: bits 63..39 must all equal bit 38.
//!
//! Page-table walk:
//!
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
//!
//! Sv39 page-table entry, 64-bit value:
//!
//!   63      54 53          28 27          19 18          10 9   8 7 6 5 4 3 2 1 0
//!  +----------+--------------+--------------+--------------+-----+-+-+-+-+-+-+-+-+
//!  | reserved |    PPN[2]    |    PPN[1]    |    PPN[0]    | RSW |D|A|G|U|X|W|R|V|
//!  +----------+--------------+--------------+--------------+-----+-+-+-+-+-+-+-+-+
//!     10 bits      26 bits        9 bits         9 bits     2 bits
//!
//!   Bits 0..7 are the hardware-defined flags represented by `PteFlags`.
//!   Bits 8..9 are reserved for supervisor software and are currently unused.
//!   Bits 10..53 hold the physical page number of either the next page table
//!   or the mapped physical page.
//!
//! PTE kind:
//!
//!   V=0                  invalid PTE
//!   V=1, R/W/X all zero  pointer to the next page-table level
//!   V=1, any R/W/X set   leaf PTE mapping memory
//!
//! Accessed/dirty bits:
//!
//!   A and D are meaningful for leaf PTEs. A records that the virtual page
//!   has been read, written, or fetched since A was last cleared. D records
//!   that the virtual page has been written since D was last cleared.
//!   Non-leaf PTEs should keep A, D, and U clear.
//!
//!   RISC-V allows two A/D management schemes:
//!
//!   - Svadu, or the legacy no-Svade behavior: the MMU page-table walker
//!     updates the in-memory leaf PTE. It sets A on load, store, or fetch,
//!     and sets D on store.
//!   - Svade: the MMU raises a page-fault exception instead when A must be
//!     set, or when a store needs D set. The kernel fault handler can then
//!     update the PTE and retry the faulting instruction.
//!
//! QEMU RISC-V behavior:
//!
//!   QEMU models this with the effective ADUE setting. If ADUE is enabled,
//!   QEMU sets PTE_A and, for stores, PTE_D during the page-table walk before
//!   filling the TLB. The write-back is an atomic compare-and-swap when the
//!   PTE lives in RAM; if the PTE is in ROM or MMIO and cannot be updated,
//!   translation fails with a page fault. If ADUE is disabled, QEMU does not
//!   update the PTE and instead faults when A or D is required but clear.
//!
//!   On QEMU 11.0.1, `qemu-system-riscv64 -machine virt` defaults to an rv64
//!   CPU whose device tree advertises `svadu` and not `svade`; reset therefore
//!   starts with `menvcfg.ADUE=1`, so A/D bits are automatically updated.
//!   To test software-managed A/D faults, run with a CPU such as
//!   `-cpu rv64,svadu=false,svade=true`. If both `svadu` and `svade` are
//!   enabled, QEMU reset leaves ADUE clear, so M-mode software must set
//!   `menvcfg.ADUE` to opt back into hardware-style A/D updates.

use bitflags::bitflags;

use crate::mm::addr::{Pa, Va};

pub const SATP_MODE_SV39: usize = 8 << 60;

#[repr(C, align(4096))]
pub struct PageTable([PageTableEntry; 512]);

impl Default for PageTable {
    fn default() -> Self {
        Self([PageTableEntry::default(); 512])
    }
}

impl PageTable {
    pub const fn new() -> Self {
        Self([PageTableEntry(0); 512])
    }

    pub fn entry(&mut self, index: usize) -> &mut PageTableEntry {
        &mut self.0[index]
    }
}

bitflags! {
    // These flags occupy bits 0..7 of a 64-bit Sv39 PTE.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PteFlags: usize {
        /// valid. If clear, the PTE is invalid and other bits are ignored.
        const V = 1 << 0;
        /// readable leaf mapping.
        const R = 1 << 1;
        /// writable leaf mapping. The spec reserves W=1,R=0 as invalid.
        const W = 1 << 2;
        /// executable leaf mapping.
        const X = 1 << 3;
        /// user-accessible when set; supervisor-only when clear.
        const U = 1 << 4;
        /// global mapping, shared across address spaces.
        const G = 1 << 5;
        /// accessed. Set by hardware, or pre-set by the kernel.
        const A = 1 << 6;
        /// dirty. Set by hardware on writes, or pre-set by the kernel.
        const D = 1 << 7;
    }
}

#[derive(Clone, Copy, Default)]
#[repr(transparent)]
pub struct PageTableEntry(usize);

impl PageTableEntry {
    const RSW_MASK: usize = 0x3 << 8;
    const PPN_MASK: usize = ((1 << 44) - 1) << 10;

    /// Store a page-aligned physical address, preserving flags and RSW
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

    /// Decode page-aligned physical address
    pub fn address(&self) -> Pa {
        Pa::new(((self.0 & Self::PPN_MASK) >> 10) << 12)
    }

    // pub fn page_table(&self) -> Option<&mut PageTable> {
    //     if self.flags().contains(PteFlags::V) {
    //         unsafe { Some(&mut *(self.address().to_va().as_mut_ptr())) }
    //     } else {
    //         None
    //     }
    // }

    pub fn or_insert_with(&mut self, default: impl FnOnce() -> *mut PageTable) -> &mut PageTable {
        if self.flags().contains(PteFlags::V) {
            return unsafe { &mut *(self.address().into_va().as_mut_ptr()) };
        }

        let page_table = unsafe { &mut *default() };
        *page_table = PageTable::default();

        self.mut_address(Va::from(&mut *page_table).into_pa())
            .mut_flags(PteFlags::V);
        page_table
    }

    /// Decode PTE flags
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
