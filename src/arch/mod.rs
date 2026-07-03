//! Architecture abstraction layer.
//!
//! Each supported ISA lives under its own sub-directory (`rv64/`, `aarch64/`, …)
//! and implements the traits defined here so the rest of the kernel can be
//! architecture-agnostic.

pub mod interrupt;

#[cfg(target_arch = "riscv64")]
pub mod rv64;
#[cfg(target_arch = "riscv64")]
pub use rv64::*;
