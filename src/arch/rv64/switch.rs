use core::arch::naked_asm;
use core::mem::offset_of;

use crate::arch::regs::GeneralRegs;
use crate::kernel::thread::Thread;

#[rustfmt::skip]
macro_rules! restore_regs {
    ($base:literal) => {
        concat!(
            "ld ra, {ra}(", $base, ")\n",
            "ld sp, {sp}(", $base, ")\n",
            "ld s0, {s0}(", $base, ")\n",
            "ld s1, {s1}(", $base, ")\n",
            "ld s2, {s2}(", $base, ")\n",
            "ld s3, {s3}(", $base, ")\n",
            "ld s4, {s4}(", $base, ")\n",
            "ld s5, {s5}(", $base, ")\n",
            "ld s6, {s6}(", $base, ")\n",
            "ld s7, {s7}(", $base, ")\n",
            "ld s8, {s8}(", $base, ")\n",
            "ld s9, {s9}(", $base, ")\n",
            "ld s10, {s10}(", $base, ")\n",
            "ld s11, {s11}(", $base, ")\n",
            "ld a0, {a0}(", $base, ")\n",
        )
    };
}

/// Switch from `current` to `next`, preserving the return point of the current
/// kernel thread and entering the saved context of the next one.
///
/// # Safety
///
/// Both pointers must reference live `GeneralRegs` storage. The pointed-to
/// thread objects must remain allocated across the switch and until the saved
/// context is resumed.
#[rustfmt::skip]
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _switch(
    _current: *mut GeneralRegs,
    _next: *const GeneralRegs,
    _prev: *mut Thread,
) {
    naked_asm!(
        "sd ra, {ra}(a0)",
        "sd sp, {sp}(a0)",
        "sd s0, {s0}(a0)",
        "sd s1, {s1}(a0)",
        "sd s2, {s2}(a0)",
        "sd s3, {s3}(a0)",
        "sd s4, {s4}(a0)",
        "sd s5, {s5}(a0)",
        "sd s6, {s6}(a0)",
        "sd s7, {s7}(a0)",
        "sd s8, {s8}(a0)",
        "sd s9, {s9}(a0)",
        "sd s10, {s10}(a0)",
        "sd s11, {s11}(a0)",
        restore_regs!("a1"),
        "addi sp, sp, -16",
        "sd a0, 0(sp)",
        "sd ra, 8(sp)",
        "mv a0, a2",
        "call {after_switch}",
        "ld a0, 0(sp)",
        "ld ra, 8(sp)",
        "addi sp, sp, 16",
        "ret",
        after_switch = sym crate::kernel::thread::after_switch,
        ra = const offset_of!(GeneralRegs, ra),
        sp = const offset_of!(GeneralRegs, sp),
        a0 = const offset_of!(GeneralRegs, a0),
        s0 = const offset_of!(GeneralRegs, s0),
        s1 = const offset_of!(GeneralRegs, s1),
        s2 = const offset_of!(GeneralRegs, s2),
        s3 = const offset_of!(GeneralRegs, s3),
        s4 = const offset_of!(GeneralRegs, s4),
        s5 = const offset_of!(GeneralRegs, s5),
        s6 = const offset_of!(GeneralRegs, s6),
        s7 = const offset_of!(GeneralRegs, s7),
        s8 = const offset_of!(GeneralRegs, s8),
        s9 = const offset_of!(GeneralRegs, s9),
        s10 = const offset_of!(GeneralRegs, s10),
        s11 = const offset_of!(GeneralRegs, s11),
    )
}

#[rustfmt::skip]
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _switch_to(_next: *const GeneralRegs) -> ! {
    naked_asm!(
        restore_regs!("a0"),
        "ret",
        ra = const offset_of!(GeneralRegs, ra),
        sp = const offset_of!(GeneralRegs, sp),
        a0 = const offset_of!(GeneralRegs, a0),
        s0 = const offset_of!(GeneralRegs, s0),
        s1 = const offset_of!(GeneralRegs, s1),
        s2 = const offset_of!(GeneralRegs, s2),
        s3 = const offset_of!(GeneralRegs, s3),
        s4 = const offset_of!(GeneralRegs, s4),
        s5 = const offset_of!(GeneralRegs, s5),
        s6 = const offset_of!(GeneralRegs, s6),
        s7 = const offset_of!(GeneralRegs, s7),
        s8 = const offset_of!(GeneralRegs, s8),
        s9 = const offset_of!(GeneralRegs, s9),
        s10 = const offset_of!(GeneralRegs, s10),
        s11 = const offset_of!(GeneralRegs, s11),
    )
}

/// First return target for a newly spawned kernel thread.
#[unsafe(naked)]
pub unsafe extern "C" fn _kernel_thread_trampoline() -> ! {
    naked_asm!(
        "call {start}",
        start = sym crate::kernel::thread::_kernel_thread_start,
    )
}
