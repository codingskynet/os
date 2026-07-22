//! NS16550-compatible UART driver.
//!
//! The compatible programming model is documented by the [Texas Instruments
//! PC16550D data sheet] (the current publication of National Semiconductor's
//! original interface), section 8.6, "Register Maps". In particular, this
//! driver uses the following subset of Table 1:
//!
//! | Byte offset | Access | Register | Bits used |
//! |-------------|--------|----------|-----------|
//! | `0` | read | Receiver Buffer Register (RBR) | received byte |
//! | `0` | write | Transmitter Holding Register (THR) | transmitted byte |
//! | `1` | read/write | Interrupt Enable Register (IER) | bit 0, received data available |
//! | `5` | read | Line Status Register (LSR) | bit 0, data ready; bit 5, THR empty |
//!
//! RBR, THR, and IER are selected only while the Line Control Register's
//! Divisor Latch Access Bit (DLAB) is zero (sections 8.6.1 and 8.6.2). LSR's
//! data-ready and THR-empty semantics come from section 8.6.3, and IER bit 0
//! comes from section 8.6.6. Draining RBR until LSR bit 0 clears also follows
//! the received-data interrupt reset behavior in Table 5.
//!
//! The original chip specifies an 8-bit data bus and register-select pins
//! `A2..A0`, not a system MMIO layout. This implementation assumes that the
//! platform maps those register numbers as 8-bit MMIO registers with a byte
//! stride of one. It therefore matches QEMU's RISC-V `virt` machine, which
//! provides one [NS16550-compatible UART], but not integrations that require a
//! wider access or a shifted register stride.
//!
//! The driver performs byte-at-a-time polling I/O and only enables the receive
//! interrupt. It does not program the baud divisor, line format, modem control,
//! or FIFO mode, so those settings must already be usable.
//!
//! [Texas Instruments PC16550D data sheet]:
//!     https://www.ti.com/lit/ds/symlink/pc16550d.pdf
//! [NS16550-compatible UART]:
//!     https://www.qemu.org/docs/master/system/riscv/virt.html#supported-devices

use core::convert::Infallible;
use core::fmt;
use core::ptr::{read_volatile, write_volatile};

use super::{Read, Write};
use crate::dev::console::Console;

// PC16550D section 8.6.1, Table 2. RBR, THR, and IER require LCR.DLAB = 0.
const REG_RBR: usize = 0x00; // Receiver Buffer Register (read)
const REG_THR: usize = 0x00; // Transmitter Holding Register (write)
const REG_IER: usize = 0x01; // Interrupt Enable Register
const REG_LSR: usize = 0x05; // Line Status Register (read)

// PC16550D sections 8.6.3 and 8.6.6.
const IER_RX_AVAILABLE: u8 = 0x01;
const LSR_DR: u8 = 0x01; // Data Ready
const LSR_THRE: u8 = 0x40; // Transmitter Holding Register Empty

/// Minimal NS16550 UART handle.
///
/// The handle owns no memory; it only stores the MMIO base address used for
/// volatile register accesses in the current address space. It assumes
/// byte-wide registers at consecutive addresses and `LCR.DLAB = 0` whenever
/// RBR, THR, or IER is accessed.
pub struct NS16550 {
    addr: usize,
}

impl NS16550 {
    /// Create a new NS16550 driver at the given MMIO base address.
    ///
    /// # Safety
    ///
    /// The caller must guarantee:
    ///
    /// * `addr` points to a valid NS16550-compatible device whose register
    ///   window is at least 8 bytes.
    /// * The device exposes byte-wide registers with a register stride of one.
    /// * The memory region is **not** backed by normal RAM (it is an MMIO region
    ///   that reacts to loads/stores with side-effects).
    /// * No aliasing mutable references to the same registers exist at the same
    ///   time.
    pub const unsafe fn new(addr: usize) -> Self {
        Self { addr }
    }
}

impl Console for NS16550 {
    fn enable_rx_interrupt(&mut self) {
        let ier = unsafe { read_volatile(self.reg(REG_IER)) };
        unsafe { write_volatile(self.reg(REG_IER), ier | IER_RX_AVAILABLE) };
    }

    fn drain_rx(&mut self, receive: &mut dyn FnMut(u8)) {
        while self.is_readable() {
            receive(self.read_byte());
        }
    }
}

impl fmt::Write for NS16550 {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        Write::write(self, s.as_bytes()).unwrap();
        Ok(())
    }
}

impl Write for NS16550 {
    type Error = Infallible;

    fn write(&mut self, buffer: &[u8]) -> Result<usize, Self::Error> {
        for &byte in buffer {
            while unsafe { read_volatile(self.reg(REG_LSR)) } & LSR_THRE == 0 {
                core::hint::spin_loop();
            }
            unsafe { write_volatile(self.reg(REG_THR), byte) };
        }
        Ok(buffer.len())
    }
}

impl Read for NS16550 {
    type Error = Infallible;

    fn read(&mut self, buffer: &mut [u8]) -> Result<Option<usize>, Self::Error> {
        let Some((first, rest)) = buffer.split_first_mut() else {
            return Ok(Some(0));
        };

        let Some(byte) = self.receive() else {
            return Ok(None);
        };
        *first = byte;

        // Once input is available, drain only the bytes already buffered by
        // the UART instead of waiting to fill the caller's entire buffer.
        let mut read = 1;
        for byte in rest {
            let Some(received) = self.receive() else {
                break;
            };
            *byte = received;
            read += 1;
        }

        Ok(Some(read))
    }
}

impl NS16550 {
    fn receive(&self) -> Option<u8> {
        self.is_readable().then(|| self.read_byte())
    }

    fn is_readable(&self) -> bool {
        (unsafe { read_volatile(self.reg(REG_LSR)) } & LSR_DR) != 0
    }

    fn read_byte(&self) -> u8 {
        unsafe { read_volatile(self.reg(REG_RBR)) }
    }

    fn reg(&self, offset: usize) -> *mut u8 {
        (self.addr + offset) as *mut _
    }
}
