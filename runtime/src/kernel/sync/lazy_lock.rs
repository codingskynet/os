//! SMP-safe one-time initialization for runtime-created kernel globals.

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::ops::Deref;
use core::sync::atomic::{AtomicU8, Ordering};

const UNINITIALIZED: u8 = 0;
const INITIALIZING: u8 = 1;
const INITIALIZED: u8 = 2;

/// A value initialized on the first call to [`LazyLock::get_or_init`].
///
/// The initializer is supplied at the call site because kernel globals often
/// depend on boot-time data such as an FDT. Once published, the value is
/// immutable and can be read concurrently without locking.
pub struct LazyLock<T> {
    state: AtomicU8,
    value: UnsafeCell<MaybeUninit<T>>,
}

// SAFETY: initialization transfers one `T` into shared storage, so `T` must be
// movable between harts and safe to access through shared references.
unsafe impl<T: Send + Sync> Sync for LazyLock<T> {}

// SAFETY: moving an uninitialized lock or its initialized value is sound when
// the contained value itself can be moved between harts.
unsafe impl<T: Send> Send for LazyLock<T> {}

impl<T> LazyLock<T> {
    pub const fn new() -> Self {
        Self {
            state: AtomicU8::new(UNINITIALIZED),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    /// Return the initialized value, or [`None`] if initialization has not
    /// completed yet.
    pub fn get(&self) -> Option<&T> {
        if self.state.load(Ordering::Acquire) == INITIALIZED {
            Some(unsafe { (&*self.value.get()).assume_init_ref() })
        } else {
            None
        }
    }

    /// Initialize this value once and return its shared reference.
    ///
    /// If another hart is already initializing the value, this hart waits for
    /// it to publish the result. An initializer must not recursively access the
    /// same `LazyLock`.
    pub fn get_or_init(&self, init: impl FnOnce() -> T) -> &T {
        let mut init = Some(init);
        loop {
            match self.state.load(Ordering::Acquire) {
                INITIALIZED => return unsafe { (&*self.value.get()).assume_init_ref() },
                UNINITIALIZED => {
                    if self
                        .state
                        .compare_exchange(
                            UNINITIALIZED,
                            INITIALIZING,
                            Ordering::Acquire,
                            Ordering::Acquire,
                        )
                        .is_err()
                    {
                        continue;
                    }

                    let value = init.take().unwrap()();
                    unsafe { (*self.value.get()).write(value) };
                    self.state.store(INITIALIZED, Ordering::Release);
                    return unsafe { (&*self.value.get()).assume_init_ref() };
                }
                INITIALIZING => core::hint::spin_loop(),
                _ => unreachable!(),
            }
        }
    }
}

impl<T> Default for LazyLock<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Deref for LazyLock<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.get().expect("LazyLock is not initialized")
    }
}

impl<T> Drop for LazyLock<T> {
    fn drop(&mut self) {
        if *self.state.get_mut() == INITIALIZED {
            unsafe { self.value.get_mut().assume_init_drop() };
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::vec::Vec;

    use super::*;

    #[test]
    fn initializes_only_once() {
        let lock = LazyLock::<usize>::new();

        assert!(lock.get().is_none());
        assert_eq!(*lock.get_or_init(|| 42), 42);
        assert_eq!(*lock.get_or_init(|| panic!("initializer ran twice")), 42);
        assert_eq!(lock.get(), Some(&42));
    }

    #[test]
    fn concurrent_initialization_publishes_one_value() {
        let lock = Arc::new(LazyLock::<usize>::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let threads: Vec<_> = (0..8)
            .map(|value| {
                let lock = Arc::clone(&lock);
                let calls = Arc::clone(&calls);
                thread::spawn(move || {
                    *lock.get_or_init(|| {
                        calls.fetch_add(1, Ordering::Relaxed);
                        value
                    })
                })
            })
            .collect();

        let values: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();

        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert!(values.iter().all(|value| *value == values[0]));
    }
}
