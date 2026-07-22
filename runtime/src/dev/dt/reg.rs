use core::num::{NonZeroU64, NonZeroUsize};

use crate::dev::dt::RegIter;

#[derive(Clone)]
pub struct CompatibleRegIter<'a>(RegIter<'a>);

impl<'a> From<RegIter<'a>> for CompatibleRegIter<'a> {
    fn from(value: RegIter<'a>) -> Self {
        Self(value)
    }
}

impl<'a> Iterator for CompatibleRegIter<'a> {
    /// A target-representable `(address, nonzero size)` pair.
    type Item = Result<(usize, NonZeroUsize), IncompatibleRegError>;

    fn next(&mut self) -> Option<Self::Item> {
        let (address, size) = self.0.next()?;
        let a: Result<usize, _> = address.try_into();
        let s: Result<Option<NonZeroUsize>, _> = match size {
            Some(size) => usize::try_from(size.get()).map(|s| Some(NonZeroUsize::new(s).unwrap())),
            None => Ok(None),
        };
        match (a, s) {
            (Ok(address), Ok(Some(size))) => Some(Ok((address, size))),
            _ => Some(Err(IncompatibleRegError(address, size))),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("incompatible reg: {0}, {1:?}")]
pub struct IncompatibleRegError(u64, Option<NonZeroU64>);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_target_compatible_reg() {
        let raw = RegIter::new(b"\0\0\0\x10\0\0\0\x20", 1, 1);
        let mut reg = CompatibleRegIter::from(raw);

        assert_eq!(
            reg.next().unwrap().unwrap(),
            (0x10, NonZeroUsize::new(0x20).unwrap())
        );
        assert!(reg.next().is_none());
    }

    #[test]
    fn rejects_missing_or_zero_size() {
        let missing_size = RegIter::new(b"\0\0\0\x10", 1, 0);
        assert!(
            CompatibleRegIter::from(missing_size)
                .next()
                .unwrap()
                .is_err()
        );

        let zero_size = RegIter::new(b"\0\0\0\x10\0\0\0\0", 1, 1);
        assert!(CompatibleRegIter::from(zero_size).next().unwrap().is_err());
    }
}
