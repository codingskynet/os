/// Matches `runtime::kernel::syscall::Syscall`: number in `a0`, args in `a1`…
const EXIT: usize = 0;

pub fn exit(code: usize) -> ! {
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a0") EXIT,
            in("a1") code,
            options(noreturn)
        );
    }
}
