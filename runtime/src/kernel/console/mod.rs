//! Kernel console and logging macros.
//!
//! Output produced before a device is installed is buffered and flushed after
//! the device tree selects the console described by `stdout-path`.

mod buffer;
mod fnode;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::convert::Infallible;
use core::fmt;
use core::task::Waker;

pub use fnode::ConsoleFnode;

use self::buffer::{RxBuffer, TxBuffer};
use crate::dev;
use crate::dev::console::ConsoleConfig;
use crate::dev::uart::ns16550::NS16550;
use crate::dev::uart::{Read, Write};
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
    fmt::Write::write_fmt(&mut *CONSOLE.lock(), args).unwrap()
}

static CONSOLE: SpinLock<Console> = SpinLock::new(Console::empty());

type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unsupported console")]
    Unsupported,
}

pub struct Console {
    backend: Option<Box<dyn dev::console::Console>>,
    tx: TxBuffer,
    rx: RxBuffer,
    rx_wakers: Vec<Waker>,
}

impl Console {
    #[allow(clippy::large_stack_frames, clippy::large_stack_arrays)]
    const fn empty() -> Self {
        Self {
            backend: None,
            tx: TxBuffer::EMPTY,
            rx: RxBuffer::EMPTY,
            rx_wakers: Vec::new(),
        }
    }

    pub fn install(config: &ConsoleConfig<'_>) -> Result<()> {
        let (base, _) = config.reg();
        let (compatible, backend) = config
            .compatibles()
            .find_map(|compatible| match compatible {
                "ns16550" | "ns16550a" => Some((
                    compatible,
                    Box::new(unsafe { NS16550::new(Pa::new(base).into_va().as_raw()) })
                        as Box<dyn dev::console::Console>,
                )),
                _ => None,
            })
            .ok_or(Error::Unsupported)?;

        CONSOLE.lock().install_backend(backend);

        printlnk!(
            "dtb: console {} @ {:#x} (compatible: {})",
            config.path(),
            config.reg().0,
            compatible,
        );

        Ok(())
    }

    pub fn enable_rx_interrupt() {
        CONSOLE
            .lock()
            .backend
            .as_mut()
            .expect("console is not installed")
            .enable_rx_interrupt();
    }

    pub fn handle_rx_interrupt() -> bool {
        let wakers = {
            let console = &mut *CONSOLE.lock();
            let backend = console.backend.as_mut().expect("console is not installed");
            let rx = &mut console.rx;
            let mut received = false;
            backend.drain_rx(&mut |byte| {
                // Always consume the UART RBR so its level-triggered source can
                // deassert. Preserve older queued input if the software ring is full.
                received = true;
                let _ = rx.push(byte);
            });

            received.then(|| core::mem::take(&mut console.rx_wakers))
        };

        let Some(readers) = wakers else {
            return false;
        };
        let woke = !readers.is_empty();
        for reader in readers {
            reader.wake();
        }
        woke
    }

    fn register_rx_waker(&mut self, waker: &Waker) {
        if !self.rx_wakers.iter().any(|reader| reader.will_wake(waker)) {
            self.rx_wakers.push(waker.clone());
        }
    }

    fn install_backend(&mut self, mut backend: Box<dyn dev::console::Console>) {
        assert!(self.backend.is_none(), "console is already installed");

        if self.tx.take_truncated() {
            backend
                .write(b"[early console output truncated]\n")
                .unwrap();
        }

        let mut buffer = [0; 64];
        loop {
            let read = self.tx.read(&mut buffer);
            if read == 0 {
                break;
            }
            backend.write(&buffer[..read]).unwrap();
        }
        self.backend = Some(backend);
    }
}

impl fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write(s.as_bytes()).unwrap();
        Ok(())
    }
}

impl Write for Console {
    type Error = Infallible;

    fn write(&mut self, buffer: &[u8]) -> core::result::Result<usize, Self::Error> {
        if let Some(backend) = self.backend.as_mut() {
            backend.write(buffer)
        } else {
            self.tx.write(buffer);
            Ok(buffer.len())
        }
    }
}

impl Read for Console {
    type Error = Infallible;

    fn read(&mut self, buffer: &mut [u8]) -> core::result::Result<Option<usize>, Self::Error> {
        if buffer.is_empty() {
            return Ok(Some(0));
        }

        let read = self.rx.read(buffer);
        if read == buffer.len() {
            return Ok(Some(read));
        }

        let Some(backend) = self.backend.as_mut() else {
            return Ok((read != 0).then_some(read));
        };
        match backend.read(&mut buffer[read..])? {
            Some(received) => Ok(Some(read + received)),
            None if read != 0 => Ok(Some(read)),
            None => Ok(None),
        }
    }
}
