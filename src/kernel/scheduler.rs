use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::mem::ManuallyDrop;

use crate::kernel::sync::SpinLock;
use crate::kernel::thread::{Thread, ThreadState};

pub static SCHEDULER: Scheduler = Scheduler::empty();

pub fn yield_now() {
    SCHEDULER.run_next();
}

pub fn exit_current() -> ! {
    Thread::with_current(|current| {
        current.state = ThreadState::Exited;
    });
    SCHEDULER.run_next();
    unreachable!()
}

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
        let next = self.threads.lock().pop_front().expect("empty thread queue");
        Thread::run(next);
    }
}
