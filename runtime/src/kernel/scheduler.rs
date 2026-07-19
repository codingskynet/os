//! Run queue for kernel threads.

use alloc::boxed::Box;
use alloc::collections::VecDeque;

use crate::arch::interrupt::InterruptGuard;
use crate::kernel::sync::SpinLock;
use crate::kernel::thread::{CurrentThread, Thread};

pub static SCHEDULER: Scheduler = Scheduler::empty();

/// FIFO scheduler for runnable kernel threads.
pub struct Scheduler {
    threads: SpinLock<VecDeque<Box<Thread>>>,
}

impl Scheduler {
    const fn empty() -> Self {
        Self {
            threads: SpinLock::new(VecDeque::new()),
        }
    }

    pub fn push(&self, thread: Box<Thread>) {
        self.threads.lock().push_back(thread);
    }

    pub fn run_next(&self) {
        assert!(self.try_run_next(), "empty queue");
    }

    pub fn try_run_next(&self) -> bool {
        let _guard = InterruptGuard::new();
        let Some(next) = self.threads.lock().pop_front() else {
            return false;
        };
        CurrentThread::switch_to(next);
        true
    }
}
