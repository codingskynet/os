//! Typed physical and virtual addresses.
//!
//! The wrappers make address-space conversions explicit while preserving the
//! simple integer operations needed by low-level paging and allocator code.

use core::cmp::Ordering;
use core::fmt;
use core::num::NonZeroUsize;

use crate::arch::consts::*;

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

macro_rules! impl_display {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl fmt::Display for $ty {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    fmt_addr(f, self.0)
                }
            }
        )+
    };
}

impl_display!(Pa, Va, Uva);

/// Physical address.
///
/// `Pa` values are plain physical addresses. Converting to a virtual address
/// uses the runtime's direct map or the kernel image offset.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, derive_more::Debug)]
#[debug("Pa({})", self)]
pub struct Pa(usize);

impl PartialEq<usize> for Pa {
    fn eq(&self, other: &usize) -> bool {
        self.0 == *other
    }
}

impl PartialEq<Pa> for usize {
    fn eq(&self, other: &Pa) -> bool {
        *self == other.0
    }
}

impl PartialOrd<usize> for Pa {
    fn partial_cmp(&self, other: &usize) -> Option<Ordering> {
        Some(self.0.cmp(other))
    }
}

impl PartialOrd<Pa> for usize {
    fn partial_cmp(&self, other: &Pa) -> Option<Ordering> {
        Some(self.cmp(&other.0))
    }
}

impl TryFrom<u64> for Pa {
    type Error = core::num::TryFromIntError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Ok(Self(usize::try_from(value)?))
    }
}

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

    pub fn offset(&self, offset: impl Into<usize>) -> Self {
        Self(
            self.0
                .checked_add(offset.into())
                .expect("overflow physical address"),
        )
    }

    pub const fn into_va(self) -> Va {
        Va(self.0.checked_add(DIRECT_VMA_BASE).expect("invalid Pa"))
    }

    pub const fn into_kernel_va(self) -> Va {
        Va(self.0.checked_add(KERNEL_VMA_OFFSET).expect("invalid Pa"))
    }

    pub const fn as_raw(&self) -> usize {
        self.0
    }

    pub const fn aligned_order(&self, base: NonZeroUsize) -> usize {
        (self.0 / base.get()).trailing_zeros() as usize
    }
}

/// Virtual address in the kernel's Sv39 address layout.
///
/// `Va::into_pa` currently recognizes direct-map and kernel-image addresses;
/// user-space and other upper-canonical ranges are not implemented.
/// Lower-canonical, per-process addresses are intentionally incomparable as
/// `Va`; convert them to [`Uva`] when equality or ordering is required.
#[repr(transparent)]
#[derive(Clone, Copy, derive_more::Debug)]
#[debug("Va({})", self)]
pub struct Va(usize);

impl PartialEq for Va {
    fn eq(&self, other: &Self) -> bool {
        !self.is_user() && !other.is_user() && self.0 == other.0
    }
}

impl PartialOrd for Va {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        (!self.is_user() && !other.is_user()).then(|| self.0.cmp(&other.0))
    }
}

impl PartialEq<usize> for Va {
    fn eq(&self, other: &usize) -> bool {
        !self.is_user() && !Va::new(*other).is_user() && self.0 == *other
    }
}

impl PartialEq<Va> for usize {
    fn eq(&self, other: &Va) -> bool {
        other == self
    }
}

impl PartialOrd<usize> for Va {
    fn partial_cmp(&self, other: &usize) -> Option<Ordering> {
        (!self.is_user() && !Va::new(*other).is_user()).then(|| self.0.cmp(other))
    }
}

impl PartialOrd<Va> for usize {
    fn partial_cmp(&self, other: &Va) -> Option<Ordering> {
        other.partial_cmp(self).map(Ordering::reverse)
    }
}

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

impl<T> From<*mut T> for Va {
    fn from(value: *mut T) -> Self {
        Va(value as usize)
    }
}

impl Va {
    pub const fn new(addr: usize) -> Self {
        Self(addr)
    }

    pub const fn is_user(&self) -> bool {
        matches!(self.0, LOWER_CANONICAL_BASE..LOWER_CANONICAL_END)
    }

    pub fn align_down(&self, align: NonZeroUsize) -> Self {
        let mask = align.get() - 1;
        Self(self.0 & !mask)
    }

    pub fn offset(&self, offset: impl Into<usize>) -> Self {
        Self(
            self.0
                .checked_add(offset.into())
                .expect("overflow virtual address"),
        )
    }

    pub fn into_pa(self) -> Pa {
        let addr = match self.0 {
            LOWER_CANONICAL_BASE..LOWER_CANONICAL_END => {
                panic!("userspace VA cannot be directly converted")
            }
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

/// A virtual address in the lower-canonical, per-process address space.
///
/// Unlike [`Va`], this type has total equality and ordering and can therefore
/// safely be used as an ordered-map key.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, derive_more::Debug)]
#[debug("Uva({})", self)]
pub struct Uva(usize);

impl Uva {
    pub const fn new(addr: usize) -> Option<Self> {
        if matches!(addr, LOWER_CANONICAL_BASE..LOWER_CANONICAL_END) {
            Some(Self(addr))
        } else {
            None
        }
    }

    pub const fn as_raw(&self) -> usize {
        self.0
    }

    pub const fn into_va(self) -> Va {
        Va(self.0)
    }

    pub fn align_down(&self, align: NonZeroUsize) -> Self {
        let mask = align.get() - 1;
        Self(self.0 & !mask)
    }

    pub fn offset(&self, offset: impl Into<usize>) -> Self {
        self.checked_offset(offset)
            .expect("overflow per-user virtual address")
    }

    pub fn checked_offset(&self, offset: impl Into<usize>) -> Option<Self> {
        Some(Self(
            self.0
                .checked_add(offset.into())
                .filter(|addr| *addr < LOWER_CANONICAL_END)?,
        ))
    }
}

impl From<Uva> for Va {
    fn from(value: Uva) -> Self {
        value.into_va()
    }
}

impl TryFrom<Va> for Uva {
    type Error = Va;

    fn try_from(value: Va) -> Result<Self, Self::Error> {
        Self::new(value.0).ok_or(value)
    }
}

#[derive(Clone, Copy, derive_more::From)]
pub enum VarVa {
    User(Uva),
    Kernel(Va),
}
