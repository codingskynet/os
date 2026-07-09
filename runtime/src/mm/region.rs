//! Half-open physical address ranges.

use core::num::NonZeroUsize;

use crate::mm::addr::{Pa, Va};

/// Half-open physical range `[start, end)`.
///
/// Empty regions are allowed when `start == end`; constructors reject inverted
/// ranges where `start > end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Region {
    pub start: Pa,
    pub end: Pa,
}

impl Region {
    pub fn new(start: Pa, end: Pa) -> Option<Self> {
        if start <= end {
            Some(Self { start, end })
        } else {
            None
        }
    }

    pub fn from_raw(start: *const u8, end: *const u8) -> Self {
        assert!(start <= end);
        Self {
            start: Va::new(start.addr()).into_pa(),
            end: Va::new(end.addr()).into_pa(),
        }
    }

    pub fn from_size(addr: Pa, size: NonZeroUsize) -> Option<Self> {
        let end = addr.checked_offset(size.into())?;
        Some(Region { start: addr, end })
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn contains(&self, addr: Pa) -> bool {
        self.start <= addr && addr < self.end
    }

    pub fn overlap(&self, other: Region) -> bool {
        self.start < other.end && other.start < self.end
    }
}
