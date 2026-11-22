//! Device-facing interfaces used by the kernel.
//!
//! Hardware description parsing and service interfaces live here. Concrete
//! implementations live in the driver crate and are installed by boot, keeping
//! the kernel independent of board names and MMIO register layouts.

pub mod console;
pub mod dt;
pub mod interrupt;
pub mod timer;
pub mod uart;

pub mod discovery;
pub mod routes;
pub use routes::Routes;
