use core::num::NonZeroUsize;

use crate::util::consts::G;

/// Sv39 uses 39-bit virtual addresses sign-extended to 64 bits.
///
/// The upper canonical half spans 256GiB:
///
/// ```text
/// 0xffff_ffc0_0000_0000 ..= 0xffff_ffff_ffff_ffff
/// ```
///
/// Kernel virtual memory layout:
///
/// ```text
/// 0xffff_ffc0_0000_0000 .. 0xffff_ffd0_0000_0000  MMIO direct map, 64GiB
/// 0xffff_ffd0_0000_0000 .. 0xffff_fff0_0000_0000  physical direct map, 128GiB
/// 0xffff_fff0_0000_0000 .. 0x0000_0000_0000_0000  (empty), 64GiB
/// ```
///
///
pub const UPPER_CANONICAL_ADDR: usize = 0xffff_ffc0_0000_0000;
pub const UPPER_CANONICAL_SIZE: usize = 256 * G;

pub const MMIO_MAP_ADDR: usize = 0xffff_ffc0_0000_0000;
pub const MMIO_MAP_SIZE: usize = 64 * G;

pub const DIRECT_MAP_ADDR: usize = 0xffff_ffd0_0000_0000;
pub const DIRECT_MAP_SIZE: usize = 128 * G;
pub const DIRECT_MAP_ADDR_END: usize = DIRECT_MAP_ADDR + DIRECT_MAP_SIZE;

pub const PAGE_SIZE: NonZeroUsize = NonZeroUsize::new(1 << 12).unwrap();
