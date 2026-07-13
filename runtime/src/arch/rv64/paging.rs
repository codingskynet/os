//! Runtime paging operations after the final kernel page table is active.

use crate::arch::consts::PAGE_SIZE;
use crate::arch::page_table::{PageTable, SATP_MODE_SV39, vpn0, vpn1, vpn2};
use crate::asm;
use crate::mm::addr::{Pa, Va};
use crate::mm::region::Region;

const SATP_PPN_MASK: usize = (1usize << 44) - 1;

pub fn active_root() -> &'static mut PageTable {
    let satp: usize;
    unsafe {
        asm!(
            "csrr {satp}, satp",
            satp = out(reg) satp,
            options(nomem, nostack, preserves_flags),
        );
    }

    assert_eq!(satp & SATP_MODE_SV39, SATP_MODE_SV39);
    let root = Pa::new((satp & SATP_PPN_MASK) << 12);
    unsafe { &mut *root.into_va().as_mut_ptr() }
}

/// Remove mappings for a page-aligned kernel image region.
///
/// This is used when `.init.*` memory has been reclaimed so stale virtual
/// aliases cannot continue executing or reading it.
pub fn unmap_kernel_region(region: Region) {
    assert_eq!(region.start.align_down(PAGE_SIZE), region.start);
    assert_eq!(region.end.align_down(PAGE_SIZE), region.end);

    let root = active_root();
    let mut pa = region.start;
    while pa < region.end {
        unmap(root, pa.into_kernel_va());
        pa = pa.checked_offset(PAGE_SIZE.get()).unwrap();
    }

    unsafe {
        asm!("sfence.vma zero, zero", options(nostack, preserves_flags));
    }
}

fn unmap(root: &mut PageTable, va: Va) {
    let l1 = root
        .entry(vpn2(va))
        .page_table_mut()
        .expect("missing level-1 page table");
    let l0 = l1
        .entry(vpn1(va))
        .page_table_mut()
        .expect("missing level-0 page table");
    let pte = l0.entry(vpn0(va));

    assert!(pte.is_valid(), "unmapping an unmapped page");
    assert!(pte.is_leaf(), "unmapping a non-leaf page table entry");
    assert_eq!(pte.address(), va.into_pa());

    pte.clear();
}
