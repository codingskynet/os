//! Optional smoke tests enabled by crate features.

#[cfg(feature = "smoke-allocator")]
pub mod allocator;

#[cfg(feature = "smoke-initarfs")]
pub mod initarfs;

#[cfg(feature = "smoke-page-fault")]
pub mod page_fault;

#[cfg(feature = "smoke-kernel-thread")]
pub mod kernel_thread;

#[cfg(feature = "smoke-kernel-stack-overflow")]
pub mod kernel_stack_overflow;

#[cfg(feature = "smoke-user-trap-stack-overflow")]
pub mod user_trap_stack_overflow;

#[cfg(feature = "smoke-floating-point")]
pub mod floating_point;

#[cfg(feature = "smoke-userland")]
pub mod userland;
