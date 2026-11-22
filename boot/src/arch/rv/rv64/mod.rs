//! RISC-V 64-bit boot support.

pub mod paging;

core::arch::global_asm!(include_str!("boot.s"));
