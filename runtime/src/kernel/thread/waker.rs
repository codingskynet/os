//! A [`Waker`] target associated with one kernel thread.
//!
//! Waking first records a notification, then asks the scheduler to move the
//! associated parked thread back to the ready queue. Recording first covers a
//! wake that races with the running-to-parked ownership handoff.

use alloc::sync::Arc;
use alloc::task::Wake;
use core::sync::atomic::{AtomicBool, Ordering};

use super::id::ThreadId;
use crate::kernel::scheduler::Scheduler;

pub struct ThreadWaker {
    id: ThreadId,
    waked: AtomicBool,
}

impl ThreadWaker {
    pub const fn new(id: ThreadId) -> Self {
        Self {
            id,
            waked: AtomicBool::new(false),
        }
    }

    pub const fn id(&self) -> ThreadId {
        self.id
    }

    pub fn consume_wake(&self) -> bool {
        self.waked.swap(false, Ordering::AcqRel)
    }

    pub fn is_waked(&self) -> bool {
        self.waked.load(Ordering::Acquire)
    }

    fn notify(&self) {
        self.waked.store(true, Ordering::Release);
        Scheduler::wake(self.id);
    }
}

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.notify();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.notify();
    }
}
