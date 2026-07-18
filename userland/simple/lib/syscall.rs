/// Matches `runtime::kernel::syscall::Syscall`: number in `a0`, args in `a1`…
const EXIT: usize = 0;
const WRITE: usize = 1;
const READ: usize = 2;
const OPEN: usize = 3;
const CLOSE: usize = 4;

pub type FileDescriptor = usize;

pub const STDIN: FileDescriptor = 0;
pub const STDOUT: FileDescriptor = 1;
pub const STDERR: FileDescriptor = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteError {
    InvalidBuffer,
    BadFileDescriptor,
    Unknown(usize),
}

pub fn write(fd: FileDescriptor, buffer: &[u8]) -> Result<usize, WriteError> {
    match unsafe { syscall(WRITE, fd, buffer.as_ptr() as usize, buffer.len()) } {
        Ok(written) => Ok(written),
        Err(1) => Err(WriteError::InvalidBuffer),
        Err(2) => Err(WriteError::BadFileDescriptor),
        Err(status) => Err(WriteError::Unknown(status)),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    InvalidBuffer,
    BadFileDescriptor,
    Unknown(usize),
}

pub fn read(fd: FileDescriptor, buffer: &mut [u8]) -> Result<usize, ReadError> {
    match unsafe { syscall(READ, fd, buffer.as_mut_ptr() as usize, buffer.len()) } {
        Ok(read) => Ok(read),
        Err(1) => Err(ReadError::InvalidBuffer),
        Err(2) => Err(ReadError::BadFileDescriptor),
        Err(status) => Err(ReadError::Unknown(status)),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenError {
    InvalidPath,
    InvalidUtf8,
    NotFound,
    Unknown(usize),
}

pub fn open(path: &str) -> Result<FileDescriptor, OpenError> {
    match unsafe { syscall(OPEN, path.as_ptr() as usize, path.len(), 0) } {
        Ok(fd) => Ok(fd),
        Err(1) => Err(OpenError::InvalidPath),
        Err(2) => Err(OpenError::InvalidUtf8),
        Err(3) => Err(OpenError::NotFound),
        Err(status) => Err(OpenError::Unknown(status)),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseError {
    BadFileDescriptor,
    Unknown(usize),
}

pub fn close(fd: FileDescriptor) -> Result<(), CloseError> {
    match unsafe { syscall(CLOSE, fd, 0, 0) } {
        Ok(_) => Ok(()),
        Err(1) => Err(CloseError::BadFileDescriptor),
        Err(status) => Err(CloseError::Unknown(status)),
    }
}

/// Invoke a syscall returning its status in `a0` and value in `a1`.
///
/// # Safety
///
/// The arguments must satisfy the requirements of `number`.
unsafe fn syscall(
    number: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
) -> Result<usize, usize> {
    let status: usize;
    let value: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") number => status,
            inlateout("a1") arg1 => value,
            in("a2") arg2,
            in("a3") arg3,
            options(nostack),
        );
    }
    if status == 0 {
        Ok(value)
    } else {
        Err(status)
    }
}

pub fn exit(code: usize) -> ! {
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") EXIT,
            in("a1") code,
            options(noreturn)
        );
    }
}
