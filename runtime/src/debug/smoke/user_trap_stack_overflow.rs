//! Destructive smoke test for the U-mode trap-frame stack guard check.

use core::sync::atomic::Ordering;

use crate::kernel::thread;
use crate::{fs, printlnk};

pub fn smoke() -> ! {
    let exit_code = thread::spawn(|| {
        printlnk!("smoke-user-trap-stack-overflow: trigger U-mode guard");
        fs::kernel_exec("/bin/simple_print").expect("failed to enter U-mode guard smoke");
    });

    while exit_code.load(Ordering::Relaxed) == isize::MIN {
        thread::yield_now();
    }

    panic!("U-mode trap stack-overflow thread returned without panicking");
}
