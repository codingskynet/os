//! Shared RISC-V runtime code and target selection.

pub mod paging;

#[cfg(target_arch = "riscv32")]
pub mod rv32;
#[cfg(target_arch = "riscv64")]
pub mod rv64;

pub use native::*;
#[cfg(target_arch = "riscv32")]
use rv32 as native;
#[cfg(target_arch = "riscv64")]
use rv64 as native;
