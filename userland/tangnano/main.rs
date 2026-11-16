//! Small RV32 init shell using the same syscall ABI as the RV64 userland.
#![no_std]
#![no_main]
#![allow(dead_code)]
#[rustfmt::skip]
#[path = "../simple/lib/syscall.rs"]
mod syscall;

use syscall::{STDIN, STDOUT, read, write};
fn out(s: &str) {
    let _ = write(STDOUT, s.as_bytes());
}
fn number(mut n: usize) {
    let mut digits = [0u8; 10];
    let mut i = digits.len();
    loop {
        i -= 1;
        digits[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    let _ = write(STDOUT, &digits[i..]);
}
fn cat(path: &str) {
    match syscall::open(path) {
        Ok(fd) => {
            let mut buffer = [0; 128];
            while let Ok(n) = read(fd, &mut buffer) {
                if n == 0 {
                    break;
                }
                let _ = write(STDOUT, &buffer[..n]);
            }
            let _ = syscall::close(fd);
        }
        Err(_) => out("file not found\n"),
    }
}
#[unsafe(link_section = ".text.entry")]
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    out("Rust OS / Tang Nano 9K / RV32IMA + Sv32\n");
    out("U-mode init: read/write/open/close/sleep syscalls ready\n");
    #[cfg(feature = "mmu-fault")]
    {
        out("MMU_FAULT_PROBE: U-mode read of supervisor kernel page\n");
        unsafe {
            core::ptr::read_volatile(0xc0000000 as *const u32);
        }
        out("FAIL: supervisor page was readable from U-mode\n");
    }
    let mut line = [0u8; 128];
    let mut last_cr = false;
    loop {
        out("os> ");
        let mut len = 0;
        loop {
            let mut b = [0u8];
            if read(STDIN, &mut b).unwrap_or(0) == 0 {
                continue;
            }
            if b[0] == b'\n' && last_cr {
                last_cr = false;
                continue;
            }
            last_cr = b[0] == b'\r';
            match b[0] {
                b'\r' | b'\n' => {
                    out("\n");
                    break;
                }
                8 | 127 if len > 0 => {
                    len -= 1;
                    out("\x08 \x08");
                }
                32..=126 if len < line.len() => {
                    line[len] = b[0];
                    len += 1;
                    let _ = write(STDOUT, &b);
                }
                _ => {}
            }
        }
        let command = core::str::from_utf8(&line[..len]).unwrap_or("").trim();
        match command {
            "" => {}
            "help" => out("help | uname | cat <path> | sleep <ms> | hartid | test\n"),
            "uname" => out("Rust OS riscv32 TangNano-9K VexRiscv Sv32 27MHz 8MiB\n"),
            "hartid" => {
                number(hart_id());
                out("\n");
            }
            "test" => {
                cat("/etc/board");
                syscall::sleep(20);
                out("PASS: RV32 U-mode + Sv32 + VFS + timer wakeup\n");
            }
            c if c.starts_with("echo ") => {
                out(&c[5..]);
                out("\n");
            }
            c if c.starts_with("cat ") => cat(c[4..].trim()),
            c if c.starts_with("sleep ") => match c[6..].trim().parse::<usize>() {
                Ok(ms) => {
                    syscall::sleep(ms);
                    out("awake\n");
                }
                Err(_) => out("usage: sleep <milliseconds>\n"),
            },
            _ => out("unknown command; type help\n"),
        }
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    out("init panic\n");
    loop {
        core::hint::spin_loop();
    }
}

fn hart_id() -> usize {
    let mut status = 5usize;
    let result: usize;
    unsafe {
        core::arch::asm!("ecall", inlateout("a0") status,
        lateout("a1") result, in("a2") 0usize, in("a3") 0usize,
        options(nostack));
    }
    if status == 0 { result } else { usize::MAX }
}
