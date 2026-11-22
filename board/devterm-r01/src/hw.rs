//! Byte offsets within a permanent device or uncached DMA mapping.
use core::num::NonZeroUsize;
use core::ptr::{read_volatile, write_volatile};

use runtime::arch::asm::time::ticks;
use runtime::arch::paging::MemoryType;
use runtime::mm::mmio::Mapper;

pub(crate) type Result<T> = core::result::Result<T, &'static str>;
const TICKS_PER_US: u64 = 24;

#[derive(Clone, Copy)]
pub(crate) struct Registers(usize);

impl Registers {
    pub(crate) fn map(mapper: &mut Mapper, physical: usize, bytes: usize) -> Result<Self> {
        Self::map_with_type(mapper, physical, bytes, MemoryType::IO)
    }

    pub(crate) fn map_with_type(
        mapper: &mut Mapper,
        physical: usize,
        bytes: usize,
        memory_type: MemoryType,
    ) -> Result<Self> {
        let size = NonZeroUsize::new(bytes).ok_or("empty register range")?;
        // Callers own these board peripherals and the reserved DMA windows.
        let address = unsafe { mapper.map_with_type(physical, size, memory_type) }
            .map_err(|_| "cannot map D1 device")?;
        Ok(Self(address.as_raw()))
    }

    pub(crate) fn offset(self, bytes: usize) -> Self {
        Self(self.0 + bytes)
    }

    fn address<T>(self, offset: usize) -> *mut T {
        core::ptr::with_exposed_provenance_mut(self.0 + offset)
    }

    pub(crate) fn read8(self, offset: usize) -> u8 {
        unsafe { read_volatile(self.address(offset)) }
    }

    pub(crate) fn write8(self, offset: usize, data: u8) {
        unsafe { write_volatile(self.address(offset), data) }
    }

    pub(crate) fn read16(self, offset: usize) -> u16 {
        unsafe { read_volatile(self.address(offset)) }
    }

    pub(crate) fn write16(self, offset: usize, data: u16) {
        unsafe { write_volatile(self.address(offset), data) }
    }

    pub(crate) fn get(self, offset: usize) -> u32 {
        unsafe { read_volatile(self.address(offset)) }
    }

    pub(crate) fn set(self, offset: usize, data: u32) {
        unsafe { write_volatile(self.address(offset), data) }
    }

    pub(crate) fn read_bytes(self, offset: usize, out: &mut [u8]) {
        for (index, dest) in out.iter_mut().enumerate() {
            *dest = self.read8(offset + index);
        }
    }

    pub(crate) fn update(self, reg: usize, mask: u32, fields: u32) {
        let preserved = self.get(reg) & !mask;
        self.set(reg, preserved | (fields & mask));
    }

    pub(crate) fn wait(self, reg: usize, mask: u32, target: u32, micros: u64) -> bool {
        spin_until(micros, || self.get(reg) & mask == target)
    }

    // D1-H 9.7, eight four-bit selectors per configuration word.
    pub(crate) fn pin_function(self, bank: usize, pin: usize, function: u32) {
        let bit = 4 * (pin % 8);
        self.update(bank + 4 * (pin / 8), 15 << bit, function << bit);
    }
}

fn spin_until(micros: u64, ready: impl Fn() -> bool) -> bool {
    let begin = ticks();
    loop {
        if ready() {
            return true;
        }
        if ticks().wrapping_sub(begin) >= micros * TICKS_PER_US {
            return false;
        }
        core::hint::spin_loop();
    }
}

pub(crate) fn delay_us(micros: u64) {
    spin_until(micros, || false);
}
