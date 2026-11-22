//! Select either target's real address constants for host-side conversion tests.
#[path = "../../runtime/src/arch/linker_consts.rs"]
mod linker_consts;

#[cfg(feature = "rv32-layout")]
#[path = "../../runtime/src/arch/rv/rv32/consts.rs"]
pub mod consts;
#[cfg(not(feature = "rv32-layout"))]
#[path = "../../runtime/src/arch/rv/rv64/consts.rs"]
pub mod consts;

// Supply the word normally emitted by the board's linker script.
#[unsafe(export_name = "_kernel_vma_offset")]
static TEST_IMAGE_OFFSET: usize = consts::KERNEL_VMA_BASE - 0x1000_0000;
