use core::{slice, str};

use crate::arch::memory::UserMemoryGuard;
use crate::arch::regs::GeneralRegs;
use crate::kernel::scheduler::SCHEDULER;
use crate::kernel::thread::Thread;
use crate::mm::addr::Uva;
use crate::{arch, args_enum};

args_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Syscall(usize, a1: usize, a2: usize, a3: usize, a4: usize) {
        0 => Exit(usize = a1),
        1 => Print((usize, usize) = (a1, a2)),
    }
}

impl From<&GeneralRegs> for Syscall {
    fn from(value: &GeneralRegs) -> Self {
        Self::new(value.a0, value.a1, value.a2, value.a3, value.a4)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum PrintError {
    InvalidBuffer = 1,
    InvalidUtf8 = 2,
}

pub fn print(addr: usize, len: usize) -> Result<(), PrintError> {
    if len == 0 {
        return Ok(());
    }

    let addr = Uva::new(addr).ok_or(PrintError::InvalidBuffer)?;
    if !Thread::is_current_user_readable(addr, len) {
        return Err(PrintError::InvalidBuffer);
    }

    let _guard = UserMemoryGuard::new();

    let bytes = unsafe { slice::from_raw_parts(addr.as_raw() as *const u8, len) };
    match str::from_utf8(bytes) {
        Ok(text) => {
            crate::print!("{text}");
            Ok(())
        }
        Err(_) => Err(PrintError::InvalidUtf8),
    }
}

/// Terminate the current thread with `code` and switch to the next runnable
/// thread. Never returns.
///
/// Note for callers: invoking this from inside a kernel-thread entry closure
/// skips the destructors of everything still live in that closure (there is
/// no unwinding). Prefer returning from the closure and letting
/// `_kernel_thread_start` exit on its behalf; see its doc comment.
pub fn exit(code: usize) -> ! {
    // Disable so the exit protocol (mark `Exited`, then switch away for the
    // final time) cannot be interrupted by a timer preemption half-way
    // through, which would context-switch a thread already marked `Exited`.
    //
    // There is deliberately no matching re-enable: this function never
    // returns, so there is no prior interrupt state to restore. After
    // `run_next` switches away, the next thread's saved sstatus (restored by
    // the context switch) determines the interrupt state, and this thread is
    // destroyed in `_after_switch` without ever resuming.
    arch::asm::interrupt::disable();

    Thread::with_current(|current| {
        current.set_exit(code);
    });
    SCHEDULER.run_next();
    unreachable!("exited thread resumed after scheduler switch")
}
