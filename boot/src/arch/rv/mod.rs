//! Shared RISC-V boot code and target selection.

pub mod paging;

#[cfg(target_arch = "riscv32")]
mod rv32;
#[cfg(target_arch = "riscv64")]
mod rv64;

#[cfg(target_arch = "riscv32")]
use rv32 as native;
#[cfg(target_arch = "riscv64")]
use rv64 as native;
