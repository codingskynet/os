use crate::asm;

use crate::debug;

pub const PAGE_FAULT_SMOKE_ADDR: usize = 0x3939_3939;

pub fn smoke() {
    debug!("page fault smoke: start");

    unsafe {
        // `ld` is a fixed-width 4-byte instruction. The page-fault handler
        // advances `sepc` by 4 under this feature, so execution resumes at the
        // next instruction after the intentional fault.
        asm!(
            "ld zero, 0({addr})",
            addr = in(reg) PAGE_FAULT_SMOKE_ADDR,
            options(nostack, readonly),
        );
    }

    debug!("page fault smoke: recovered");
}
