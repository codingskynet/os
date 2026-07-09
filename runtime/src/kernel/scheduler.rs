//! Run queue for kernel threads.

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::mem::ManuallyDrop;

use crate::arch::interrupt::InterruptGuard;
use crate::kernel::sync::SpinLock;
use crate::kernel::thread::Thread;

pub static SCHEDULER: Scheduler = Scheduler::empty();

/// FIFO scheduler for runnable kernel threads.
pub struct Scheduler {
    threads: SpinLock<VecDeque<ManuallyDrop<Box<Thread>>>>,
}

impl Scheduler {
    const fn empty() -> Self {
        Self {
            threads: SpinLock::new(VecDeque::new()),
        }
    }

    pub fn push(&self, thread: Box<Thread>) {
        self.threads.lock().push_back(ManuallyDrop::new(thread));
    }

    pub fn run_next(&self) {
        let _guard = InterruptGuard::new();
        let next = self.threads.lock().pop_front().expect("empty thread queue");
        Thread::run(next);
    }
}
