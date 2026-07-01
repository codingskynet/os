use core::num::NonZeroUsize;

include!("../../../src/util/consts.rs");

unsafe extern "C" {
    pub static _kernel_start: u8;
    pub static _stext: u8;
    pub static _etext: u8;
    pub static _rodata_start: u8;
    pub static _rodata_end: u8;
    pub static _data_start: u8;
    pub static _data_end: u8;
    pub static _bss_start: u8;
    pub static _bss_end: u8;
    pub static _kernel_end: u8;
}

/// Sv39 uses 39-bit virtual addresses sign-extended to 64 bits.
///
/// ```text
/// 0x0000_0000_0000_0000 .. 0x0000_0040_0000_0000  user-space, 256 GiB
///
/// 0x0000_0040_0000_0000 .. 0xffff_ffc0_0000_0000  non-canonical hole
///
/// 0xffff_ffc0_0000_0000 .. 0xffff_ffd0_0000_0000  MMIO direct map, 64 GiB
/// 0xffff_ffd0_0000_0000 .. 0xffff_fff0_0000_0000  physical direct map, 128 GiB
/// 0xffff_fff0_0000_0000 .. 0xffff_fffe_0000_0000  (empty), 64GiB - 256 MiB
/// 0xffff_ffff_f000_0000 .. 0x0000_0000_0000_0000  kernel image, 256 MiB
/// ```
///
pub const LOWER_CANONICAL_BASE: usize = 0x0000_0000_0000_0000;
pub const LOWER_CANONICAL_SIZE: usize = 256 * G;
pub const LOWER_CANONICAL_END: usize = LOWER_CANONICAL_BASE + LOWER_CANONICAL_SIZE;

pub const NON_CANONICAL_HOLE_BASE: usize = 0x0000_0040_0000_0000;
pub const NON_CANONICAL_HOLE_END: usize = 0xffff_ffc0_0000_0000;

pub const UPPER_CANONICAL_BASE: usize = 0xffff_ffc0_0000_0000;
pub const UPPER_CANONICAL_SIZE: usize = 256 * G;

pub const MMIO_VMA_BASE: usize = 0xffff_ffc0_0000_0000;
pub const MMIO_VMA_SIZE: usize = 64 * G;

pub const DIRECT_VMA_BASE: usize = 0xffff_ffd0_0000_0000;
pub const DIRECT_VMA_SIZE: usize = 128 * G;
pub const DIRECT_VMA_END: usize = DIRECT_VMA_BASE + DIRECT_VMA_SIZE;

pub const KERNEL_VMA_BASE: usize = 0xffff_ffff_ffff_ffff - KERNEL_VMA_SIZE + 1;
pub const KERNEL_VMA_SIZE: usize = 256 * M;

pub const KERNEL_LMA_BASE: usize = 0x8000_0000;
pub const KERNEL_VMA_OFFSET: usize = KERNEL_VMA_BASE - KERNEL_LMA_BASE;

pub const PAGE_SIZE: NonZeroUsize = NonZeroUsize::new(1 << 12).unwrap();
