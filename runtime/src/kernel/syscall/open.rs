use core::{slice, str};

use crate::arch::memory::UserMemoryGuard;
use crate::arch::paging::Permission;
use crate::kernel::file::FileDescriptor;
use crate::kernel::thread::{CurrentThread, Thread};
use crate::mm::addr::Uva;
use crate::nonzero_enum;

nonzero_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Error {
        InvalidPath = 1,
        InvalidUtf8 = 2,
        NotFound = 3,
    }
}

impl Thread {
    pub fn open(&mut self, addr: usize, len: usize) -> Result<FileDescriptor, Error> {
        if len == 0 {
            return Err(Error::NotFound);
        }

        let addr = Uva::new(addr).ok_or(Error::InvalidPath)?;
        if !self.mm.is_accessible(addr, len, Permission::R) {
            return Err(Error::InvalidPath);
        }

        let _guard = UserMemoryGuard::new();

        // SAFETY: the complete range was checked above to be mapped readable user
        // memory. `UserMemoryGuard` permits supervisor access for this scope, and
        // the borrowed path does not escape this function.
        let bytes = unsafe { slice::from_raw_parts(addr.as_raw() as *const u8, len) };
        let path = str::from_utf8(bytes).map_err(|_| Error::InvalidUtf8)?;

        self.fs
            .open(path)
            .map(|node| self.files.insert(node))
            .map_err(|_| Error::NotFound)
    }
}

pub fn open(addr: usize, len: usize) -> Result<FileDescriptor, Error> {
    CurrentThread::with_mut(|thread| thread.open(addr, len))
}
