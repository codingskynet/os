//! Device-facing interfaces used by the kernel.
//!
//! This module keeps hardware description parsing and concrete device drivers
//! behind small abstractions so the rest of the runtime does not depend on
//! firmware-specific data formats or MMIO register layouts.

pub mod dt;
pub mod uart;
