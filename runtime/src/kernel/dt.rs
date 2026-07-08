use core::num::NonZeroU64;

use crate::dev::dt::FdtWalker;
use crate::dev::dt::prop::Value;

type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Not found")]
    NotFound,
    #[error("Invalid value")]
    InvalidValue,
}

#[extend::ext]
pub impl<'a> FdtWalker<'a> {
    fn prop_or_err(self, name: &str) -> Result<Value<'a>> {
        self.prop(name).ok_or(Error::NotFound)
    }
}

#[extend::ext]
pub impl<'a> Value<'a> {
    fn into_str_or_err(self) -> Result<&'a str> {
        self.into_str().ok_or(Error::InvalidValue)
    }

    fn into_scalar_or_err(self) -> Result<u64> {
        self.into_scalar().ok_or(Error::InvalidValue)
    }

    fn into_nonzero_scalar_or_err(self) -> Result<NonZeroU64> {
        self.into_scalar()
            .and_then(NonZeroU64::new)
            .ok_or(Error::InvalidValue)
    }
}
