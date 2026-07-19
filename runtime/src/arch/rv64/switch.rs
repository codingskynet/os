//! RISC-V kernel-thread context switching.
//!
//! Switch contexts store callee-saved integer state plus the complete RV64D
//! floating-point state needed to suspend and resume kernel threads. Kernel
//! code runs with `sstatus.FS=Off`; the switch assembly enables the FPU only
//! while copying a context and disables it again before calling Rust code.

use core::arch::naked_asm;
use core::mem::offset_of;

use crate::arch::Sstatus;
use crate::arch::regs::{CalleeSavedRegs, FpRegs};
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
    fp_regs: FpRegs,
    fcsr: usize,
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

// Naked assembly is lowered as global assembly, where rustc does not currently
// pass the target's D extension to LLVM's assembler. Enable it only around one
// FP instruction fragment so the option cannot leak into surrounding assembly.
macro_rules! fp_asm {
    ($($item:tt),+ $(,)?) => {
        $crate::asm!(@asm_lines(
            ".option push",
            ".option arch, +d",
            $($item,)+
            ".option pop",
        ))
    };
}

// Generate one load or store for every contiguous RV64D register slot.
macro_rules! fp_regs_asm {
    ($instruction:literal, $base:literal) => {
        fp_regs_asm!(@expand $instruction, $base;
            f0 => 0, f1 => 8, f2 => 16, f3 => 24,
            f4 => 32, f5 => 40, f6 => 48, f7 => 56,
            f8 => 64, f9 => 72, f10 => 80, f11 => 88,
            f12 => 96, f13 => 104, f14 => 112, f15 => 120,
            f16 => 128, f17 => 136, f18 => 144, f19 => 152,
            f20 => 160, f21 => 168, f22 => 176, f23 => 184,
            f24 => 192, f25 => 200, f26 => 208, f27 => 216,
            f28 => 224, f29 => 232, f30 => 240, f31 => 248,
        )
    };

    (@expand $instruction:literal, $base:literal;
        $($reg:ident => $offset:literal),+ $(,)?) => {
        fp_asm!(
            $((
                $instruction, " ", stringify!($reg), ", ", stringify!($offset), "(", $base, ")"
            ),)+
        )
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
        // Read the outgoing kernel thread's supervisor status.
        "csrr t0, sstatus",
        // Save it before temporarily enabling floating-point instructions.
        "sd t0, {sstatus}(a0)",
        // TODO: Track FS state and the hart's FP owner so threads that do not
        // use floating point can skip these eager save/restore operations.
        // Build the temporary FS=Dirty value that permits FP instructions.
        "li t0, {fs_dirty}",
        // Enable the FPU only for the context-copy sequence below.
        "csrs sstatus, t0",
        // Point at the outgoing thread's FP register save area.
        "addi t0, a0, {fp_regs}",
        // Save all 32 outgoing RV64D registers.
        fp_regs_asm!("fsd", "t0"),
        // Read the outgoing floating-point control and status register.
        fp_asm!(
            "frcsr t1",
            // Save its rounding mode and accumulated exception flags.
            "sd t1, {fcsr}(a0)",
        ),
        // Point at the incoming thread's FP register save area.
        "addi t0, a1, {fp_regs}",
        // Restore all 32 incoming RV64D registers.
        fp_regs_asm!("fld", "t0"),
        // Load the incoming floating-point control and status register.
        fp_asm!(
            "ld t1, {fcsr}(a1)",
            // Restore its rounding mode and accumulated exception flags.
            "fscsr t1",
        ),
        // Build the mask for both FS bits.
        "li t0, {fs}",
        // Disable the FPU again before returning to ordinary kernel code.
        "csrc sstatus, t0",
        // Save the outgoing thread's integer callee-saved register state.
        store_regs!("a0"),
        // Restore the incoming thread's integer callee-saved register state.
        restore_regs!("a1"),
        // Load the incoming thread's saved supervisor status.
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
        fs = const Sstatus::FS.bits(),
        fs_dirty = const Sstatus::FS_DIRTY.bits(),
        fp_regs = const offset_of!(SwitchContext, fp_regs),
        fcsr = const offset_of!(SwitchContext, fcsr),
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
        // Load the first thread's saved supervisor status.
        "ld t0, {sstatus}(a0)",
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
        "call {start}",
        start = sym crate::kernel::thread::_kernel_thread_start,
    )
}
