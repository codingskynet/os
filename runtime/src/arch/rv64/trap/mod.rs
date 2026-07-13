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
//!      Supervisor scratch CSR. Software uses it as a stack-switch slot so
//!      trap frames always live on a kernel stack. While S-mode runs it is 0
//!      and `sp` is the live kernel stack; while U-mode runs it holds the
//!      kernel stack pointer to resume on and `sp` is the user stack. On
//!      U-mode trap entry the user `sp` moves into `TrapFrame.regs.sp` and
//!      `sscratch` is cleared back to 0 so a nested S-mode trap can take the
//!      normal kernel-stack path. Returning to U-mode writes the post-frame
//!      kernel `sp` back into `sscratch` before restoring the saved user `sp`.
//!
//! `sepc`, `sstatus`, `scause`, and `stval` are part of `TrapFrame` rather than
//! `GeneralRegs` because they are not architectural integer registers; they
//! are control/status state saved by hardware on trap entry and consumed by
//! `sret` on trap return. `sscratch` stays outside the frame and is maintained
//! by the trap entry/return path around that stack switch.

mod exception;

use core::arch::naked_asm;
use core::mem::{offset_of, size_of};

use bitflags::bitflags;

use super::regs::GeneralRegs;
use crate::arch::timer::handle_timer;
use crate::arch::trap::exception::{PageFaultReason, handle_exception};
use crate::mm::addr::Va;
use crate::{args_enum_from_usize, asm};

pub fn init() {
    unsafe {
        asm!(
            "csrw stvec, {entry}",
            "csrw sscratch, zero",
            entry = in(reg) _trap_entry as *const u8 as usize,
            options(nostack, preserves_flags),
        );
    }
}

/// Supervisor trap vector entry.
///
/// # Safety
///
/// Hardware must enter this function through `stvec`. For S-mode traps the
/// current `sp` must already be a valid kernel stack with `sscratch = 0`. For
/// U-mode traps `sscratch` must hold the kernel stack pointer to switch onto.
/// It is naked because it saves the interrupted register state itself before
/// calling Rust code.
///
/// The trap handler must not set SIE in a U-mode frame's `sstatus` (hardware
/// saves it as 0). The U-mode return path briefly parks the user sp in
/// `sscratch` before the final swap; an interrupt taken in that window would
/// mistake the user sp for a kernel stack.
#[rustfmt::skip]
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
                    // Swap sp with sscratch.
                    // U-mode: sp <- kernel stack, sscratch <- user sp
                    // S-mode: sp <- 0,            sscratch <- kernel sp
                    "csrrw sp, sscratch, sp",
                    "bnez sp, 1f",
                    // S-mode: put the kernel sp back and leave sscratch = 0.
                    "csrrw sp, sscratch, sp",
                    "1:",

                    // Reserve a TrapFrame on the kernel stack.
                    "addi sp, sp, -{frame_size}",

                    $(
                        // Save one general-purpose register into TrapFrame.regs.
                        ("sd ", stringify!($reg), ", {", stringify!($reg), "}(sp)"),
                    )+
                    $(
                        ("sd ", stringify!($scratch), ", {", stringify!($scratch), "}(sp)"),
                    )+

                    // Read the interrupted status bits recorded by hardware.
                    "csrr t0, sstatus",
                    // Save status so the handler can inspect or edit it.
                    "sd t0, {sstatus}(sp)",
                    // SPP distinguishes the interrupted privilege for sp save.
                    "andi t1, t0, {spp}",
                    "bnez t1, 2f",
                    // U-mode: user sp is still parked in sscratch.
                    "csrr t0, sscratch",
                    "sd t0, {saved_sp}(sp)",
                    // Clear sscratch so nested S-mode traps keep the contract.
                    "csrw sscratch, zero",
                    "j 3f",
                    "2:",
                    // S-mode: reconstruct the original kernel sp.
                    "addi t0, sp, {frame_size}",
                    "sd t0, {saved_sp}(sp)",
                    "3:",

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
                    // S-mode return: sscratch stays 0; restore scratch regs and sp.
                    $(
                        ("ld ", stringify!($scratch), ", {", stringify!($scratch), "}(sp)"),
                    )+
                    "ld sp, {saved_sp}(sp)",
                    "sret",
                )),
                frame_size = const size_of::<TrapFrame>(),
                saved_sp = const offset_of!(TrapFrame, regs.sp),
                spp = const Sstatus::SPP.bits(),
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

