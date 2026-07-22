use crate::kernel::file::FileDescriptor;
use crate::kernel::thread::{CurrentThread, Thread};
use crate::nonzero_enum;

nonzero_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Error {
        BadFileDescriptor = 1,
    }
}

impl Thread {
    pub fn close(&mut self, fd: FileDescriptor) -> Result<usize, Error> {
        match self.files.remove(fd) {
            Some(_) => Ok(0),
            None => Err(Error::BadFileDescriptor),
        }
    }
}

pub fn close(fd: FileDescriptor) -> Result<usize, Error> {
    CurrentThread::with_mut(|thread| thread.close(fd))
}
