//! Library crate for `os`
//!
//! Only modules that are architecture-independent are exposed here.  Arch-
//! specific modules live behind `#[cfg(target_arch = …)]` and are compiled
//! only for their native target.

#![no_std]

#[cfg(test)]
extern crate std;

pub mod dev;
pub mod util;