bitflags! {
    /// Decoded bits from the RISC-V supervisor status register, `sstatus`.
    ///
    /// `sstatus` is saved on trap entry so the handler can inspect the
    /// interrupted privilege/interrupt state and optionally edit the state that
    /// `sret` will restore.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[repr(transparent)]
    pub struct Sstatus: usize {
        /// Supervisor interrupt enable for normal S-mode execution.
        ///
        /// Hardware clears SIE on trap entry. On `sret`, SIE is restored from
        /// SPIE.
        const SIE = 1 << 1;

        /// Previous supervisor interrupt enable.
        ///
        /// Hardware copies the pre-trap SIE value here on trap entry. `sret`
        /// copies SPIE back to SIE.
        const SPIE = 1 << 5;

        /// Previous privilege mode.
        ///
        /// Clear means the trap came from U-mode; set means it came from
        /// S-mode. `sret` uses this bit to choose the return privilege.
        const SPP = 1 << 8;

        /// Permit S-mode loads/stores to pages marked user-accessible.
        const SUM = 1 << 18;

        /// Make executable pages readable by S-mode loads.
        const MXR = 1 << 19;

        /// Summary dirty bit for extension state such as floating point or
        /// vector state.
        const SD = 1 << (usize::BITS as usize - 1);
    }
}

/// A decoded RISC-V supervisor trap cause register, `scause`.
///
/// The most-significant bit reports whether the trap is an interrupt. The
/// remaining bits hold the exception or interrupt cause code defined by the
/// privileged architecture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Scause(usize);

impl Scause {
    const INTERRUPT_BIT: usize = 1 << (usize::BITS as usize - 1);

    pub const fn is_interrupt(self) -> bool {
        self.0 & Self::INTERRUPT_BIT != 0
    }

    pub const fn code(self) -> usize {
        self.0 & !Self::INTERRUPT_BIT
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrapCause {
    Exception(Exception),
    Interrupt(Interrupt),
}

args_enum_from_usize! {
    #[doc = concat!(
        "Standard synchronous exception codes reported in `scause`.\n\n",
        "The numeric codes come from RISC-V Privileged ISA ",
        riscv_privileged_isa_version!(),
        "'s [`scause` register] definition. Some exception variants also ",
        "carry a decoded `stval` payload. That payload interpretation follows ",
        "the [`stval` register] description: address/page/access faults use it ",
        "as a trap-related virtual address when provided, illegal-instruction ",
        "traps may use it for instruction bits, and environment calls do not ",
        "use it.\n\n",
        "[`scause` register]: ",
        riscv_supervisor_doc_url!("#scause"),
        "\n",
        "[`stval` register]: ",
        riscv_supervisor_doc_url!("#12-1-1-9-supervisor-trap-value-stval-register"),
    )]
    pub enum Exception(stval) {
        0 => InstructionAddressMisaligned(Va = Va::new(stval)),
        1 => InstructionAccessFault(Va = Va::new(stval)),
        2 => IllegalInstruction(usize = stval),
        3 => Breakpoint(Va = Va::new(stval)),
        4 => LoadAddressMisaligned(Va = Va::new(stval)),
        5 => LoadAccessFault(Va = Va::new(stval)),
        6 => StoreAddressMisaligned(Va = Va::new(stval)),
        7 => StoreAccessFault(Va = Va::new(stval)),
        8 => EnvironmentCallFromUMode,
        9 => EnvironmentCallFromSMode,
        PageFault(PageFaultReason) {
            12 => PageFaultReason::Instruction(Va::new(stval)),
            13 => PageFaultReason::LoadPage(Va::new(stval)),
            15 => PageFaultReason::StorePage(Va::new(stval)),
        },
    }
}

args_enum_from_usize! {
    #[doc = concat!(
        "Standard supervisor-level interrupt codes reported in `scause`.\n\n",
        "The numeric codes come from RISC-V Privileged ISA ",
        riscv_privileged_isa_version!(),
        "'s [`scause` register] definition. Interrupts are asynchronous ",
        "events, so this enum does not decode `stval`; timer, software, and ",
        "external interrupt handlers should inspect the relevant ",
        "interrupt-pending state or interrupt controller instead.\n\n",
        "[`scause` register]: ",
        riscv_supervisor_doc_url!("#scause"),
    )]
    pub enum Interrupt() {
        1 => SupervisorSoftware,
        5 => SupervisorTimer,
        9 => SupervisorExternal,
    }
}
