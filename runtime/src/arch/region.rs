//! Linker-script regions exposed as physical address ranges.

use crate::arch::consts::*;
use crate::mm::region::Region;

pub fn kernel() -> Region {
    unsafe { Region::from_raw(&_kernel_start, &_kernel_end) }
}

pub fn rx() -> Region {
    text()
}

pub fn r() -> Region {
    rodata()
}

pub fn rw() -> Region {
    debug_assert!(data().start <= bss().end);
    Region::new(data().start, bss().end).unwrap()
}

pub fn live() -> Region {
    debug_assert!(text().start <= bss().end);
    Region::new(text().start, bss().end).unwrap()
}

pub fn init() -> Region {
    unsafe { Region::from_raw(&_init_start, &_init_end) }
}

pub fn text() -> Region {
    unsafe { Region::from_raw(&_text_start, &_text_end) }
}

pub fn rodata() -> Region {
    unsafe { Region::from_raw(&_rodata_start, &_rodata_end) }
}

pub fn data() -> Region {
    unsafe { Region::from_raw(&_data_start, &_data_end) }
}

pub fn bss() -> Region {
    unsafe { Region::from_raw(&_bss_start, &_bss_end) }
}
