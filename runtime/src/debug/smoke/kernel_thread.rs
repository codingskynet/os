use core::sync::atomic::{AtomicUsize, Ordering};

use crate::kernel::thread;
use crate::printlnk;

const THREADS: usize = 16;
const ITERATIONS: usize = 32;

pub fn smoke() {
    static DONE: AtomicUsize = AtomicUsize::new(0);

    DONE.store(0, Ordering::Relaxed);
    printlnk!("smoke-kernel-thread: start threads={THREADS} iterations={ITERATIONS}");

    for thread_id in 0..THREADS {
        if thread_id % 2 == 0 {
            // preemption thread
            thread::spawn(move || {
                for iteration in 0..ITERATIONS {
                    printlnk!(
                        "smoke-kernel-thread: kernel thread {thread_id:02} iter {iteration:02}"
                    );
                }

                DONE.fetch_add(1, Ordering::Relaxed);
            });
        } else {
            // cooperative thread
            thread::spawn(move || {
                for iteration in 0..ITERATIONS {
                    printlnk!(
                        "smoke-kernel-thread: kernel thread {thread_id:02} iter {iteration:02}"
                    );
                    thread::yield_now();
                }

                DONE.fetch_add(1, Ordering::Relaxed);
            });
        }
    }

    while DONE.load(Ordering::Relaxed) != THREADS {
        thread::yield_now();
    }

    printlnk!("smoke-kernel-thread: done threads={THREADS} iterations={ITERATIONS}");
}
