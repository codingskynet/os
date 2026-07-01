use core::fmt;
use core::num::NonZeroUsize;

use crate::arch::consts::*;

macro_rules! impl_partial_eq_usize {
    ($ty:ty) => {
        impl PartialEq<usize> for $ty {
            fn eq(&self, other: &usize) -> bool {
                self.0 == *other
            }
        }
        impl PartialEq<$ty> for usize {
            fn eq(&self, other: &$ty) -> bool {
                *self == other.0
            }
        }
    };
}
impl_partial_eq_usize!(Pa);
impl_partial_eq_usize!(Va);

fn fmt_addr(f: &mut fmt::Formatter<'_>, addr: usize) -> fmt::Result {
    write!(
        f,
        "0x{:04x}_{:04x}_{:04x}_{:04x}",
        (addr >> 48) & 0xffff,
        (addr >> 32) & 0xffff,
        (addr >> 16) & 0xffff,
        addr & 0xffff,
    )
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, derive_more::Debug)]
#[debug("Pa({})", self.0)]
pub struct Pa(usize);

impl Pa {
    pub const fn new(addr: usize) -> Self {
        Self(addr)
    }

    pub fn align_up(&self, align: NonZeroUsize) -> Self {
        let align = align.get();
        let mask = align - 1;
        Pa((self.0 + mask) & (!mask))
    }

    pub fn align_down(&self, align: NonZeroUsize) -> Self {
        let align = align.get();
        let mask = align - 1;
        Pa(self.0 & (!mask))
    }

    pub fn checked_offset(&self, offset: usize) -> Option<Self> {
        Some(Self(self.0.checked_add(offset)?))
    }

    pub const fn into_va(self) -> Va {
        Va(self.0.checked_add(DIRECT_VMA_BASE).expect("Invalid Pa"))
    }

    pub const fn as_raw(&self) -> usize {
        self.0
    }
}

impl fmt::Display for Pa {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_addr(f, self.0)
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, derive_more::Debug)]
#[debug("Va({})", self.0)]
pub struct Va(usize);

impl<T> From<&T> for Va {
    fn from(value: &T) -> Self {
        Va(value as *const T as usize)
    }
}

impl<T> From<&mut T> for Va {
    fn from(value: &mut T) -> Self {
        Va(value as *const T as usize)
    }
}

impl Va {
    pub const fn new(addr: usize) -> Self {
        Self(addr)
    }

    pub fn checked_offset(&self, offset: usize) -> Option<Self> {
        Some(Self(self.0.checked_add(offset)?))
    }

    pub fn into_pa(self) -> Pa {
        let addr = match self.0 {
            LOWER_CANONICAL_BASE..LOWER_CANONICAL_END => todo!("user-space VA not defined"),
            NON_CANONICAL_HOLE_BASE..NON_CANONICAL_HOLE_END => panic!("Invalid VA(non-canonical)"),
            DIRECT_VMA_BASE..DIRECT_VMA_END => self.0.checked_sub(DIRECT_VMA_BASE),
            KERNEL_VMA_BASE.. => self.0.checked_sub(KERNEL_VMA_OFFSET),
            _ => panic!("Undefined VA"),
        }
        .expect("Invalid Va");
        Pa(addr)
    }

    pub const fn as_raw(&self) -> usize {
        self.0
    }

    pub fn as_ptr<T>(&self) -> *const T {
        self.0 as *const T
    }

    pub fn as_mut_ptr<T>(&self) -> *mut T {
        self.0 as *mut T
    }
}

impl fmt::Display for Va {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_addr(f, self.0)
    }
}
