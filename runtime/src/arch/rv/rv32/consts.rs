use core::num::NonZeroUsize;

use crate::util::consts::*;

#[path = "../../linker_consts.rs"]
mod linker_consts;
pub use linker_consts::*;

/// Sv32 uses the full 32-bit virtual address space, with no non-canonical hole.
/// Like RV64, the upper half places MMIO before the physical direct map,
/// followed by guarded stacks and the kernel image at the top of the space.
/// The split is OS policy, not an Sv32 requirement.
///
/// ```text
/// 0x0000_0000 .. 0x8000_0000  user-space, 2 GiB
///
/// 0x8000_0000 .. 0x9000_0000  MMIO mapping window, 256 MiB
/// 0x9000_0000 .. 0xe000_0000  physical direct map, 1280 MiB
/// 0xe000_0000 .. 0xfec0_0000  reserved gap
/// 0xfec0_0000 .. 0xff00_0000  guarded kernel stacks, 4 MiB
/// 0xff00_0000 ..=0xffff_ffff  kernel image window, 16 MiB
/// ```
///
/// All ranges are half-open. The kernel image's exclusive end is 2^32 and
/// cannot be represented by an RV32 usize. As on RV64, it has no END constant.
/// Firmware describes which RAM and devices actually populate these windows.
pub const LOWER_CANONICAL_BASE: usize = 0;
pub const LOWER_CANONICAL_SIZE: usize = 2 * Gi;
pub const LOWER_CANONICAL_END: usize = LOWER_CANONICAL_BASE + LOWER_CANONICAL_SIZE;

const _: () = assert!(LOWER_CANONICAL_END == NON_CANONICAL_HOLE_BASE);

// An empty interval represents Sv32's absent non-canonical hole.
pub const NON_CANONICAL_HOLE_BASE: usize = 0x8000_0000;
pub const NON_CANONICAL_HOLE_END: usize = 0x8000_0000;

const _: () = assert!(NON_CANONICAL_HOLE_BASE == NON_CANONICAL_HOLE_END);
const _: () = assert!(NON_CANONICAL_HOLE_END == UPPER_CANONICAL_BASE);

pub const UPPER_CANONICAL_BASE: usize = 0x8000_0000;

const _: () = assert!(UPPER_CANONICAL_BASE == MMIO_VMA_BASE);

pub const MMIO_VMA_BASE: usize = 0x8000_0000;
pub const MMIO_VMA_SIZE: usize = 256 * Mi;
pub const MMIO_VMA_END: usize = MMIO_VMA_BASE + MMIO_VMA_SIZE;

const _: () = assert!(MMIO_VMA_END == DIRECT_VMA_BASE);

// As on RV64, PA zero corresponds to DIRECT_VMA_BASE: VA = PA + base.
// The window covers PA [0, 0x5000_0000); RAM at 0x4000_0000 maps to 0xd000_0000.
pub const DIRECT_VMA_BASE: usize = 0x9000_0000;
pub const DIRECT_VMA_SIZE: usize = Gi + 256 * Mi;
pub const DIRECT_VMA_END: usize = DIRECT_VMA_BASE + DIRECT_VMA_SIZE;

const _: () = assert!(DIRECT_VMA_END < KERNEL_STACK_VMA_BASE);

pub const KERNEL_STACK_VMA_BASE: usize = 0xfec0_0000;
pub const KERNEL_STACK_VMA_SIZE: usize = 4 * Mi;
pub const KERNEL_STACK_VMA_END: usize = KERNEL_STACK_VMA_BASE + KERNEL_STACK_VMA_SIZE;

const _: () = assert!(KERNEL_STACK_VMA_END == KERNEL_VMA_BASE);

// Spell out the target maximum: build.rs also includes this file on 64-bit hosts.
pub const KERNEL_VMA_BASE: usize = 0xffff_ffff - KERNEL_VMA_SIZE + 1;
pub const KERNEL_VMA_SIZE: usize = 16 * Mi;

// Sv32 has 4 KiB pages and 4 MiB superpages. A page table contains 1024
// four-byte entries; a superpage covers 1024 * 4 KiB in one root-level leaf.
pub const PAGE_SIZE: NonZeroUsize = NonZeroUsize::new(4 * Ki).unwrap();
pub const HUGE_PAGE_SIZE: NonZeroUsize = NonZeroUsize::new(4 * Mi).unwrap();

// A 16 KiB stack fits inside each 64 KiB slot with a one-page guard.
// STACK_SIZE is also used by the linker to reserve the temporary boot stack.
pub const STACK_SIZE: NonZeroUsize = NonZeroUsize::new(16 * Ki).unwrap();
pub const KERNEL_STACK_SLOT_SIZE: usize = 64 * Ki;
pub const KERNEL_STACK_GUARD_SIZE: usize = PAGE_SIZE.get();
