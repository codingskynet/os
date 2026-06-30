pub mod consts;
pub mod debug;

use core::cell::UnsafeCell;

// TODO: Dummy struct for simply impl singleton instance, will be replaced spin lock.
pub struct Global<T>(pub UnsafeCell<T>);

impl<T> Global<T> {
    pub const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    pub fn as_mut(&self) -> &mut T {
        unsafe { self.0.get().as_mut_unchecked() }
    }
}

unsafe impl<T> Sync for Global<T> {}
