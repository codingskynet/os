mod close;
mod open;
mod read;
mod sleep;
mod write;

use core::num::NonZeroUsize;

use crate::arch::regs::GeneralRegs;
use crate::args_enum;
use crate::kernel::file::FileDescriptor;
use crate::kernel::per_core::PerCore;
use crate::kernel::thread;

args_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Syscall(usize, a1: usize, a2: usize, a3: usize, a4: usize) {
        0 => Exit(isize = a1 as isize),
        1 => Write((FileDescriptor, usize, usize) = (a1.into(), a2, a3)),
        2 => Read((FileDescriptor, usize, usize) = (a1.into(), a2, a3)),
        3 => Open((usize, usize) = (a1, a2)),
        4 => Close(FileDescriptor = a1.into()),
        5 => HartId,
        6 => Sleep(usize = a1),
    }
}

impl From<&GeneralRegs> for Syscall {
    fn from(value: &GeneralRegs) -> Self {
        Self::new(value.a0, value.a1, value.a2, value.a3, value.a4)
    }
}

#[repr(C)]
struct SyscallResult {
    status: usize,
    value: usize,
}

impl<T: Into<usize>, E: Into<NonZeroUsize>> From<core::result::Result<T, E>> for SyscallResult {
    fn from(value: core::result::Result<T, E>) -> Self {
        match value {
            Ok(value) => Self::ok(value),
            Err(status) => Self::err(status),
        }
    }
}

impl From<usize> for SyscallResult {
    fn from(value: usize) -> Self {
        Self::ok(value)
    }
}

impl From<SyscallResult> for (usize, usize) {
    fn from(value: SyscallResult) -> Self {
        let SyscallResult { status, value } = value;
        (status, value)
    }
}

impl SyscallResult {
    pub fn ok<T: Into<usize>>(value: T) -> Self {
        Self {
            status: 0,
            value: value.into(),
        }
    }

    pub fn err<E: Into<NonZeroUsize>>(status: E) -> Self {
        Self {
            status: status.into().get(),
            value: 0,
        }
    }
}

pub fn handle(syscall: Syscall) -> (usize, usize) {
    let result: SyscallResult = match syscall {
        Syscall::Exit(code) => thread::exit(code),
        Syscall::Write((fd, addr, len)) => write::write(fd, addr, len).into(),
        Syscall::Read((fd, addr, len)) => read::read(fd, addr, len).into(),
        Syscall::Open((addr, len)) => open::open(addr, len).into(),
        Syscall::Close(fd) => close::close(fd).into(),
        Syscall::HartId => SyscallResult::ok(PerCore::with_mut(|per_core| per_core.hart_id)),
        Syscall::Sleep(milliseconds) => sleep::sleep(milliseconds).into(),
        Syscall::Unknown(number) => panic!("unhandled ecall from U-mode: number={number}"),
    };
    result.into()
}
