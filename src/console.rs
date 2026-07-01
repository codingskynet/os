use core::ffi::CStr;
use core::fmt::{self, Write};
use core::{ptr, str};

use crate::dev::dt::{Fdt, RegIter};
use crate::dev::uart::ns16550::NS16550;
use crate::mm::addr::Pa;
use crate::util::Global;

/// Prints without a newline.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::console::_print(format_args!($($arg)*)));
}

/// Prints with a newline.
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ({
        $crate::console::_print(format_args!("{}\n", format_args!($($arg)*)));
    })
}

pub fn _print(args: fmt::Arguments) {
    CONSOLE.as_mut().write_fmt(args).unwrap();
}

pub static CONSOLE: Global<Console> = Global::new(Console::Ns16550(NS16550::new(
    Pa::new(0x1000_0000).as_raw(),
)));

pub enum Console {
    Ns16550(NS16550),
}

impl Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        match self {
            Console::Ns16550(ns16550) => ns16550.write_str(s),
        }
    }
}

/// Parse `stdout-path` from `/chosen` and install the matching console driver.
///
/// # Safety
///
/// `fdt` must point to a valid, readable Flattened Device Tree blob.
pub unsafe fn install_from_fdt(fdt: &Fdt) -> Result<(), Error> {
    unsafe {
        let Some(stdout_path) = fdt.query().at("chosen").prop("stdout-path") else {
            return Err(Error::NotFound);
        };
        let stdout_path = CStr::from_bytes_until_nul(stdout_path)?.to_str()?;

        // strip optional colon+options suffix, e.g. "/soc/serial@10000000:57600"
        let path = stdout_path.split(':').next().unwrap_or("");

        let mut compatible = None;
        let mut reg = None;
        for (name, value) in fdt.lookup(path).props() {
            match name {
                "compatible" => compatible = Some(value),
                "reg" => reg = Some(value),
                _ => {}
            }
        }

        let Some((compatible, reg)) = compatible.zip(reg) else {
            return Err(Error::NotFound);
        };
        let compatible = CStr::from_bytes_until_nul(compatible)?;

        // install console for known devices
        if [c"ns16550", c"ns16550a"].contains(&compatible) {
            let (ac, sc) = fdt.reg_cells(path);
            let (base, _size) = RegIter::new(reg, ac, sc).next().ok_or(Error::NotFound)?;

            ptr::write(
                CONSOLE.0.get(),
                Console::Ns16550(NS16550::new(Pa::new(base as usize).into_va().as_raw())),
            );

            println!(
                "dtb: console {} @ {:#x} (compatible: {})",
                path,
                base,
                compatible.to_str().unwrap_or_default(),
            );
        }

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Not found")]
    NotFound,
    #[error("Invalid c string")]
    InvalidCStr(#[from] core::ffi::FromBytesUntilNulError),
    #[error("Invalid utf8")]
    InvalidUtf8(#[from] str::Utf8Error),
}
