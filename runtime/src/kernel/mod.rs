//! Core kernel services.
//!
//! This layer owns scheduling, kernel threads, console output, timekeeping, and
//! synchronization primitives used by the rest of the runtime.

pub mod clock;
pub mod console;
pub mod file;
pub mod init;
pub mod per_core;
pub mod scheduler;
pub mod sync;
pub mod syscall;
pub mod thread;
pub mod timer;
