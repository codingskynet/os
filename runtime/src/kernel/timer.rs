//! Waker-based monotonic timer waits.

use alloc::collections::btree_map::BTreeMap;
use alloc::vec::Vec;
use core::task::{Context, Poll, Waker};

use crate::kernel::clock;
use crate::kernel::sync::SpinLock;

static TIMER: SpinLock<Timer> = SpinLock::new(Timer::empty());

pub struct Timer {
    wakers: BTreeMap<u64, Vec<Waker>>,
}

impl Timer {
    const fn empty() -> Self {
        Self {
            wakers: BTreeMap::new(),
        }
    }

    pub fn wake() {
        let mut timer = TIMER.lock();
        let now = clock::clock_millis();
        while timer
            .wakers
            .first_key_value()
            .is_some_and(|(&deadline, _)| deadline <= now)
        {
            let (_, wakers) = timer.wakers.pop_first().unwrap();
            wakers.iter().for_each(Waker::wake_by_ref);
        }
    }

    fn register(deadline: u64, waker: &Waker) {
        let mut timer = TIMER.lock();

        // TODO: dedup by will_wake?
        timer
            .wakers
            .entry(deadline)
            .or_default()
            .push(waker.clone());
    }
}

pub fn poll_sleep(deadline: u64, cx: &mut Context<'_>) -> Poll<()> {
    if clock::clock_millis() >= deadline {
        Poll::Ready(())
    } else {
        Timer::register(deadline, cx.waker());
        Poll::Pending
    }
}
