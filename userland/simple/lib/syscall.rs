/// Matches `runtime::kernel::syscall::Syscall`: number in `a0`, args in `a1`…
const EXIT: usize = 0;
const WRITE: usize = 1;
const READ: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteError {
    InvalidBuffer,
    InvalidUtf8,
    Console,
    Unknown(isize),
}

pub fn write(text: &str) -> Result<(), WriteError> {
    let result = unsafe { syscall(WRITE, text.as_ptr() as usize, text.len()) };
    match result {
        0 => Ok(()),
        -1 => Err(WriteError::InvalidBuffer),
        -2 => Err(WriteError::InvalidUtf8),
        -3 => Err(WriteError::Console),
        code => Err(WriteError::Unknown(code)),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    InvalidBuffer,
    Console,
    Unknown(isize),
}

pub fn read(buffer: &mut [u8]) -> Result<usize, ReadError> {
    let result = unsafe { syscall(READ, buffer.as_mut_ptr() as usize, buffer.len()) };
    match result {
        0.. => Ok(result as usize),
        -1 => Err(ReadError::InvalidBuffer),
        -2 => Err(ReadError::Console),
        code => Err(ReadError::Unknown(code)),
    }
}

/// Invoke a syscall whose result follows the kernel's signed return ABI.
///
/// # Safety
///
/// `arg1` and `arg2` must satisfy the requirements of `number`.
unsafe fn syscall(number: usize, arg1: usize, arg2: usize) -> isize {
    let result: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") number => result,
            in("a1") arg1,
            in("a2") arg2,
            options(nostack),
        );
    }
    result as isize
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
