use crate::println;

pub fn kernel_init() {
    #[cfg(feature = "fuzz-allocator")]
    crate::debug::fuzz::allocator::run();

    println!("hello, init!");
}
