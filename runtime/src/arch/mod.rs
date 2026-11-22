//! Architecture abstraction layer.
//!
//! The selected architecture supplies the low-level interfaces used by the runtime.

pub mod interrupt;
pub mod memory;
pub mod region;

#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
pub mod rv;
#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
pub use rv::*;
