use alloc::boxed::Box;

use super::{Error, Result};
use crate::fs::path::AbsolutePathBuf;
use crate::fs::{Fnode, MOUNTS};

#[derive(Debug, Default)]
pub struct FsContext {
    root: AbsolutePathBuf,
    cwd: AbsolutePathBuf,
}

impl FsContext {
    pub fn open(&self, path: &str) -> Result<Box<dyn Fnode>> {
        let guard = MOUNTS.lock();
        let mounts = guard.as_ref().ok_or(Error::NotFound)?;

        let root = mounts.get(&self.root).ok_or(Error::NotFound)?;
        root.open(&self.cwd.join(path))
    }
}
