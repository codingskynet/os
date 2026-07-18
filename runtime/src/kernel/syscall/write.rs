use alloc::slice;

use crate::arch::memory::UserMemoryGuard;
use crate::arch::paging::Permission;
use crate::kernel::file::FileDescriptor;
use crate::kernel::thread::Thread;
use crate::mm::addr::Uva;
use crate::nonzero_enum;

nonzero_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Error {
        InvalidBuffer = 1,
        BadFileDescriptor = 2,
    }
}

impl Thread {
    pub fn write(&mut self, fd: FileDescriptor, addr: usize, len: usize) -> Result<usize, Error> {
        let Some(file) = self.files.get(fd) else {
            return Err(Error::BadFileDescriptor);
        };
        if len == 0 {
            return Ok(0);
        }

        let addr = Uva::new(addr).ok_or(Error::InvalidBuffer)?;
        if !self.mm.is_accessible(addr, len, Permission::R) {
            return Err(Error::InvalidBuffer);
        }

        let _guard = UserMemoryGuard::new();

        // SAFETY: the complete range was checked above to be mapped readable user
        // memory. `UserMemoryGuard` permits supervisor access for this scope, and
        // the slice does not escape this function.
        let buffer = unsafe { slice::from_raw_parts(addr.as_raw() as *const u8, len) };
        Ok(file.lock().write(buffer))
    }
}
