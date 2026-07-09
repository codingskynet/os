//! NS16550-compatible UART driver.
//!
//! The driver performs byte-at-a-time polling writes through MMIO registers and
//! implements [`core::fmt::Write`] for early kernel logging.

use core::fmt;
use core::ptr::{read_volatile, write_volatile};

const REG_THR: usize = 0x00; // Transmitter Holding Register (write)
const REG_LSR: usize = 0x05; // Line Status Register (read)
const LSR_THRE: u8 = 0x40; // Transmitter Holding Register Empty

/// Minimal transmit-only NS16550 UART handle.
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
            // Wait for the transmitter to be ready.
            while unsafe { read_volatile(self.reg(REG_LSR)) } & LSR_THRE == 0 {
                core::hint::spin_loop();
            }
            // Write the byte to the transmitter holding register.
            unsafe { write_volatile(self.reg(REG_THR), c) };
        }
        Ok(())
    }
}

impl NS16550 {
    fn reg(&self, offset: usize) -> *mut u8 {
        (self.addr + offset) as *mut _
    }
}
