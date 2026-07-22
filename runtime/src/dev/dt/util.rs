//! Fallible helpers for interpreting device-tree values in kernel init code.

use core::num::NonZeroU64;

use crate::dev::dt::FdtWalker;
use crate::dev::dt::prop::Value;

type Result<T> = core::result::Result<T, DtError>;

#[derive(Debug, thiserror::Error)]
pub enum DtError {
    #[error("Not found")]
    NotFound,
    #[error("Invalid value")]
    InvalidValue,
}

#[extend::ext]
pub impl<'a> FdtWalker<'a> {
    fn prop_or_err(self, name: &str) -> Result<Value<'a>> {
        self.prop(name).ok_or(DtError::NotFound)
    }
}

#[extend::ext]
pub impl<'a> Value<'a> {
    fn into_str_or_err(self) -> Result<&'a str> {
        self.into_str().ok_or(DtError::InvalidValue)
    }

    fn into_u64_or_err(self) -> Result<u64> {
        self.into_scalar_u64().ok_or(DtError::InvalidValue)
    }

    fn into_u32_or_err(self) -> Result<u32> {
        self.into_scalar_u32().ok_or(DtError::InvalidValue)
    }

    fn into_nonzero_u64_or_err(self) -> Result<NonZeroU64> {
        self.into_scalar_u64()
            .and_then(NonZeroU64::new)
            .ok_or(DtError::InvalidValue)
    }
}
