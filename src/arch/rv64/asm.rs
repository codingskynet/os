use core::arch::asm;

pub mod interrupt {
    use super::*;

    const SSTATUS_SIE: usize = 1 << 1;

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
