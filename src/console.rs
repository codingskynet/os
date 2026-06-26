use core::fmt::{self, Write};

use crate::machine;

pub fn _print(args: fmt::Arguments) {
    console().write_fmt(args).unwrap();
}

/// Prints without a newline.
///
/// Carbon copy from <https://doc.rust-lang.org/src/std/macros.rs.html>
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::console::_print(format_args!($($arg)*)));
}

/// Prints with a newline.
///
/// Carbon copy from <https://doc.rust-lang.org/src/std/macros.rs.html>
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ({
        // well optimized even raw "\n" <https://github.com/rust-lang/rust/pull/97658#issuecomment-1530505696>
        $crate::console::_print(format_args!("{}\n", format_args!($($arg)*)));
    })
}

pub mod interface {
    pub use core::fmt::Write;
}

fn console() -> impl interface::Write {
    machine::console::console()
}
