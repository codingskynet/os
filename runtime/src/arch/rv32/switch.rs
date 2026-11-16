use core::arch::naked_asm;
use core::mem::offset_of;

use crate::arch::Sstatus;
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
    pub fn as_kernel_thread_trampoline(&mut self, sp: Va) {
        self.ra = _kernel_thread_trampoline as *const u8 as usize;
        self.sp = sp.as_raw();
        self.sstatus = Sstatus::SIE.bits(); // start from interrupt-enabled
    }
}

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

macro_rules! store_regs {
    ($base:literal) => {
        $crate::asm!(@asm_lines(
            ("sw ra, {ra}(", $base, ")"),
            ("sw sp, {sp}(", $base, ")"),
            ("sw s0, {s0}(", $base, ")"),
            ("sw s1, {s1}(", $base, ")"),
            ("sw s2, {s2}(", $base, ")"),
            ("sw s3, {s3}(", $base, ")"),
            ("sw s4, {s4}(", $base, ")"),
            ("sw s5, {s5}(", $base, ")"),
            ("sw s6, {s6}(", $base, ")"),
            ("sw s7, {s7}(", $base, ")"),
            ("sw s8, {s8}(", $base, ")"),
            ("sw s9, {s9}(", $base, ")"),
            ("sw s10, {s10}(", $base, ")"),
            ("sw s11, {s11}(", $base, ")"),
        ))
    };
}

macro_rules! restore_regs {
    ($base:literal) => {
        $crate::asm!(@asm_lines(
            ("lw ra, {ra}(", $base, ")"),
            ("lw sp, {sp}(", $base, ")"),
            ("lw s0, {s0}(", $base, ")"),
            ("lw s1, {s1}(", $base, ")"),
            ("lw s2, {s2}(", $base, ")"),
            ("lw s3, {s3}(", $base, ")"),
            ("lw s4, {s4}(", $base, ")"),
            ("lw s5, {s5}(", $base, ")"),
            ("lw s6, {s6}(", $base, ")"),
            ("lw s7, {s7}(", $base, ")"),
            ("lw s8, {s8}(", $base, ")"),
            ("lw s9, {s9}(", $base, ")"),
            ("lw s10, {s10}(", $base, ")"),
            ("lw s11, {s11}(", $base, ")"),
            ("lw a0, {a0}(", $base, ")"),
        ))
    };
}

macro_rules! switch_context_naked_asm {
    ($($template:expr),+ $(,)?) => {
        switch_context_naked_asm!(
            ".option arch, rv32ima",
            ".option norvc",$($template,)* ;)
    };

    ($($template:expr,)* ; $($extra_operand:tt)*) => {
        naked_asm!(
            ".option arch, rv32ima",
            ".option norvc",
            $($template,)*
            $($extra_operand)*
            sie = const Sstatus::SIE.bits(),
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
/// be the `Thread` that owns `current`. Before calling this routine, the caller
/// must activate the memory context owned by the `Thread` containing `next` and
/// install that `Thread` in the global current-thread owner. The active page
/// table must keep the switch code, both `Thread` allocations, the outgoing
/// kernel stack until `sp` is restored, and the incoming kernel stack mapped
/// throughout the handoff. Both allocations must remain valid until their saved
/// contexts are resumed or `_after_switch` observes that one exited. This
/// routine only saves and restores architectural context before returning into
/// `next`'s saved `ra`; it does not activate a page table.
#[unsafe(naked)]
pub unsafe extern "C" fn _switch(
    _current: *mut SwitchContext,
    _next: *const SwitchContext,
    _prev: *mut Thread,
) {
    switch_context_naked_asm!(
            ".option arch, rv32ima",
            ".option norvc",
        // Read the outgoing kernel thread's supervisor status.
        "csrr t0, sstatus",
        "sw t0, {sstatus}(a0)",
        // Save the outgoing thread's integer callee-saved register state.
        store_regs!("a0"),
        // Restore the incoming thread's integer callee-saved register state.
        restore_regs!("a1"),
        // Load the incoming thread's saved supervisor status.
        "lw t0, {sstatus}(a1)",
        // Preserve values that are needed after `_after_switch(prev)` returns.
        // At this point `sp` belongs to `next`, so this scratch frame is on the
        // newly selected thread's stack.
        "addi sp, sp, -32",
        // Keep the current context pointer and return address alive across the
        // ordinary Rust call into `_after_switch`.
        "sw a0, 0(sp)",
        "sw ra, 8(sp)",
        // Keep `next.sstatus` until the very end: restoring SIE too early would
        // allow a timer interrupt in the middle of the switch epilogue.
        "sw t0, 16(sp)",
        "mv a0, a2",
        "call {after_switch}",
        // Tear down the scratch frame, then restore next's interrupt-enable bit
        // immediately before returning into next's saved continuation.
        "lw t0, 16(sp)",
        "lw a0, 0(sp)",
        "lw ra, 8(sp)",
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
        ".option arch, rv32ima",
        ".option norvc",
        // Load the first thread's saved supervisor status.
        "lw t0, {sstatus}(a0)",
        // Restore the first thread's integer register state.
        restore_regs!("a0"),
        // Restore its saved supervisor interrupt-enable state.
        restore_sie_from!("t0"),
        // Enter the first thread at its saved return address.
        "ret",
    )
}

/// First return target for a newly spawned kernel thread.
///
/// # Safety
///
/// This function is entered only through a prepared `SwitchContext`; `sp` must
/// point at the new thread's kernel stack. The thread itself is resolved
/// through [`crate::kernel::thread::CurrentThread`], so no argument register
/// carries a `Thread` pointer.
#[unsafe(naked)]
pub unsafe extern "C" fn _kernel_thread_trampoline() -> ! {
    naked_asm!(
            ".option arch, rv32ima",
            ".option norvc",
        "call {start}",
        start = sym crate::kernel::thread::_kernel_thread_start,
    )
}
