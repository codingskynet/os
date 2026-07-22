//! RISC-V 64-bit architecture support.
//!
//! This module contains the supervisor-mode pieces required by the runtime:
//! CSR wrappers, Sv39 paging, trap dispatch, timer interrupts, and kernel
//! thread context switching.

pub mod asm;
pub mod consts;
pub mod external;
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
    #[doc = concat!(
        "Fields used by the runtime from the RISC-V supervisor status register, ",
        "`sstatus`.\n\n",
        "`sstatus` records the current supervisor execution state and controls ",
        "trap handling, floating-point context, and supervisor memory accesses. ",
        "This bitflag type names only the fields used by this runtime; unnamed ",
        "register bits are not necessarily zero.\n\n",
        "## Represented RV64 bit layout\n\n",
        "```text\n",
        "+---------+------------+---------------------------------------------+\n",
        "| Bit(s)  | Field      | Constant(s)                                 |\n",
        "+---------+------------+---------------------------------------------+\n",
        "| 63      | SD         | SD                                          |\n",
        "| 62:20   | --         | not represented                             |\n",
        "| 19      | MXR        | MXR                                         |\n",
        "| 18      | SUM        | SUM                                         |\n",
        "| 17:15   | --         | not represented                             |\n",
        "| 14:13   | FS[1:0]    | FS, FS_INITIAL, FS_CLEAN, FS_DIRTY          |\n",
        "| 12:9    | --         | not represented                             |\n",
        "| 8       | SPP        | SPP                                         |\n",
        "| 7:6     | --         | not represented                             |\n",
        "| 5       | SPIE       | SPIE                                        |\n",
        "| 4:2     | --         | not represented                             |\n",
        "| 1       | SIE        | SIE                                         |\n",
        "| 0       | --         | not represented                             |\n",
        "+---------+------------+---------------------------------------------+\n",
        "```\n\n",
        "`FS` is a two-bit field, not a pair of independent Boolean flags. Its ",
        "encodings are:\n\n",
        "```text\n",
        "+----------+---------+------------+\n",
        "| FS[1:0]  | State   | Constant   |\n",
        "+----------+---------+------------+\n",
        "| 00       | Off     | --         |\n",
        "| 01       | Initial | FS_INITIAL |\n",
        "| 10       | Clean   | FS_CLEAN   |\n",
        "| 11       | Dirty   | FS_DIRTY   |\n",
        "+----------+---------+------------+\n",
        "```\n\n",
        "The encoded `FS_*` values overlap. In particular, `FS_DIRTY` contains ",
        "the bit pattern of `FS_INITIAL`, and `FS` has the same bits as ",
        "`FS_DIRTY`. Therefore, decode a value such as Initial with ",
        "`(status & Sstatus::FS) == Sstatus::FS_INITIAL` rather than ",
        "`status.contains(Sstatus::FS_INITIAL)`, and replace the field by ",
        "clearing `FS` before inserting a new encoded value.\n\n",
        "The field positions and behavior follow RISC-V Privileged ISA ",
        riscv_privileged_isa_version!(),
        "'s [`sstatus` register] definition.\n\n",
        "[`sstatus` register]: ",
        riscv_supervisor_doc_url!("#sstatus"),
    )]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[repr(transparent)]
    pub struct Sstatus: usize {
        /// Enables supervisor-level interrupts while executing in S-mode.
        ///
        /// If clear, supervisor interrupts are masked in S-mode. In U-mode this
        /// bit is ignored and supervisor-level interrupts remain globally
        /// enabled; individual sources are still controlled through `sie`.
        const SIE = 1 << 1;

        /// Supervisor interrupt-enable state saved across a trap.
        ///
        /// Trap entry copies `SIE` into `SPIE` and clears `SIE`. `sret` copies
        /// `SPIE` back into `SIE`, then sets `SPIE`.
        const SPIE = 1 << 5;

        /// Privilege mode that was active before the most recent S-mode trap.
        ///
        /// Clear denotes U-mode and set denotes S-mode. `sret` returns to that
        /// mode and then clears `SPP`.
        const SPP = 1 << 8;

        /// Mask covering the complete two-bit `FS[1:0]` field.
        ///
        /// Use this to clear or extract the field. It is not a distinct state;
        /// its bit pattern is identical to [`Self::FS_DIRTY`].
        const FS = 0b11 << 13;

        /// `FS=Initial`: floating-point state has its initial value.
        const FS_INITIAL = 0b01 << 13;

        /// `FS=Clean`: floating-point state matches its saved context.
        const FS_CLEAN = 0b10 << 13;

        /// `FS=Dirty`: floating-point state may differ from its saved context.
        const FS_DIRTY = 0b11 << 13;

        /// Permits S-mode loads and stores to pages marked user-accessible.
        ///
        /// This does not permit S-mode instruction fetches from user pages and
        /// has no effect in U-mode or when page-based translation is disabled.
        const SUM = 1 << 18;

        /// Allows loads from executable-only pages when translation is active.
        ///
        /// If clear, a load requires the page's read permission. If set, either
        /// read or execute permission is sufficient. Store and fetch permissions
        /// are unaffected.
        const MXR = 1 << 19;

        /// Read-only summary that some extension state is dirty.
        ///
        /// The hardware sets `SD` when a tracked extension-status field such as
        /// `FS`, `VS`, or `XS` is Dirty, allowing context-switch code to test one
        /// bit before inspecting the individual fields.
        const SD = 1 << (usize::BITS as usize - 1);
    }
}

