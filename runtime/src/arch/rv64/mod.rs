//! RISC-V 64-bit architecture support.
//!
//! This module contains the supervisor-mode pieces required by the runtime:
//! CSR wrappers, Sv39 paging, trap dispatch, timer interrupts, and kernel
//! thread context switching.

pub mod asm;
pub mod consts;
pub mod page_table;
pub mod regs;
pub mod switch;
pub mod timer;
pub mod trap;

use bitflags::bitflags;

use crate::args_enum;
use crate::mm::addr::Va;

macro_rules! riscv_privileged_isa_version {
    () => {
        "v20260120"
    };
}

macro_rules! riscv_supervisor_doc_url {
    ($fragment:literal) => {
        concat!(
            "https://docs.riscv.org/reference/isa/",
            riscv_privileged_isa_version!(),
            "/priv/supervisor.html",
            $fragment,
        )
    };
}

bitflags! {
    /// Decoded bits from the RISC-V supervisor status register, `sstatus`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[repr(transparent)]
    pub struct Sstatus: usize {
        /// Supervisor interrupt enable for normal S-mode execution.
        const SIE = 1 << 1;

        /// Previous supervisor interrupt enable restored by `sret`.
        const SPIE = 1 << 5;

        /// Previous privilege mode restored by `sret`.
        const SPP = 1 << 8;

        /// Mask for the two-bit floating-point state field.
        const FS = 0b11 << 13;

        /// Floating-point state is initial and has not yet been used.
        const FS_INITIAL = 0b01 << 13;

        /// Floating-point state is clean relative to its saved context.
        const FS_CLEAN = 0b10 << 13;

        /// Floating-point state may have changed since its last save.
        const FS_DIRTY = 0b11 << 13;

        /// Permit S-mode loads/stores to pages marked user-accessible.
        const SUM = 1 << 18;

        /// Make executable pages readable by S-mode loads.
        const MXR = 1 << 19;

        /// Summary dirty bit for extension state such as FP or vector state.
        const SD = 1 << (usize::BITS as usize - 1);
    }
}

/// A decoded RISC-V supervisor trap cause register, `scause`.
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrapCause {
    Exception(Exception),
    Interrupt(Interrupt),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PageFaultReason {
    Instruction(Va),
    LoadPage(Va),
    StorePage(Va),
}

impl PageFaultReason {
    pub const fn addr(self) -> Va {
        match self {
            Self::Instruction(addr) | Self::LoadPage(addr) | Self::StorePage(addr) => addr,
        }
    }
}

args_enum! {
    #[derive(Clone, Copy, Debug, PartialEq)]
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
    pub enum Exception(usize, stval: usize) {
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

args_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    pub enum Interrupt(usize) {
        1 => SupervisorSoftware,
        5 => SupervisorTimer,
        9 => SupervisorExternal,
    }
}
