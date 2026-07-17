//! NS16550-compatible UART driver.
//!
//! The driver performs byte-at-a-time polling I/O through MMIO registers.

use core::convert::Infallible;
use core::fmt;
use core::ptr::{read_volatile, write_volatile};

use super::Read;

const REG_RBR: usize = 0x00; // Receiver Buffer Register (read)
const REG_THR: usize = 0x00; // Transmitter Holding Register (write)
const REG_LSR: usize = 0x05; // Line Status Register (read)
const LSR_DR: u8 = 0x01; // Data Ready
const LSR_THRE: u8 = 0x40; // Transmitter Holding Register Empty

/// Minimal NS16550 UART handle.
///
/// The handle owns no memory; it only stores the MMIO base address used for
/// volatile register accesses in the current address space.
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
    /// * `base_addr` points to a valid NS16550-compatible device whose register
    ///   window is at least 8 bytes.
    /// * The memory region is **not** backed by normal RAM (it is an MMIO region
    ///   that reacts to loads/stores with side-effects).
    /// * No aliasing mutable references to the same registers exist at the same
    ///   time.
    pub const unsafe fn new(addr: usize) -> Self {
        Self { addr }
    }
}

impl fmt::Write for NS16550 {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.bytes() {
            while unsafe { read_volatile(self.reg(REG_LSR)) } & LSR_THRE == 0 {
                core::hint::spin_loop();
            }
            unsafe { write_volatile(self.reg(REG_THR), c) };
        }
        Ok(())
    }
}

impl Read for NS16550 {
    type Error = Infallible;

    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        let Some((first, rest)) = buffer.split_first_mut() else {
            return Ok(0);
        };

        // A blocking read waits only for the first byte.
        while !self.is_readable() {
            core::hint::spin_loop();
        }
        *first = self.read_byte();

        // Once input is available, drain only the bytes already buffered by
        // the UART instead of waiting to fill the caller's entire buffer.
        let mut read = 1;
        for byte in rest {
            if !self.is_readable() {
                break;
            }
            *byte = self.read_byte();
            read += 1;
        }

        Ok(read)
    }
}

impl NS16550 {
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
