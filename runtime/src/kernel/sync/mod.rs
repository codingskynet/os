//! Synchronization primitives for early kernel code.

mod lazy_lock;
mod spinlock;
pub use lazy_lock::LazyLock;
pub use spinlock::SpinLock;
