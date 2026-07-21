//! RISC-V supervisor trap handling.
//!
//! Trap entry saves general-purpose registers plus the supervisor CSRs that
//! describe or manage leaving normal execution:
//!
//!   `sepc`
//!      Supervisor exception program counter. Hardware writes the return PC
//!      here on trap entry, and `sret` resumes from this address. For page
//!      faults it usually points at the faulting instruction so the instruction
//!      can be retried after the fault is handled. For `ecall`, a handler would
//!      normally advance it past the `ecall` instruction before returning.
//!
//!   `sstatus`
//!      Supervisor status. On trap entry, hardware records the interrupted
//!      privilege and interrupt-enable state in fields such as SPP and SPIE,
//!      and clears SIE while the trap handler runs. Restoring this value before
//!      `sret` controls which privilege mode and interrupt state execution
//!      returns to.
//!
//!   `scause`
//!      Supervisor trap cause. The most-significant bit tells whether the trap
//!      is an interrupt; the remaining bits are the exception or interrupt
//!      cause code. For example, load page fault is exception code 13 and
//!      store/AMO page fault is exception code 15.
//!
//!   `stval`
//!      Supervisor trap value. Its meaning depends on `scause`. For page
//!      faults, it contains the faulting virtual address. For some other traps
//!      it may contain an instruction value or be zero.
//!
//!   `sscratch`
//!      Supervisor scratch CSR. While S-mode runs it is 0 and `sp` is the live
//!      task kernel stack; while U-mode runs it holds a mapped scratch anchor
//!      at the kernel-stack top and `sp` is the user stack. Trap entry also uses
//!      it to park an S-mode `sp` while checking whether a `TrapFrame` would
//!      cross a guard page. An overflow switches to the boot hart's emergency
//!      panic stack.
//!
//! `sepc`, `sstatus`, `scause`, and `stval` are part of `TrapFrame` rather than
//! `GeneralRegs` because they are not architectural integer registers; they
//! are control/status state saved by hardware on trap entry and consumed by
//! `sret` on trap return. `sscratch` stays outside the frame and is maintained
//! by the trap entry/return path around that stack switch.

mod exception;

use core::arch::naked_asm;
use core::mem::{offset_of, size_of};

use super::regs::GeneralRegs;
use super::{Exception, Interrupt, Scause, Sstatus, TrapCause};
use crate::arch::asm::floating_point;
use crate::arch::consts::{KERNEL_STACK_GUARD_SIZE, KERNEL_STACK_SLOT_SIZE, PAGE_SIZE, STACK_SIZE};
use crate::arch::timer::handle_timer;
use crate::arch::trap::exception::handle_exception;
use crate::asm;
use crate::kernel::per_core::PerCore;
use crate::mm::addr::Va;

// A U-mode trap needs one location from which to restore the kernel's per-core
// pointer. Keep it as the trailing part of TrapFrame at the kernel-stack top.
const TRAP_ENTRY_SCRATCH_SIZE: usize = size_of::<TrapEntryScratch>();
const ENTRY_KERNEL_TP: usize = offset_of!(TrapEntryScratch, kernel_tp);

pub fn init() {
    unsafe {
        asm!(
            "csrw stvec, {entry}",
            "csrw sscratch, zero",
            entry = in(reg) _trap_entry as *const u8 as usize,
            options(nostack, preserves_flags),
        );
    }
    floating_point::disable();
}

