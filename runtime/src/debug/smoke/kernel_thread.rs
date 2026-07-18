use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use crate::kernel::thread;
use crate::printlnk;

const THREADS: usize = 16;
const ITERATIONS: usize = 32;

pub fn smoke() {
    printlnk!("smoke-kernel-thread: start threads={THREADS} iterations={ITERATIONS}");

    let mut exit_codes = Vec::with_capacity(THREADS);
    for thread_id in 0..THREADS {
        if thread_id % 2 == 0 {
            // preemption thread
            exit_codes.push(thread::spawn(move || {
                for iteration in 0..ITERATIONS {
                    printlnk!(
                        "smoke-kernel-thread: kernel thread {thread_id:02} iter {iteration:02}"
                    );
                }
            }));
        } else {
            // cooperative thread
            exit_codes.push(thread::spawn(move || {
                for iteration in 0..ITERATIONS {
                    printlnk!(
                        "smoke-kernel-thread: kernel thread {thread_id:02} iter {iteration:02}"
                    );
                    thread::yield_now();
                }
            }));
        }
    }

    while exit_codes
        .iter()
        .any(|code| code.load(Ordering::Relaxed) == isize::MIN)
    {
        thread::yield_now();
    }

    assert_eq!(exit_codes.len(), THREADS);
    for exit_code in exit_codes {
        assert_eq!(exit_code.load(Ordering::Relaxed), 0);
    }

    printlnk!("smoke-kernel-thread: done threads={THREADS} iterations={ITERATIONS}");
}
