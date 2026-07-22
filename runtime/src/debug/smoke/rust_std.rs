use core::sync::atomic::Ordering;

use crate::kernel::thread;
use crate::{fs, printlnk};

const RUNNING: isize = isize::MIN;
const EXPECTED_EXIT_CODE: isize = 0;

pub fn smoke() {
    printlnk!("smoke-rust-std: start");

    let exit_code = thread::spawn(|| {
        fs::kernel_exec("/bin/rust-std-demo").expect("failed to run Rust std smoke test");
    });

    while exit_code.load(Ordering::Relaxed) == RUNNING {
        thread::yield_now();
    }

    assert_eq!(exit_code.load(Ordering::Relaxed), EXPECTED_EXIT_CODE);
    printlnk!("smoke-rust-std: done exit_code={EXPECTED_EXIT_CODE}");
}