/// Leave S-mode and begin a freshly loaded program in U-mode.
///
/// # Safety
///
/// `entry` and the memory below `user_sp` must be valid user mappings in the
/// active address space. One complete [`TrapFrame`] immediately below
/// `kernel_sp` must be mapped writable memory in the current thread's guarded
/// kernel-stack slot. The entry sequence disables supervisor interrupts before
/// changing the `sscratch` stack-switch contract.
#[unsafe(naked)]
pub unsafe extern "C" fn enter_user(_entry: Va, _user_sp: Va, _kernel_sp: Va) -> ! {
    macro_rules! zero_regs {
        ($($reg:ident),+ $(,)?) => {
            $crate::asm!(@asm_lines(
                $(("li ", stringify!($reg), ", 0")),+
            ))
        };
    }

    naked_asm!(
        "csrw sepc, a0",
        "li t0, {clear}",
        "csrc sstatus, t0",
        // Reserve a known-mapped scratch area before entering U-mode. Trap
        // entry uses it before touching the prospective TrapFrame.
        "addi a2, a2, -{entry_scratch_size}",
        // User mode owns tp. Preserve the current hart's PerCore pointer for
        // the next U-mode trap before clearing the user register bank.
        "sd tp, {entry_kernel_tp}(a2)",
        "csrw sscratch, a2",
        "li t0, {user_status}",
        "csrs sstatus, t0",
        "mv sp, a1",
        zero_regs!(
            ra, gp, tp,
            a0, a1, a2, a3, a4, a5, a6, a7,
            t0, t1, t2, t3, t4, t5, t6,
            s0, s1, s2, s3, s4, s5, s6, s7, s8, s9, s10, s11,
        ),
        "sret",
        clear = const Sstatus::SPP.bits() | Sstatus::SIE.bits() | Sstatus::FS.bits(),
        user_status = const Sstatus::SPIE.bits() | Sstatus::FS_INITIAL.bits(),
        entry_scratch_size = const TRAP_ENTRY_SCRATCH_SIZE,
        entry_kernel_tp = const ENTRY_KERNEL_TP,
    )
}

