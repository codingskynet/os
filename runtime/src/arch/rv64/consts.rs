use core::num::NonZeroUsize;

use crate::util::consts::*;

unsafe extern "C" {
    pub static _kernel_start: u8;
    pub static _init_start: u8;
    pub static _init_end: u8;
    pub static _text_start: u8;
    pub static _text_end: u8;
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
/// 0xffff_ffc0_0000_0000 .. 0xffff_ffd0_0000_0000  MMIO direct map, 64 GiB(TODO)
/// 0xffff_ffd0_0000_0000 .. 0xffff_fff0_0000_0000  physical direct map, 128 GiB
/// 0xffff_fff0_0000_0000 .. 0xffff_ffff_e000_0000  reserved gap
/// 0xffff_ffff_e000_0000 .. 0xffff_ffff_f000_0000  guarded kernel stacks, 256 MiB
/// 0xffff_ffff_f000_0000 .. 0x0000_0000_0000_0000  kernel image, 256 MiB
/// ```
///
pub const LOWER_CANONICAL_BASE: usize = 0x0000_0000_0000_0000;
pub const LOWER_CANONICAL_SIZE: usize = 256 * Gi;
pub const LOWER_CANONICAL_END: usize = LOWER_CANONICAL_BASE + LOWER_CANONICAL_SIZE;

const _: () = assert!(LOWER_CANONICAL_END == NON_CANONICAL_HOLE_BASE);

pub const NON_CANONICAL_HOLE_BASE: usize = 0x0000_0040_0000_0000;
pub const NON_CANONICAL_HOLE_END: usize = 0xffff_ffc0_0000_0000;

const _: () = assert!(NON_CANONICAL_HOLE_END == UPPER_CANONICAL_BASE);

pub const UPPER_CANONICAL_BASE: usize = 0xffff_ffc0_0000_0000;

const _: () = assert!(UPPER_CANONICAL_BASE == MMIO_VMA_BASE);

pub const MMIO_VMA_BASE: usize = 0xffff_ffc0_0000_0000;
pub const MMIO_VMA_SIZE: usize = 64 * Gi;
pub const MMIO_VMA_END: usize = MMIO_VMA_BASE + MMIO_VMA_SIZE;

const _: () = assert!(MMIO_VMA_END == DIRECT_VMA_BASE);

pub const DIRECT_VMA_BASE: usize = 0xffff_ffd0_0000_0000;
pub const DIRECT_VMA_SIZE: usize = 128 * Gi;
pub const DIRECT_VMA_END: usize = DIRECT_VMA_BASE + DIRECT_VMA_SIZE;

const _: () = assert!(DIRECT_VMA_END < KERNEL_STACK_VMA_BASE);

pub const KERNEL_STACK_VMA_BASE: usize = 0xffff_ffff_e000_0000;
pub const KERNEL_STACK_VMA_SIZE: usize = 256 * Mi;
pub const KERNEL_STACK_VMA_END: usize = KERNEL_STACK_VMA_BASE + KERNEL_STACK_VMA_SIZE;

const _: () = assert!(KERNEL_STACK_VMA_END == KERNEL_VMA_BASE);

pub const KERNEL_VMA_BASE: usize = 0xffff_ffff_ffff_ffff - KERNEL_VMA_SIZE + 1;
pub const KERNEL_VMA_SIZE: usize = 256 * Mi;

pub const KERNEL_LMA_BASE: usize = 0x8000_0000;
pub const KERNEL_VMA_OFFSET: usize = KERNEL_VMA_BASE - KERNEL_LMA_BASE;

pub const PAGE_SIZE: NonZeroUsize = NonZeroUsize::new(4 * Ki).unwrap();
pub const HUGE_PAGE_SIZE: NonZeroUsize = NonZeroUsize::new(2 * Mi).unwrap();

pub const STACK_SIZE: NonZeroUsize = NonZeroUsize::new(16 * Ki).unwrap();
pub const KERNEL_STACK_SLOT_SIZE: usize = 64 * Ki;
pub const KERNEL_STACK_GUARD_SIZE: usize = PAGE_SIZE.get();
