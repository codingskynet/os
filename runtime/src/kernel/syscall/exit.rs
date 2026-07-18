use crate::arch;
use crate::kernel::scheduler::SCHEDULER;
use crate::kernel::thread::Thread;

impl Thread {
    /// Terminate the current thread with `code` and switch to the next runnable
    /// thread. Never returns.
    ///
    /// Note for callers: invoking this from inside a kernel-thread entry closure
    /// skips the destructors of everything still live in that closure (there is
    /// no unwinding). Prefer returning from the closure and letting
    /// `_kernel_thread_start` exit on its behalf; see its doc comment.
    pub fn exit(&mut self, code: isize) -> ! {
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

        self.set_exit(code);
        SCHEDULER.run_next();
        unreachable!("exited thread resumed after scheduler switch")
    }
}
