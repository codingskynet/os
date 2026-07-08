use core::arch::asm;

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
