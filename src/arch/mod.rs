//! Architecture abstraction layer.
//!
//! Each supported ISA lives under its own sub-directory (`rv64/`, `aarch64/`, …)
//! and implements the traits defined here so the rest of the kernel can be
//! architecture-agnostic.

#[cfg(feature = "rv64")]
pub mod rv64;
