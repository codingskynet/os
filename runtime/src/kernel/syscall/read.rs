use core::slice;

use crate::arch::memory::UserMemoryGuard;
use crate::arch::paging::Permission;
use crate::dev::uart::Read;
use crate::kernel::console::CONSOLE;
use crate::kernel::thread::Thread;
use crate::mm::addr::Uva;

type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(isize)]
pub enum Error {
    InvalidBuffer = -1,
    Console = -2,
}

pub fn read(addr: usize, len: usize) -> Result<usize> {
    if len == 0 {
        return Ok(0);
    }

    let addr = Uva::new(addr).ok_or(Error::InvalidBuffer)?;
    if !Thread::is_accessible(addr, len, Permission::W) {
        return Err(Error::InvalidBuffer);
    }

    let _guard = UserMemoryGuard::new();

    // SAFETY: the complete range was checked above to be mapped writable user
    // memory. `UserMemoryGuard` permits supervisor access for this scope, and
    // the mutable slice does not escape this function.
    let buffer = unsafe { slice::from_raw_parts_mut(addr.as_raw() as *mut u8, len) };
    match CONSOLE.lock().read(buffer) {
        Ok(read) => Ok(read),
        Err(error) => Err(Error::Console),
    }
}
