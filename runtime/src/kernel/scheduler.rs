//! Global FIFO run queue for normal kernel threads.
//!
//! Per-hart idle threads are kept outside this queue in [`PerCore`].

use alloc::boxed::Box;
use alloc::collections::VecDeque;

use hashbrown::HashMap;

use crate::arch::interrupt::InterruptGuard;
use crate::kernel::per_core::PerCore;
use crate::kernel::sync::{LazyLock, SpinLock};
use crate::kernel::thread::{CurrentThread, Thread, ThreadId};

static SCHEDULER: LazyLock<Scheduler> = LazyLock::new();

/// FIFO scheduler for globally runnable normal kernel threads.
pub struct Scheduler {
    ready: SpinLock<VecDeque<Box<Thread>>>,
    parked: SpinLock<HashMap<ThreadId, Box<Thread>>>,
}

impl Scheduler {
    pub fn init() {
        SCHEDULER.get_or_init(|| Self {
            ready: SpinLock::new(VecDeque::new()),
            parked: SpinLock::new(HashMap::new()),
        });
    }

    pub fn push_ready(thread: Box<Thread>) {
        SCHEDULER.ready.lock().push_back(thread);
    }

    pub fn push_parked(mut thread: Box<Thread>) {
        let mut parked = SCHEDULER.parked.lock();
        if thread.consume_wake() {
            thread.wake();
            drop(parked);
            SCHEDULER.ready.lock().push_back(thread);
        } else {
            thread.finish_parking();
            assert!(
                parked.insert(thread.id(), thread).is_none(),
                "thread parked twice"
            );
        }
    }

    pub fn wake(id: ThreadId) {
        let thread = { SCHEDULER.parked.lock().remove(&id) };
        if let Some(mut thread) = thread {
            thread.wake();
            SCHEDULER.ready.lock().push_back(thread);
        }
    }

    /// Switch to the next globally ready thread or this hart's idle thread.
    ///
    /// When the global queue is empty, a non-idle caller switches to its parked
    /// local idle context. The switch-out path then places a still-runnable
    /// normal thread on the global queue, where another hart may claim it.
    /// If the local idle thread is already running, there is no context to take
    /// from the idle slot and this function returns `false` without switching.
    ///
    /// Returns `false` only when the current idle thread has no work to run. If
    /// a switch occurs, callers that are later rescheduled observe `true` when
    /// they resume; exited callers never return from the switch.
    pub fn run_next() -> bool {
        let _guard = InterruptGuard::new();
        let next = match SCHEDULER.ready.lock().pop_front() {
            Some(next) => next,
            None => match PerCore::take_idle() {
                Some(idle) => idle,
                None => return false, // The local idle thread is already current.
            },
        };
        CurrentThread::switch_to(next);
        true
    }
}
