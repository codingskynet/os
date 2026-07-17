use core::sync::atomic::Ordering;

use crate::fs;
use crate::kernel::thread;

const RUNNING: usize = usize::MAX;
const EXPECTED_EXIT_CODE: usize = 39;

pub fn smoke() {
    let exit_code = thread::spawn(|| {
        fs::kernel_exec("/bin/simple_print").expect("failed to run userland smoke test");
    });

    while exit_code.load(Ordering::Relaxed) == RUNNING {
        thread::yield_now();
    }

    assert_eq!(exit_code.load(Ordering::Relaxed), EXPECTED_EXIT_CODE);
}
