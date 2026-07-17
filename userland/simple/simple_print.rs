#![no_std]
#![no_main]

ulib::runtime!();

fn main() -> usize {
    if ulib::syscall::write("hello, userland!\n").is_err() {
        return 1;
    }
    39
}
