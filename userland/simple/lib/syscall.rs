/// Matches `runtime::kernel::syscall::Syscall`: number in `a0`, args in `a1`…
const EXIT: usize = 0;
const PRINT: usize = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrintError {
    InvalidBuffer,
    InvalidUtf8,
    Unknown(usize),
}

pub fn print(text: &str) -> Result<(), PrintError> {
    let result: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") PRINT => result,
            in("a1") text.as_ptr(),
            in("a2") text.len(),
            options(nostack),
        );
    }
    match result {
        0 => Ok(()),
        1 => Err(PrintError::InvalidBuffer),
        2 => Err(PrintError::InvalidUtf8),
        code => Err(PrintError::Unknown(code)),
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
