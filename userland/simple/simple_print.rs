#![no_std]
#![no_main]

ulib::runtime!();

fn main() -> usize {
    use ulib::syscall;

    if syscall::write(syscall::STDOUT, b"hello, userland!\n").is_err() {
        return 1;
    }

    let Ok(fd) = syscall::open("/hello.txt") else {
        return 2;
    };

    let mut buffer = [0; 64];
    loop {
        let Ok(read) = syscall::read(fd, &mut buffer) else {
            return 3;
        };
        if read == 0 {
            break;
        }
        if syscall::write(syscall::STDOUT, &buffer[..read]) != Ok(read) {
            return 4;
        }
    }

    if syscall::close(fd).is_err() {
        return 5;
    }

    39
}
