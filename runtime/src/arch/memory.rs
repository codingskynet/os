//! Guard for temporary supervisor access to user memory.

use core::marker::PhantomData;

use crate::arch;

/// Allows supervisor-mode loads from user pages for the lifetime of the guard.
///
/// When dropped on the same hart, the guard restores the user-memory-access
/// state that was active when it was created.
pub struct UserMemoryGuard {
    was_enabled: bool,
    // User-memory access is local hart state. Keeping the guard `!Send`
    // prevents its drop from restoring another hart's state.
    _marker: PhantomData<*mut ()>,
}

impl Default for UserMemoryGuard {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: sharing references to the guard is harmless; only ownership/drop
// matters, and the marker keeps the guard `!Send`.
unsafe impl Sync for UserMemoryGuard {}

impl UserMemoryGuard {
    pub fn new() -> Self {
        Self {
            was_enabled: arch::asm::memory::enable_userspace_access(),
            _marker: PhantomData,
        }
    }
}

impl Drop for UserMemoryGuard {
    fn drop(&mut self) {
        if !self.was_enabled {
            arch::asm::memory::disable_userspace_access();
        }
    }
}