/// Supervisor trap vector entry.
///
/// # Safety
///
/// Hardware must enter this function through `stvec`. For S-mode traps `sp`
/// must identify the current thread's guarded kernel-stack slot, `sscratch`
/// must be 0, and `tp` must point to the current hart's [`PerCore`]. For U-mode
/// traps `sscratch` holds the mapped trap-entry scratch anchor installed by
/// `enter_user`.
/// It is naked because it saves the interrupted register state itself before
/// calling Rust code.
///
/// The trap handler must not set SIE in a U-mode frame's `sstatus` (hardware
/// saves it as 0). The U-mode return path briefly parks the user sp in
/// `sscratch` before the final swap; an interrupt taken in that window would
/// mistake the user sp for a kernel stack.
#[unsafe(naked)]
pub unsafe extern "C" fn _trap_entry() -> ! {
    // This is the first code executed after the CPU vectors to `stvec`.
    // Keep this function naked so Rust does not emit a prologue before the
    // interrupted context has been saved.
    //
    // `t0`/`t1` are scratch after their TrapFrame slots are filled, and on
    // return they are restored only after the S/U epilogue no longer needs
    // them as temporaries.
    macro_rules! trap_entry_asm {
        (
            scratch: [$($scratch:ident),+ $(,)?],
            saved: [$($reg:ident),+ $(,)?] $(,)?
        ) => {
            naked_asm!(
                $crate::asm!(@asm_lines(
                    // U-mode receives its mapped entry-scratch anchor and
                    // parks user sp in sscratch. S-mode receives zero and
                    // parks its live kernel sp in sscratch.
                    "csrrw sp, sscratch, sp",
                    "bnez sp, 5f",

                    // S-mode: test the prospective frame address without
                    // clobbering a GPR. The interrupted sp is in sscratch.
                    // Each 64 KiB slot maps page indexes 1..=4 only.
                    "csrr sp, sscratch",
                    "addi sp, sp, -{frame_size}",
                    "srli sp, sp, {page_shift}",
                    "andi sp, sp, {slot_page_mask}",
                    "addi sp, sp, -{guard_pages}",
                    "sltiu sp, sp, {stack_pages}",
                    "beqz sp, 8f",

                    // Restore the interrupted kernel sp and reserve its frame.
                    "csrr sp, sscratch",
                    "csrw sscratch, zero",
                    "addi sp, sp, -{frame_size}",
                    $(
                        ("sd ", stringify!($reg), ", {", stringify!($reg), "}(sp)"),
                    )+
                    $(
                        ("sd ", stringify!($scratch), ", {", stringify!($scratch), "}(sp)"),
                    )+
                    "sd tp, {tp}(sp)",
                    "addi t0, sp, {frame_size}",
                    "sd t0, {saved_sp}(sp)",
                    "j 1f",

                    // U-mode starts from an empty kernel stack whose capacity
                    // was asserted before enter_user, so build its frame
                    // directly without a dynamic guard check.
                    "5:",
                    "addi sp, sp, -{entry_scratch_offset}",
                    $(
                        ("sd ", stringify!($reg), ", {", stringify!($reg), "}(sp)"),
                    )+
                    "sd t0, {t0}(sp)",
                    "sd t1, {t1}(sp)",
                    "sd tp, {tp}(sp)",
                    "csrr t0, sscratch",
                    "sd t0, {saved_sp}(sp)",
                    "csrw sscratch, zero",
                    // Kernel Rust code addresses PerCore through tp. Recover
                    // it before the first possible call into Rust.
                    "ld tp, {frame_entry_kernel_tp}(sp)",

                    "1:",

                    // Read the interrupted status bits recorded by hardware.
                    "csrr t0, sstatus",
                    // Save status so the handler can inspect or edit it.
                    "sd t0, {sstatus}(sp)",
                    // U-mode may use the FPU, but ordinary kernel code must
                    // fault on every FP instruction. Preserve the user's FS
                    // bits in the frame and run the handler with FS=Off.
                    "li t1, {fs}",
                    "csrc sstatus, t1",
                    // Read the return PC recorded by hardware on trap entry.
                    "csrr t0, sepc",
                    // Save the return PC into TrapFrame.sepc.
                    "sd t0, {sepc}(sp)",
                    // Read the trap cause: interrupt bit plus cause code.
                    "csrr t0, scause",
                    // Save the trap cause into TrapFrame.scause.
                    "sd t0, {scause}(sp)",
                    // Read the trap-specific value, such as a faulting address.
                    "csrr t0, stval",
                    // Save that trap-specific value into TrapFrame.stval.
                    "sd t0, {stval}(sp)",

                    // Pass &mut TrapFrame as the first C ABI argument.
                    "mv a0, sp",
                    // Dispatch to Rust while the full interrupted context is saved.
                    "call {handler}",

                    // Reload the possibly edited return PC from the frame.
                    "ld t0, {sepc}(sp)",
                    // Restore the return PC used by sret.
                    "csrw sepc, t0",
                    // Reload the possibly edited status from the frame.
                    "ld t0, {sstatus}(sp)",
                    // Restore the status used by sret.
                    "csrw sstatus, t0",
                    // Keep SPP in t1 while restoring the other saved registers.
                    "andi t1, t0, {spp}",

                    $(
                        // Restore one general-purpose register from TrapFrame.regs.
                        ("ld ", stringify!($reg), ", {", stringify!($reg), "}(sp)"),
                    )+

                    "bnez t1, 4f",
                    // U-mode return: park user sp in sscratch, restore scratch
                    // regs, then swap so sscratch holds the kernel sp again.
                    // Safe only because the sstatus restored above has SIE=0
                    // (see the SIE contract in the function doc): no interrupt
                    // can hit while sscratch holds the user sp.
                    // Refresh the saved kernel tp in case the thread migrated
                    // since its previous return to user mode.
                    "sd tp, {frame_entry_kernel_tp}(sp)",
                    "ld tp, {tp}(sp)",
                    "ld t0, {saved_sp}(sp)",
                    "csrw sscratch, t0",
                    $(
                        ("ld ", stringify!($scratch), ", {", stringify!($scratch), "}(sp)"),
                    )+
                    "addi sp, sp, {entry_scratch_offset}",
                    "csrrw sp, sscratch, sp",
                    "sret",

                    "4:",
                    // S-mode return: sscratch stays clear. `tp` is hart-local,
                    // not thread-local, so keep the value of the hart on which
                    // this kernel context resumed. Restoring the interrupted
                    // value would point at the old hart after migration.
                    $(
                        ("ld ", stringify!($scratch), ", {", stringify!($scratch), "}(sp)"),
                    )+
                    "ld sp, {saved_sp}(sp)",
                    "sret",

                    // A normal frame would land in a guard/hole. Switch to this
                    // hart's private panic stack before touching memory. stvec
                    // is installed only after tp points to an initialized PerCore.
                    "8:",
                    "ld sp, {per_core_panic_stack}(tp)",
                    "addi sp, sp, 2047",
                    "addi sp, sp, 2047",
                    "addi sp, sp, 2",
                    "addi sp, sp, -{frame_size}",
                    $(
                        ("sd ", stringify!($reg), ", {", stringify!($reg), "}(sp)"),
                    )+
                    $(
                        ("sd ", stringify!($scratch), ", {", stringify!($scratch), "}(sp)"),
                    )+
                    "sd tp, {tp}(sp)",
                    "csrr t0, sscratch",
                    "sd t0, {saved_sp}(sp)",
                    "csrr t0, sstatus",
                    "sd t0, {sstatus}(sp)",
                    "li t1, {fs}",
                    "csrc sstatus, t1",
                    "csrr t0, sepc",
                    "sd t0, {sepc}(sp)",
                    "csrr t0, scause",
                    "sd t0, {scause}(sp)",
                    "csrr t0, stval",
                    "sd t0, {stval}(sp)",
                    "csrw sscratch, zero",
                    "mv a0, sp",
                    "tail {overflow_handler}",

                )),
                frame_size = const size_of::<TrapFrame>(),
                entry_scratch_offset = const offset_of!(TrapFrame, entry_scratch),
                saved_sp = const offset_of!(TrapFrame, regs.sp),
                tp = const offset_of!(TrapFrame, regs.tp),
                frame_entry_kernel_tp = const offset_of!(TrapFrame, entry_scratch) + ENTRY_KERNEL_TP,
                page_shift = const PAGE_SIZE.get().trailing_zeros(),
                slot_page_mask = const KERNEL_STACK_SLOT_SIZE / PAGE_SIZE.get() - 1,
                guard_pages = const KERNEL_STACK_GUARD_SIZE / PAGE_SIZE.get(),
                stack_pages = const STACK_SIZE.get() / PAGE_SIZE.get(),
                spp = const Sstatus::SPP.bits(),
                fs = const Sstatus::FS.bits(),
                $(
                    $reg = const offset_of!(TrapFrame, regs.$reg),
                )+
                $(
                    $scratch = const offset_of!(TrapFrame, regs.$scratch),
                )+
                sepc = const offset_of!(TrapFrame, sepc),
                sstatus = const offset_of!(TrapFrame, sstatus),
                scause = const offset_of!(TrapFrame, scause),
                stval = const offset_of!(TrapFrame, stval),
                handler = sym _trap_handler,
                per_core_panic_stack = const PerCore::PANIC_STACK_OFFSET,
                overflow_handler = sym crate::panic::kernel_stack_overflow,
            )
        };
    }

    trap_entry_asm!(
        scratch: [t0, t1],
        saved: [
            ra, gp,
            a0, a1, a2, a3, a4, a5, a6, a7,
            t2, t3, t4, t5, t6,
            s0, s1, s2, s3, s4, s5, s6, s7, s8, s9, s10, s11,
        ],
    )
}

