//! Kernel console and logging macros.
//!
//! The console starts with a platform default UART and is replaced from the
//! device tree once boot has parsed `stdout-path`.

use core::fmt::{self, Write};
use core::ops::DerefMut;
use core::{ptr, str};

use crate::dev::dt::{Fdt, RegIter};
use crate::dev::uart::Read;
use crate::dev::uart::ns16550::NS16550;
use crate::kernel::dt;
use crate::kernel::dt::{FdtWalkeraExt, ValueaExt};
use crate::kernel::sync::SpinLock;
use crate::mm::addr::Pa;

#[macro_export]
macro_rules! println {
    () => ({
        $crate::kernel::console::print(format_args!("\n"));
    });
    ($($arg:tt)*) => ({
        $crate::kernel::console::print(format_args!(
            "{}\n",
            format_args!($($arg)*),
        ));
    })
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::kernel::console::print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! printlnk {
    () => ({
        let us = $crate::kernel::clock::clock_micros();
        $crate::kernel::console::print(format_args!(
            "[{:>5}.{:06}]\n",
            us / $crate::util::consts::MICROS_PER_SEC,
            us % $crate::util::consts::MICROS_PER_SEC,
        ));
    });
    ($($arg:tt)*) => ({
        let us = $crate::kernel::clock::clock_micros();
        $crate::kernel::console::print(format_args!(
            "[{:>5}.{:06}] {}\n",
            us / $crate::util::consts::MICROS_PER_SEC,
            us % $crate::util::consts::MICROS_PER_SEC,
            format_args!($($arg)*),
        ));
    })
}

#[macro_export]
macro_rules! debug {
   () => (
        #[cfg(debug_assertions)]
        $crate::printlnk!()
    );
    ($($arg:tt)*) => ({
        #[cfg(debug_assertions)]
        $crate::printlnk!($($arg)*);
        #[cfg(not(debug_assertions))]
        let _ = format_args!($($arg)*);
    })
}

pub fn print(args: fmt::Arguments) {
    CONSOLE.lock().write_fmt(args).unwrap();
}

// TODO: remove and depends only on runtime installation
// SAFETY: QEMU virt exposes an NS16550-compatible UART at this physical
// address during early boot, before the device tree selects the final
// console.
pub static CONSOLE: SpinLock<Console> = SpinLock::new(Console::Ns16550(unsafe {
    NS16550::new(Pa::new(0x1000_0000).as_raw())
}));

/// Installed console backend.
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

impl Read for Console {
    type Error = core::convert::Infallible;

    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        match self {
            Console::Ns16550(ns16550) => ns16550.read(buffer),
        }
    }
}

pub fn install_from_fdt(fdt: &Fdt) -> Result<(), Error> {
    let value = fdt.query().at("chosen").prop_or_err("stdout-path")?;
    let stdout_path = value.into_str_or_err()?;

    // strip optional colon+options suffix, e.g. "/soc/serial@10000000:57600"
    let path = stdout_path.split(':').next().ok_or(Error::InvalidPath)?;

    let (compatible, mut reg) = mmio_info(fdt, path)?;

    // install console for known devices
    if ["ns16550", "ns16550a"].contains(&compatible) {
        let (base, _size) = reg.next().ok_or(Error::InvalidMmio)?;

        unsafe {
            ptr::write(
                CONSOLE.lock().deref_mut(),
                Console::Ns16550(NS16550::new(Pa::new(base as usize).into_va().as_raw())),
            );
        }

        printlnk!(
            "dtb: console {} @ {:#x} (compatible: {})",
            path,
            base,
            compatible,
        );
    }

    Ok(())
}

fn mmio_info<'a>(fdt: &'a Fdt, path: &'a str) -> Result<(&'a str, RegIter<'a>), Error> {
    let walker = fdt.lookup(path);
    let mut compatible = None;
    let mut reg = None;
    let (address_cells, size_cells) = walker.reg_cells();
    for (name, value) in walker.props() {
        match name {
            "compatible" => compatible = Some(value.into_str_or_err()?),
            "reg" => reg = Some(value.into_reg(address_cells, size_cells)),
            _ => {}
        }
    }

    compatible.zip(reg).ok_or(Error::InvalidMmio)
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("device tree error")]
    Dt(#[from] dt::Error),
    #[error("invalid path")]
    InvalidPath,
    #[error("invalid mmio")]
    InvalidMmio,
}
