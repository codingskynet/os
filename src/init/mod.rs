use crate::printlnk;

pub fn kernel_init() {
    #[cfg(debug_assertions)]
    crate::debug::smoke();

    printlnk!("hello, init!");
}
