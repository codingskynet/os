use alloc::slice;
use core::fmt::Write;

use crate::arch::memory::UserMemoryGuard;
use crate::arch::paging::Permission;
use crate::kernel::console::CONSOLE;
use crate::kernel::thread::Thread;
use crate::mm::addr::Uva;

type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(isize)]
pub enum Error {
    InvalidBuffer = -1,
    InvalidUtf8 = -2,
    Console = -3,
}

pub fn write(addr: usize, len: usize) -> Result<()> {
    if len == 0 {
        return Ok(());
    }

    let addr = Uva::new(addr).ok_or(Error::InvalidBuffer)?;
    if !Thread::is_accessible(addr, len, Permission::R) {
        return Err(Error::InvalidBuffer);
    }

    let _guard = UserMemoryGuard::new();

    let bytes = unsafe { slice::from_raw_parts(addr.as_raw() as *const u8, len) };
    match str::from_utf8(bytes) {
        Ok(text) => CONSOLE.lock().write_str(text).map_err(|_| Error::Console),
        Err(_) => Err(Error::InvalidUtf8),
    }
}
