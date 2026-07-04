use core::fmt::{self, Write};
use core::ops::DerefMut;
use core::{ptr, str};

use crate::dev::dt::{Fdt, prop};
use crate::dev::uart::ns16550::NS16550;
use crate::mm::addr::Pa;
use crate::sync::SpinLock;

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::console::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ({
        $crate::console::_print(format_args!("{}\n", format_args!($($arg)*)));
    })
}

#[macro_export]
macro_rules! debug {
   () => (
        #[cfg(debug_assertions)]
        $crate::print!("\n")
    );
    ($($arg:tt)*) => ({
        #[cfg(debug_assertions)]
        $crate::console::_print(format_args!("{}\n", format_args!($($arg)*)));
        #[cfg(not(debug_assertions))]
        let _ = format_args!($($arg)*);
    })
}

pub fn _print(args: fmt::Arguments) {
    CONSOLE.lock().write_fmt(args).unwrap();
}

pub static CONSOLE: SpinLock<Console> = SpinLock::new(Console::Ns16550(NS16550::new(
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

pub fn install_from_fdt(fdt: &Fdt) -> Result<(), Error> {
    let value = fdt
        .query()
        .at("chosen")
        .prop("stdout-path")
        .ok_or(Error::NotFound)?;
    let stdout_path = value.as_str_or_err()?;

    // strip optional colon+options suffix, e.g. "/soc/serial@10000000:57600"
    let path = stdout_path.split(':').next().ok_or(Error::NotFound)?;

    let mut compatible = None;
    let mut reg = None;
    let walker = fdt.lookup(path);
    let (address_cells, size_cells) = walker.reg_cells();
    for (name, value) in walker.props() {
        match name {
            "compatible" => compatible = Some(value.as_str_or_err()?),
            "reg" => reg = Some(value.into_reg(address_cells, size_cells)),
            _ => {}
        }
    }

    let (compatible, mut reg) = compatible.zip(reg).ok_or(Error::NotFound)?;

    // install console for known devices
    if ["ns16550", "ns16550a"].contains(&compatible) {
        let (base, _size) = reg.next().ok_or(Error::NotFound)?;

        unsafe {
            ptr::write(
                CONSOLE.lock().deref_mut(),
                Console::Ns16550(NS16550::new(Pa::new(base as usize).into_va().as_raw())),
            );
        }

        println!(
            "dtb: console {} @ {:#x} (compatible: {})",
            path, base, compatible,
        );
    }

    Ok(())
}

#[extend::ext]
impl<'a> prop::Value<'a> {
    fn as_str_or_err(self) -> Result<&'a str, Error> {
        prop::Value::into_str(self).ok_or(Error::InvalidValue)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Not found")]
    NotFound,
    #[error("Invalid value")]
    InvalidValue,
}
