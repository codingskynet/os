//! RISC-V 32-bit architecture support.
//!
//! This module contains the supervisor-mode pieces required by the runtime:
//! CSR wrappers, Sv32 paging, trap dispatch, timer interrupts, and kernel
//! thread context switching.

pub mod asm;
pub mod consts;
pub mod page_table;
pub mod regs;
pub mod switch;
pub mod trap;

use bitflags::bitflags;

use crate::args_enum;
use crate::mm::addr::Va;

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[repr(transparent)]
    pub struct Sstatus: usize {
        const SIE = 1 << 1;

        const SPIE = 1 << 5;

        const SPP = 1 << 8;

        const FS = 0b11 << 13;

        const FS_INITIAL = 0b01 << 13;

        const FS_CLEAN = 0b10 << 13;

        const FS_DIRTY = 0b11 << 13;

        const SUM = 1 << 18;

        const MXR = 1 << 19;

        const SD = 1 << (usize::BITS as usize - 1);
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[repr(transparent)]
    pub struct Sie: usize {
        const STIE = 1 << 5;

        const SEIE = 1 << 9;
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
    pub enum Interrupt(usize) {
        1 => SupervisorSoftware,
        5 => SupervisorTimer,
        9 => SupervisorExternal,
    }
}
