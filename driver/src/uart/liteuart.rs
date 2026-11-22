//! LiteX UART with 32-bit control and status registers.
use core::convert::Infallible;
use core::ptr::{read_volatile, write_volatile};

use runtime::dev::uart::{Read, Write};

pub fn read(addr: usize) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}
pub fn write(addr: usize, value: u32) {
    unsafe { write_volatile(addr as *mut u32, value) }
}

pub struct Uart {
    base: usize,
}
impl Uart {
    /// # Safety
    /// `base` must address the mapped, exclusively owned LiteX UART CSR bank.
    pub const unsafe fn new(base: usize) -> Self {
        Self { base }
    }
    fn receive(&self) -> Option<u8> {
        if read(self.base + 8) != 0 {
            return None;
        }
        let byte = read(self.base) as u8;
        // LiteX RX FIFO pops when the RX pending event is acknowledged.
        write(self.base + 16, 2);
        Some(byte)
    }
    fn byte(&self, byte: u8) {
        while read(self.base + 4) != 0 {
            core::hint::spin_loop();
        }
        write(self.base, u32::from(byte));
    }
}
impl Write for Uart {
    type Error = Infallible;
    fn write(&mut self, bytes: &[u8]) -> Result<usize, Infallible> {
        for &b in bytes {
            if b == b'\n' {
                self.byte(b'\r');
            }
            self.byte(b);
        }
        Ok(bytes.len())
    }
}
impl Read for Uart {
    type Error = Infallible;
    fn read(&mut self, buffer: &mut [u8]) -> Result<Option<usize>, Infallible> {
        if buffer.is_empty() {
            return Ok(Some(0));
        }
        let mut count = 0;
        for byte in buffer {
            match self.receive() {
                Some(value) => {
                    *byte = value;
                    count += 1;
                }
                None => break,
            }
        }
        Ok((count != 0).then_some(count))
    }
}
impl runtime::dev::console::Console for Uart {
    fn enable_rx_interrupt(&mut self) {
        write(self.base + 20, 2);
    }
    fn drain_rx(&mut self, _space: usize, receive: &mut dyn FnMut(u8)) {
        while let Some(byte) = self.receive() {
            receive(byte);
        }
    }
}

impl core::fmt::Write for Uart {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        Write::write(self, s.as_bytes()).unwrap();
        Ok(())
    }
}

extern crate alloc;

pub const DRIVER: boot::drivers::ConsoleDriver = boot::drivers::ConsoleDriver {
    memory_type: runtime::arch::paging::MemoryType::IO,
    rx_interrupt: true,
    compatibles: &["litex,liteuart"],
    create: |base, _, _| alloc::boxed::Box::new(unsafe { Uart::new(base) }),
};
