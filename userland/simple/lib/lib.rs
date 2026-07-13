#![no_std]

pub mod syscall;

/// Return type of `fn main`, mirroring a tiny subset of `std::process::Termination`.
pub trait Termination {
    fn report(self) -> usize;
}

impl Termination for () {
    fn report(self) -> usize {
        0
    }
}

impl Termination for i32 {
    fn report(self) -> usize {
        self as usize
    }
}

impl Termination for usize {
    fn report(self) -> usize {
        self
    }
}

/// Expand once in each app crate so `_start` can call that crate's `fn main`.
#[macro_export]
macro_rules! runtime {
    () => {
        #[panic_handler]
        fn __ulib_panic(_: &::core::panic::PanicInfo) -> ! {
            $crate::syscall::exit(128);
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn _start() -> ! {
            let code = $crate::Termination::report(main());
            $crate::syscall::exit(code);
        }
    };
}