#[repr(C, align(16))]
struct TrapEntryScratch {
    kernel_tp: usize,
}

#[repr(C, align(16))]
pub struct TrapFrame {
    pub regs: GeneralRegs,
    pub sepc: Va,
    pub sstatus: Sstatus,
    pub scause: Scause,
    pub stval: usize,
    entry_scratch: TrapEntryScratch,
}

const _: () = assert!(size_of::<TrapFrame>() <= STACK_SIZE.get());
const _: () = assert!(
    offset_of!(TrapFrame, entry_scratch) + TRAP_ENTRY_SCRATCH_SIZE == size_of::<TrapFrame>()
);

impl TrapFrame {
    pub fn cause(&self) -> TrapCause {
        if self.scause.is_interrupt() {
            TrapCause::Interrupt(Interrupt::new(self.scause.code()))
        } else {
            TrapCause::Exception(Exception::new(self.scause.code(), self.stval))
        }
    }
}

extern "C" fn _trap_handler(frame: &mut TrapFrame) {
    match frame.cause() {
        TrapCause::Exception(exception) => handle_exception(frame, exception),
        TrapCause::Interrupt(Interrupt::SupervisorTimer) => handle_timer(),
        TrapCause::Interrupt(interrupt) => panic!(
            "unhandled interrupt: {:?}, sepc={}, stval={:#x}",
            interrupt, frame.sepc, frame.stval
        ),
    }
}
