use crate::fs;

pub fn smoke() {
    fs::kernel_exec("/bin/simple_print").expect("failed to run userland smoke test");
}
