//! Host-side test harness for architecture-independent runtime and driver modules.
//!
//! The real `runtime` crate stays kernel-facing. This crate reuses the modules
//! that are safe to test on the host through explicit path imports.

#![cfg_attr(not(test), no_std)]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod dev;

#[path = "../../runtime/src/kernel/sync/lazy_lock.rs"]
pub mod lazy_lock;

#[path = "../../runtime/src/kernel/console/buffer.rs"]
pub mod console_buffer;

#[path = "../../runtime/src/util/mod.rs"]
pub mod util;

// Driver parsers share the runtime interfaces without compiling kernel assembly.
extern crate self as runtime;
#[path = "../../runtime/src/dev/discovery.rs"]
pub mod discovery;
#[path = "../../driver/src/interrupt/plic/registers.rs"]
pub mod plic;

#[path = "../../runtime/src/dev/routes.rs"]
pub mod irq_routes;

#[path = "../../runtime/src/mm/mmio/layout.rs"]
pub mod mmio_layout;

#[path = "../../runtime/src/mm/addr.rs"]
pub mod addr;
pub mod arch;

#[cfg(test)]
#[path = "../../board/driver_gen.rs"]
mod driver_gen;
