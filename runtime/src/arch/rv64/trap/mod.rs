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
use crate::mm::addr::Va;

// A U-mode trap needs one known-mapped location before it can borrow a GPR for
// the guard calculation. This area is reserved at the top of the kernel stack.
pub const TRAP_ENTRY_SCRATCH_SIZE: usize = 2 * size_of::<usize>();
const ENTRY_T0: usize = 0;
const ENTRY_USER_SP: usize = size_of::<usize>();

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
/// active address space. The `TRAP_ENTRY_SCRATCH_SIZE` bytes immediately below
/// `kernel_sp` must be mapped writable memory in the current thread's guarded
/// kernel-stack slot. The selected frame itself may cross the lower boundary;
/// trap entry detects that case before touching it. The entry sequence disables
/// supervisor interrupts before changing the `sscratch` stack-switch contract.
#[unsafe(naked)]
pub(crate) unsafe extern "C" fn enter_user(_entry: Va, _user_sp: Va, _kernel_sp: Va) -> ! {
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
    )
}

/// Supervisor trap vector entry.
///
/// # Safety
///
/// Hardware must enter this function through `stvec`. For S-mode traps `sp`
/// must identify the current thread's guarded kernel-stack slot and `sscratch`
/// must be 0. For U-mode traps `sscratch` holds the mapped trap-entry scratch
/// anchor installed by [`enter_user`].
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
                    "addi t0, sp, {frame_size}",
                    "sd t0, {saved_sp}(sp)",
                    "j 1f",

                    // U-mode: preserve t0 and user sp in the known-mapped
                    // scratch area, then clear sscratch. A fault from this
                    // point onward therefore re-enters through the S-mode path.
                    "5:",
                    "sd t0, {entry_t0}(sp)",
                    "csrr t0, sscratch",
                    "sd t0, {entry_user_sp}(sp)",
                    "csrw sscratch, zero",

                    // Validate the selected kernel sp - frame_size before the
                    // first store to that frame.
                    "addi t0, sp, -{frame_size}",
                    "srli t0, t0, {page_shift}",
                    "andi t0, t0, {slot_page_mask}",
                    "addi t0, t0, -{guard_pages}",
                    "sltiu t0, t0, {stack_pages}",
                    "beqz t0, 9f",

                    "addi sp, sp, -{frame_size}",
                    $(
                        ("sd ", stringify!($reg), ", {", stringify!($reg), "}(sp)"),
                    )+
                    // t1 was not borrowed by the check. Recover t0 and user sp
                    // from the scratch area immediately above this frame.
                    "sd t1, {t1}(sp)",
                    "ld t0, {frame_entry_t0}(sp)",
                    "sd t0, {t0}(sp)",
                    "ld t0, {frame_entry_user_sp}(sp)",
                    "sd t0, {saved_sp}(sp)",

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
                    "ld t0, {saved_sp}(sp)",
                    "csrw sscratch, t0",
                    $(
                        ("ld ", stringify!($scratch), ", {", stringify!($scratch), "}(sp)"),
                    )+
                    "addi sp, sp, {frame_size}",
                    "csrrw sp, sscratch, sp",
                    "sret",

                    "4:",
                    // S-mode return: sscratch stays clear.
                    $(
                        ("ld ", stringify!($scratch), ", {", stringify!($scratch), "}(sp)"),
                    )+
                    "ld sp, {saved_sp}(sp)",
                    "sret",

                    // A normal frame would land in a guard/hole. Switch to the
                    // boot hart's 4 KiB panic stack before touching memory.
                    "8:",
                    "la sp, {panic_stack}",
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

                    // U-mode overflow. Keep the entry-scratch address in t0
                    // while switching to the emergency stack; it holds both
                    // values consumed before validation.
                    "9:",
                    "mv t0, sp",
                    "la sp, {panic_stack}",
                    "addi sp, sp, 2047",
                    "addi sp, sp, 2047",
                    "addi sp, sp, 2",
                    "addi sp, sp, -{frame_size}",
                    $(
                        ("sd ", stringify!($reg), ", {", stringify!($reg), "}(sp)"),
                    )+
                    "sd t1, {t1}(sp)",
                    "ld t1, {entry_user_sp}(t0)",
                    "sd t1, {saved_sp}(sp)",
                    "ld t1, {entry_t0}(t0)",
                    "sd t1, {t0}(sp)",
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
                    "mv a0, sp",
                    "tail {overflow_handler}",
                )),
                frame_size = const size_of::<TrapFrame>(),
                saved_sp = const offset_of!(TrapFrame, regs.sp),
                entry_t0 = const ENTRY_T0,
                entry_user_sp = const ENTRY_USER_SP,
                frame_entry_t0 = const size_of::<TrapFrame>() + ENTRY_T0,
                frame_entry_user_sp = const size_of::<TrapFrame>() + ENTRY_USER_SP,
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
                panic_stack = sym crate::panic::PANIC_STACK,
                overflow_handler = sym crate::panic::kernel_stack_overflow,
            )
        };
    }

    trap_entry_asm!(
        scratch: [t0, t1],
        saved: [
            ra, gp, tp,
            a0, a1, a2, a3, a4, a5, a6, a7,
            t2, t3, t4, t5, t6,
            s0, s1, s2, s3, s4, s5, s6, s7, s8, s9, s10, s11,
        ],
    )
}

#[repr(C, align(16))]
pub struct TrapFrame {
    pub regs: GeneralRegs,
    pub sepc: Va,
    pub sstatus: Sstatus,
    pub scause: Scause,
    pub stval: usize,
}

const _: () = assert!(size_of::<TrapFrame>() <= STACK_SIZE.get());
const _: () = assert!(size_of::<TrapFrame>() + TRAP_ENTRY_SCRATCH_SIZE <= STACK_SIZE.get());

impl TrapFrame {
    pub fn cause(&self) -> TrapCause {
        match self.scause.is_interrupt() {
            true => TrapCause::Interrupt(Interrupt::new(self.scause.code())),
            false => TrapCause::Exception(Exception::new(self.scause.code(), self.stval)),
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
