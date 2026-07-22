//! Host-side test harness for architecture-independent runtime modules.
//!
//! The real `runtime` crate stays kernel-facing. This crate reuses the modules
//! that are safe to test on the host through explicit path imports.

#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std;

#[path = "../../runtime/src/dev/mod.rs"]
pub mod dev;

#[path = "../../runtime/src/kernel/sync/lazy_lock.rs"]
pub mod lazy_lock;

#[path = "../../runtime/src/kernel/console/buffer.rs"]
pub mod console_buffer;

#[path = "../../runtime/src/util/mod.rs"]
pub mod util;
