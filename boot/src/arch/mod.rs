//! Architecture-specific boot code.

#[cfg(target_arch = "riscv64")]
pub mod rv64;
#[cfg(target_arch = "riscv64")]
pub use rv64::*;

#[cfg(target_arch = "riscv32")]
pub mod rv32;
#[cfg(target_arch = "riscv32")]
pub use rv32::*;
