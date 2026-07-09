//! Synchronization primitives for early kernel code.

pub mod freezable;
mod spinlock;
pub use spinlock::SpinLock;
