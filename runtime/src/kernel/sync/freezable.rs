//! Boot-time mutable values that become shared after initialization.
//!
//! A single [`FreezableToken`] permits mutation while boot code is building
//! global state. Once the token is forgotten, `Freezable<T>` values can only be
//! read through shared references.

use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::mem;
use core::ops::Deref;
use core::sync::atomic::{AtomicBool, Ordering};

static TOKEN_OWNED: AtomicBool = AtomicBool::new(false);
static SHARED: AtomicBool = AtomicBool::new(false);

/// Unique token that authorizes writes to [`Freezable`] values during boot.
pub struct FreezableToken {
    // prevent Clone, Copy, Send, and Sync
    _marker: PhantomData<*mut ()>,
}

impl FreezableToken {
    pub fn take() -> Option<Self> {
        if TOKEN_OWNED
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            Some(Self {
                _marker: PhantomData,
            })
        } else {
            None
        }
    }

    pub fn write<T, U>(&mut self, value: &Freezable<T>, write_fn: impl Fn(&mut T) -> U) -> U {
        assert!(!value.shared.load(Ordering::Relaxed));
        let value = unsafe { &mut *value.value.get() };
        write_fn(value)
    }

    pub fn mark_shared<T>(&mut self, value: &Freezable<T>) {
        value.shared.store(true, Ordering::Release);
    }

    pub fn forget(self) {
        SHARED.store(true, Ordering::Release);
        mem::forget(self);
    }
}

impl Drop for FreezableToken {
    fn drop(&mut self) {
        panic!("token must be explicitly forget");
    }
}

/// Value that is mutable during boot and read-only after sharing begins.
pub struct Freezable<T> {
    shared: AtomicBool,
    value: UnsafeCell<T>,
}

// SAFETY: after a `Freezable<T>` is shared, mutation through its `UnsafeCell`
// is allowed only while holding the unique `FreezableToken`; callers of
// `FreezableToken::take` must ensure that no such token exists during the
// frozen shared phase. Shared access exposes `&T`, so `T` must be `Sync`.
unsafe impl<T: Sync> Sync for Freezable<T> {}

impl<T> Deref for Freezable<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        assert!(SHARED.load(Ordering::Acquire) || self.shared.load(Ordering::Acquire));
        unsafe { &*self.value.get() }
    }
}

impl<T> Freezable<T> {
    pub const fn new(value: T) -> Self {
        Self {
            shared: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }
}
