//! Device-tree property value decoders.
//!
//! Values remain borrowed from the original DTB. Conversion helpers interpret
//! common encodings without copying the property bytes.
//!
//! Reference: [Devicetree Specification v0.4], section 2.2.4, "Properties",
//! and section 2.3, "Standard Properties".
//!
//! [Devicetree Specification v0.4]:
//!     https://github.com/devicetree-org/devicetree-specification/releases/download/v0.4/devicetree-specification-v0.4.pdf

use core::ffi::CStr;

use crate::dev::dt::RegIter;

pub mod reg;

/// Raw borrowed bytes for a device-tree property value.
///
/// Device-tree properties are untyped at the token layer; callers choose the
/// appropriate decoder based on the property name and node binding.
#[derive(Debug, PartialEq, Eq)]
pub struct Value<'a>(&'a [u8]);

impl<'a> Value<'a> {
    pub fn new(value: &'a [u8]) -> Self {
        Self(value)
    }

    pub fn into_slice(self) -> &'a [u8] {
        self.0
    }

    pub fn into_str(self) -> Option<&'a str> {
        CStr::from_bytes_until_nul(self.0).ok()?.to_str().ok()
    }

    pub fn into_reg(self, address_cells: u32, size_cells: u32) -> RegIter<'a> {
        RegIter::new(self.0, address_cells, size_cells)
    }

    pub fn into_scalar(self) -> Option<u64> {
        match self.0.len() {
            4 => self
                .0
                .try_into()
                .map(u32::from_be_bytes)
                .ok()
                .map(u64::from),
            8 => self.0.try_into().map(u64::from_be_bytes).ok(),
            _ => None,
        }
    }
}
