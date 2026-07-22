use crate::kernel::thread::CurrentThread;
use crate::kernel::{clock, timer};

/// Sleep for at least `milliseconds`, yielding the hart between timer ticks.
pub fn sleep(milliseconds: usize) -> usize {
    let deadline = clock::clock_millis().saturating_add(milliseconds as u64);
    CurrentThread::wait(|cx| timer::poll_sleep(deadline, cx));
    0
}
