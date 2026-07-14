//! RISC-V kernel-thread context switching.
//!
//! Switch contexts store callee-saved state plus the saved stack, return
//! address, first argument, and interrupt-enable bit needed to suspend and
//! resume kernel threads.

use core::arch::naked_asm;
use core::mem::offset_of;

use crate::arch::asm::interrupt::SSTATUS_SIE;
use crate::arch::regs::CalleeSavedRegs;
use crate::kernel::thread::Thread;
use crate::mm::addr::Va;

/// Saved execution context for one kernel thread.
#[derive(Default)]
#[repr(C)]
pub struct SwitchContext {
    regs: CalleeSavedRegs,
    ra: usize,
    sp: usize,
    a0: usize,
    sstatus: usize,
}

impl SwitchContext {
    pub fn as_kernel_thread_trampoline(&mut self, sp: Va, entry: Va) {
        self.ra = _kernel_thread_trampoline as *const u8 as usize;
        self.sp = sp.as_raw();
        self.a0 = entry.as_raw();
        self.sstatus = SSTATUS_SIE; // start from interrupt-enabled
    }
}

#[rustfmt::skip]
macro_rules! restore_sie_from {
    ($reg:literal) => {
        $crate::asm!(@asm_lines(
            "li t1, {sie}",
            ("and t0, ", $reg, ", t1"),
            "beqz t0, 1f",
            "csrs sstatus, t1",
            "j 2f",
            "1:",
            "csrc sstatus, t1",
            "2:",
        ))
    };
}

#[rustfmt::skip]
macro_rules! store_regs {
    ($base:literal) => {
        $crate::asm!(@asm_lines(
            ("sd ra, {ra}(", $base, ")"),
            ("sd sp, {sp}(", $base, ")"),
            ("sd s0, {s0}(", $base, ")"),
            ("sd s1, {s1}(", $base, ")"),
            ("sd s2, {s2}(", $base, ")"),
            ("sd s3, {s3}(", $base, ")"),
            ("sd s4, {s4}(", $base, ")"),
            ("sd s5, {s5}(", $base, ")"),
            ("sd s6, {s6}(", $base, ")"),
            ("sd s7, {s7}(", $base, ")"),
            ("sd s8, {s8}(", $base, ")"),
            ("sd s9, {s9}(", $base, ")"),
            ("sd s10, {s10}(", $base, ")"),
            ("sd s11, {s11}(", $base, ")"),
        ))
    };
}

#[rustfmt::skip]
macro_rules! restore_regs {
    ($base:literal) => {
        $crate::asm!(@asm_lines(
            ("ld ra, {ra}(", $base, ")"),
            ("ld sp, {sp}(", $base, ")"),
            ("ld s0, {s0}(", $base, ")"),
            ("ld s1, {s1}(", $base, ")"),
            ("ld s2, {s2}(", $base, ")"),
            ("ld s3, {s3}(", $base, ")"),
            ("ld s4, {s4}(", $base, ")"),
            ("ld s5, {s5}(", $base, ")"),
            ("ld s6, {s6}(", $base, ")"),
            ("ld s7, {s7}(", $base, ")"),
            ("ld s8, {s8}(", $base, ")"),
            ("ld s9, {s9}(", $base, ")"),
            ("ld s10, {s10}(", $base, ")"),
            ("ld s11, {s11}(", $base, ")"),
            ("ld a0, {a0}(", $base, ")"),
        ))
    };
}