bitflags! {
    #[doc = concat!(
        "Interrupt sources used by the runtime from the RISC-V supervisor ",
        "interrupt-enable register, `sie`.\n\n",
        "Bit `i` enables the supervisor interrupt whose `scause` code is `i`. ",
        "An enabled source traps to S-mode only while the matching bit in `sip` ",
        "is pending and supervisor interrupts are globally enabled by ",
        "`sstatus.SIE` (or execution is in a less-privileged mode). This type ",
        "names only the timer and external sources used by this runtime.\n\n",
        "## Represented RV64 bit layout\n\n",
        "```text\n",
        "+---------+------------+----------+-----------------------------+\n",
        "| Bit(s)  | Field      | Constant | scause interrupt code       |\n",
        "+---------+------------+----------+-----------------------------+\n",
        "| 63:10   | --         | --       | not represented             |\n",
        "| 9       | SEIE       | SEIE     | 9 (SupervisorExternal)      |\n",
        "| 8:6     | --         | --       | not represented             |\n",
        "| 5       | STIE       | STIE     | 5 (SupervisorTimer)         |\n",
        "| 4:0     | --         | --       | not represented             |\n",
        "+---------+------------+----------+-----------------------------+\n",
        "```\n\n",
        "Unnamed positions include other standard interrupt enables, reserved ",
        "positions, and platform-defined positions; they are not necessarily ",
        "zero in the hardware register. Implemented `sie` fields are WARL, and ",
        "an unsupported interrupt source reads as zero.\n\n",
        "The bit positions and behavior follow RISC-V Privileged ISA ",
        riscv_privileged_isa_version!(),
        "'s [`sie` register] definition.\n\n",
        "[`sie` register]: ",
        riscv_supervisor_doc_url!("#sie"),
    )]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[repr(transparent)]
    pub struct Sie: usize {
        /// Enables supervisor timer interrupts (`scause` interrupt code 5).
        ///
        /// The source is pending when `sip.STIP` is set. Depending on the
        /// platform, the execution environment or `stimecmp` controls STIP.
        const STIE = 1 << 5;

        /// Enables supervisor external interrupts (`scause` interrupt code 9).
        ///
        /// The source is pending when `sip.SEIP` is set, normally in response to
        /// a platform interrupt controller reporting an external interrupt.
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
