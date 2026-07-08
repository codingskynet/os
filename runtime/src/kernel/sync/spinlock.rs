use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

use crossbeam_utils::CachePadded;

use crate::arch::interrupt::InterruptGuard;

pub struct SpinLock<T: ?Sized> {
    flag: CachePadded<AtomicBool>,
    value: UnsafeCell<T>,
}

/// SAFETY: `SpinLock<T>` owns a `T`, so moving the lock to another thread may
/// also move the protected value there. That is only sound when `T: Send`.
unsafe impl<T: ?Sized + Send> Send for SpinLock<T> {}

/// SAFETY: sharing `&SpinLock<T>` between threads lets any thread acquire the
/// lock and get exclusive mutable access to `T`. The lock serializes access, so
/// `T` does not need to be `Sync`; however, the protected value can effectively
/// be handed from one thread to another through the lock guard, so `T: Send` is
/// still required.
unsafe impl<T: ?Sized + Send> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self {
        Self {
            flag: CachePadded::new(AtomicBool::new(false)),
            value: UnsafeCell::new(value),
        }
    }

    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        let interrupt_guard = InterruptGuard::new();
        loop {
            core::hint::spin_loop();

            // [Ordering::Acquire] pairs with the Release in `drop`: after this swap observes an
            // unlocked state, reads and writes through the guard must see the
            // previous owner's updates and must not be reordered before the lock.
            if !self.flag.swap(true, Ordering::Acquire) {
                break;
            }
        }

        SpinLockGuard {
            lock: self,
            _guard: interrupt_guard,
        }
    }
}

pub struct SpinLockGuard<'a, T: ?Sized + 'a> {
    lock: &'a SpinLock<T>,
    _guard: InterruptGuard,
}

impl<'a, T: ?Sized + 'a> Deref for SpinLockGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.value.get() }
    }
}

impl<'a, T: ?Sized + 'a> DerefMut for SpinLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<'a, T: ?Sized + 'a> Drop for SpinLockGuard<'a, T> {
    fn drop(&mut self) {
        // Rust runs this `Drop::drop` body before dropping the guard's fields,
        // then drops fields in declaration order. That means the lock is
        // released here first, and `interrupt_guard` is dropped afterward to
        // restore interrupts. Restoring interrupts before releasing the lock
        // could let an interrupt handler on this hart spin on the same lock.
        //
        // [Ordering::Release] publishes all writes made while the guard was held before the
        // flag becomes unlocked, so the next successful Acquire observes them.
        self.lock.flag.store(false, Ordering::Release);
    }
}
