//! Small wrappers around RISC-V supervisor CSRs and instructions.
//!
//! These functions keep inline assembly localized so higher-level runtime code
//! can express intent through named operations.

use crate::asm;

pub mod interrupt {
    use super::*;

    pub const SSTATUS_SIE: usize = 1 << 1;

    pub fn is_enabled() -> bool {
        let sstatus: usize;
        unsafe {
            asm!(
                "csrr {sstatus}, sstatus",
                sstatus = out(reg) sstatus,
                options(nomem, nostack, preserves_flags),
            );
        }
        sstatus & SSTATUS_SIE != 0
    }

    pub fn enable() {
        unsafe {
            asm!(
                "csrs sstatus, {sie}",
                sie = in(reg) SSTATUS_SIE,
                options(nomem, nostack, preserves_flags),
            );
        }
    }

    pub fn disable() {
        unsafe {
            asm!(
                "csrc sstatus, {sie}",
                sie = in(reg) SSTATUS_SIE,
                options(nomem, nostack, preserves_flags),
            );
        }
    }

    const SIE_STIE: usize = 1 << 5;
    pub fn allow_timer() {
        unsafe {
            asm!(
                "csrs sie, {stie}",
                stie = in(reg) SIE_STIE,
                options(nomem, nostack, preserves_flags),
            );
        }
    }
}

pub mod page_table {
    use super::*;
    use crate::arch::page_table::{PageTable, SATP_MODE_SV39, ppn};
    use crate::mm::addr::{Pa, Va};

    const SATP_PPN_MASK: usize = (1usize << 44) - 1;

    pub fn active() -> *mut PageTable {
        let satp: usize;
        unsafe {
            asm!(
                "csrr {satp}, satp",
                satp = out(reg) satp,
                options(nomem, nostack, preserves_flags),
            );
        }
        debug_assert_eq!(satp & SATP_MODE_SV39, SATP_MODE_SV39);
        let root = Pa::new((satp & SATP_PPN_MASK) << 12);
        root.into_va().as_mut_ptr()
    }

    /// Activate `root` as the current Sv39 page table.
    ///
    /// # Safety
    ///
    /// `root` and every page table reachable from it must remain allocated and
    /// valid until another root is activated. The new address space must map
    /// the code, stack, trap vector, and other memory needed to complete the
    /// transition. The caller must also prevent concurrent page-table mutation
    /// or deallocation and ensure that an interrupt cannot observe an invalid
    /// transition state.
    pub unsafe fn activate(root: &PageTable) {
        unsafe { activate_from_pa(Va::from(root).into_pa()) };
    }

    /// Activate the Sv39 page-table root at `pa`.
    ///
    /// # Safety
    ///
    /// `pa` must be page-aligned and identify a valid Sv39 root page table.
    /// The root and every reachable child table must remain allocated and
    /// valid until another root is activated. The new address space must map
    /// all code, stack, trap, and data accesses required during the transition.
    /// The caller must prevent concurrent mutation or deallocation and ensure
    /// that interrupts cannot observe an invalid transition state.
    pub unsafe fn activate_from_pa(pa: Pa) {
        let ppn = ppn(pa);
        debug_assert_eq!(ppn & !SATP_PPN_MASK, 0, "page-table root exceeds satp PPN");
        let satp = SATP_MODE_SV39 | ppn;

        unsafe {
            asm!(
                "csrw satp, {satp}",
                "sfence.vma zero, zero",
                satp = in(reg) satp,
                options(nostack, preserves_flags),
            );
        }
    }

    pub fn flush_tlb() {
        unsafe {
            asm!("sfence.vma zero, zero", options(nostack, preserves_flags));
        }
    }
}

pub mod reg {
    use super::*;

    pub fn sp() -> usize {
        let sp: usize;
        unsafe {
            asm!(
                "mv {sp}, sp",
                sp = out(reg) sp,
                options(nomem, nostack, preserves_flags),
            );
        }
        sp
    }
}

pub mod time {
    use super::*;

    pub fn ticks() -> u64 {
        let ticks: u64;
        unsafe {
            asm!(
                "csrr {ticks}, time",
                ticks = out(reg) ticks,
                options(nomem, nostack, preserves_flags),
            );
        }
        ticks
    }
}

pub mod timer {
    use super::*;

    pub fn set_deadline(deadline: u64) {
        unsafe {
            asm!(
                "csrw stimecmp, {deadline}",
                deadline = in(reg) deadline,
                options(nomem, nostack, preserves_flags),
            );
        }
    }
}
