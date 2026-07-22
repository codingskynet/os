use core::slice;
use core::task::{Context, Poll};

use crate::arch::memory::UserMemoryGuard;
use crate::arch::paging::Permission;
use crate::kernel::file::FileDescriptor;
use crate::kernel::thread::{CurrentThread, Thread};
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
    fn poll_read(
        &mut self,
        fd: FileDescriptor,
        addr: usize,
        len: usize,
        cx: &mut Context<'_>,
    ) -> Poll<Result<usize, Error>> {
        let Some(file) = self.files.get(fd) else {
            return Poll::Ready(Err(Error::BadFileDescriptor));
        };
        if len == 0 {
            return Poll::Ready(Ok(0));
        }

        let Some(addr) = Uva::new(addr) else {
            return Poll::Ready(Err(Error::InvalidBuffer));
        };
        if !self.mm.is_accessible(addr, len, Permission::W) {
            return Poll::Ready(Err(Error::InvalidBuffer));
        }

        let _guard = UserMemoryGuard::new();

        // SAFETY: the complete range was checked above to be mapped writable user
        // memory. `UserMemoryGuard` permits supervisor access for this scope, and
        // the mutable slice does not escape this function.
        let buffer = unsafe { slice::from_raw_parts_mut(addr.as_raw() as *mut u8, len) };
        file.lock().poll_read(buffer, cx).map(Ok)
    }
}

pub fn read(fd: FileDescriptor, addr: usize, len: usize) -> Result<usize, Error> {
    CurrentThread::wait(|cx| CurrentThread::with_mut(|thread| thread.poll_read(fd, addr, len, cx)))
}
