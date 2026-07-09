//! Interrupt masking guard used around critical sections.

use core::marker::PhantomData;

use crate::arch::{self};

/// Disables local interrupts for the lifetime of the guard.
///
/// When dropped on the same hart, the guard restores the interrupt-enable state
/// that was active when it was created.
pub struct InterruptGuard {
    was_enabled: bool,
    // Stable Rust cannot write `impl !Send` for this guard directly. The guard
    // represents local interrupt state for the current hart, so dropping it on
    // another hart/thread would restore the wrong CPU's interrupt state.
    // `PhantomData<*mut ()>` is zero-sized but makes this type `!Send`.
    _marker: PhantomData<*mut ()>,
}

impl Default for InterruptGuard {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: sharing references to the guard is harmless; only ownership/drop
// matters, and the marker keeps the guard `!Send`.
unsafe impl Sync for InterruptGuard {}

impl InterruptGuard {
    pub fn new() -> Self {
        let was_enabled = arch::asm::interrupt::is_enabled();
        if was_enabled {
            arch::asm::interrupt::disable();
        }
        Self {
            was_enabled,
            _marker: PhantomData,
        }
    }
}

impl Drop for InterruptGuard {
    fn drop(&mut self) {
        if self.was_enabled {
            arch::asm::interrupt::enable();
        }
    }
}
