//! RISC-V supervisor trap handling.
//!
//! Trap entry saves general-purpose registers plus the supervisor CSRs that
//! describe why the CPU left normal execution:
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
//! These fields are part of `TrapFrame` rather than `GeneralRegs` because they
//! are not architectural integer registers; they are control/status state saved
//! by hardware on trap entry and consumed by `sret` on trap return.

use core::arch::{asm, naked_asm};
use core::mem::{offset_of, size_of};

use bitflags::bitflags;

use super::regs::GeneralRegs;
use crate::arch::page_fault::handle_page_fault;
use crate::arch::timer::handle_timer;
use crate::mm::addr::Va;

pub fn init() {
    unsafe {
        asm!(
            "csrw stvec, {entry}",
            entry = in(reg) _trap_entry as *const () as usize,
            options(nostack, preserves_flags),
        );
    }
}

#[rustfmt::skip]
#[unsafe(naked)]
pub unsafe extern "C" fn _trap_entry() -> ! {
    // This is the first code executed after the CPU vectors to `stvec`.
    // Keep this function naked so Rust does not emit a prologue before the
    // interrupted context has been saved.
    macro_rules! trap_entry_asm {
        ($($reg:ident),+ $(,)?) => {
            naked_asm!(
                concat!(
                    // Reserve a TrapFrame on the interrupted stack.
                    "addi sp, sp, -{frame_size}\n",

                    $(
                        // Save one general-purpose register into TrapFrame.regs.
                        "sd ", stringify!($reg), ", {", stringify!($reg), "}(sp)\n",
                    )+

                    // Reconstruct the original sp from before the frame allocation.
                    "addi t0, sp, {frame_size}\n",
                    // Save the original interrupted sp into TrapFrame.regs.sp.
                    "sd t0, {saved_sp}(sp)\n",

                    // Read the return PC recorded by hardware on trap entry.
                    "csrr t0, sepc\n",
                    // Save the return PC into TrapFrame.sepc.
                    "sd t0, {sepc}(sp)\n",
                    // Read the interrupted status bits recorded by hardware.
                    "csrr t0, sstatus\n",
                    // Save status so the handler can inspect or edit it.
                    "sd t0, {sstatus}(sp)\n",
                    // Read the trap cause: interrupt bit plus cause code.
                    "csrr t0, scause\n",
                    // Save the trap cause into TrapFrame.scause.
                    "sd t0, {scause}(sp)\n",
                    // Read the trap-specific value, such as a faulting address.
                    "csrr t0, stval\n",
                    // Save that trap-specific value into TrapFrame.stval.
                    "sd t0, {stval}(sp)\n",

                    // Pass &mut TrapFrame as the first C ABI argument.
                    "mv a0, sp\n",
                    // Dispatch to Rust while the full interrupted context is saved.
                    "call {handler}\n",

                    // Reload the possibly edited return PC from the frame.
                    "ld t0, {sepc}(sp)\n",
                    // Restore the return PC used by sret.
                    "csrw sepc, t0\n",
                    // Reload the possibly edited status from the frame.
                    "ld t0, {sstatus}(sp)\n",
                    // Restore the status used by sret.
                    "csrw sstatus, t0\n",

                    $(
                        // Restore one general-purpose register from TrapFrame.regs.
                        "ld ", stringify!($reg), ", {", stringify!($reg), "}(sp)\n",
                    )+

                    // Restore the original sp last; after this, sp no longer points at TrapFrame.
                    "ld sp, {saved_sp}(sp)\n",
                    // Return from the trap to sepc with privilege/status from sstatus.
                    "sret\n",
                ),
                frame_size = const size_of::<TrapFrame>(),
                saved_sp = const offset_of!(TrapFrame, regs.sp),
                $(
                    $reg = const offset_of!(TrapFrame, regs.$reg),
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
        ra, gp, tp,
        a0, a1, a2, a3, a4, a5, a6, a7,
        t0, t1, t2, t3, t4, t5, t6,
        s0, s1, s2, s3, s4, s5, s6, s7, s8, s9, s10, s11,
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
            true => TrapCause::Interrupt(Interrupt::new(self.scause.code(), self.stval)),
            false => TrapCause::Exception(Exception::new(self.scause.code(), self.stval)),
        }
    }
}

extern "C" fn _trap_handler(frame: &mut TrapFrame) {
    match frame.cause() {
        TrapCause::Exception(
            exception @ (Exception::InstructionPageFault(_)
            | Exception::LoadPageFault(_)
            | Exception::StorePageFault(_)),
        ) => handle_page_fault(frame, exception),
        TrapCause::Exception(exception) => panic!(
            "unhandled exception: {:?}, sepc={}, stval={:#x}",
            exception, frame.sepc, frame.stval
        ),
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

macro_rules! cause_enum {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $name:ident ($stval:ident) {
            $(
                $(#[$variant_meta:meta])*
                $code:literal => $variant:ident $(($value_ty:ty $(= $value:expr)?))?,
            )+
        }
    ) => {
        $(#[$enum_meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        $vis enum $name {
            $(
                $(#[$variant_meta])*
                $variant $(($value_ty))?,
            )+
            Unknown(usize),
        }

        impl $name {
            fn new(code: usize, $stval: usize) -> Self {
                let _ = $stval;
                match code {
                    $(
                        $code => Self::$variant $((
                            cause_enum!(@value $stval, $value_ty $(, $value)?)
                        ))?,
                    )+
                    _ => Self::Unknown(code),
                }
            }
        }
    };

    (@value $stval:ident, $value_ty:ty, $value:expr) => {
        $value
    };

    (@value $stval:ident, $value_ty:ty) => {
        <$value_ty>::new($stval)
    };
}

cause_enum! {
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
        12 => InstructionPageFault(Va = Va::new(stval)),
        13 => LoadPageFault(Va = Va::new(stval)),
        15 => StorePageFault(Va = Va::new(stval)),
    }
}

cause_enum! {
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
    pub enum Interrupt(stval) {
        1 => SupervisorSoftware,
        5 => SupervisorTimer,
        9 => SupervisorExternal,
    }
}