macro_rules! switch_context_naked_asm {
    ($($template:expr),+ $(,)?) => {
        switch_context_naked_asm!($($template,)* ;)
    };

    ($($template:expr,)* ; $($extra_operand:tt)*) => {
        naked_asm!(
            $($template,)*
            $($extra_operand)*
            sie = const SSTATUS_SIE,
            ra = const offset_of!(SwitchContext, ra),
            sp = const offset_of!(SwitchContext, sp),
            a0 = const offset_of!(SwitchContext, a0),
            sstatus = const offset_of!(SwitchContext, sstatus),
            s0 = const offset_of!(SwitchContext, regs.s0),
            s1 = const offset_of!(SwitchContext, regs.s1),
            s2 = const offset_of!(SwitchContext, regs.s2),
            s3 = const offset_of!(SwitchContext, regs.s3),
            s4 = const offset_of!(SwitchContext, regs.s4),
            s5 = const offset_of!(SwitchContext, regs.s5),
            s6 = const offset_of!(SwitchContext, regs.s6),
            s7 = const offset_of!(SwitchContext, regs.s7),
            s8 = const offset_of!(SwitchContext, regs.s8),
            s9 = const offset_of!(SwitchContext, regs.s9),
            s10 = const offset_of!(SwitchContext, regs.s10),
            s11 = const offset_of!(SwitchContext, regs.s11),
        )
    };
}

/// Switch from `current` to `next`, preserving the return point of the current
/// kernel thread and entering the saved context of the next one.
///
/// # Safety
///
/// `current` must point to the switch context of the running thread, `next`
/// must point to the switch context of a different live thread, and `prev` must
/// be the `Thread` that owns `current`. Both thread allocations must remain
/// valid until their saved contexts are resumed or `after_switch` observes that
/// one has exited. This routine restores `next`, activates its page table
/// through `after_switch`, then returns into `next`'s saved `ra`.
#[rustfmt::skip]
#[unsafe(naked)]
pub unsafe extern "C" fn _switch(
    _current: *mut SwitchContext,
    _next: *const SwitchContext,
    _prev: *mut Thread,
) {
    switch_context_naked_asm!(
        "csrr t0, sstatus",
        "sd t0, {sstatus}(a0)",
        store_regs!("a0"),
        restore_regs!("a1"),
        "ld t0, {sstatus}(a1)",
        // Preserve values that are needed after `_after_switch(prev)` returns.
        // At this point `sp` belongs to `next`, so this scratch frame is on the
        // newly selected thread's stack.
        "addi sp, sp, -32",
        // Keep the current context pointer and return address alive across the
        // ordinary Rust call into `_after_switch`.
        "sd a0, 0(sp)",
        "sd ra, 8(sp)",
        // Keep `next.sstatus` until the very end: restoring SIE too early would
        // allow a timer interrupt in the middle of the switch epilogue.
        "sd t0, 16(sp)",
        "mv a0, a2",
        "call {after_switch}",
        // Tear down the scratch frame, then restore next's interrupt-enable bit
        // immediately before returning into next's saved continuation.
        "ld t0, 16(sp)",
        "ld a0, 0(sp)",
        "ld ra, 8(sp)",
        "addi sp, sp, 32",
        restore_sie_from!("t0"),
        "ret",
        ;
        after_switch = sym crate::kernel::thread::_after_switch,
    )
}

/// Switch directly into the first kernel thread context.
///
/// # Safety
///
/// `next` must point to a live `SwitchContext` initialized for a kernel thread.
/// Its saved stack pointer and return address must be valid because this
/// routine never returns to the caller's stack.
#[unsafe(naked)]
pub unsafe extern "C" fn _switch_to(_next: *const SwitchContext) -> ! {
    switch_context_naked_asm!(
        "ld t0, {sstatus}(a0)",
        restore_regs!("a0"),
        restore_sie_from!("t0"),
        "ret",
    )
}

/// First return target for a newly spawned kernel thread.
///
/// # Safety
///
/// This function is entered only through a prepared `SwitchContext`; `a0` must
/// contain a valid `Thread` pointer and `sp` must already point at that
/// thread's kernel stack.
#[unsafe(naked)]
pub unsafe extern "C" fn _kernel_thread_trampoline() -> ! {
    naked_asm!(
        "call {start}",
        start = sym crate::kernel::thread::_kernel_thread_start,
    )
}
