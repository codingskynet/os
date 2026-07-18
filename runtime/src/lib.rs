//! Runtime kernel crate.
//!
//! This crate owns the code and state that must remain valid after boot-time
//! initialization has finished.

#![no_std]
#![allow(clippy::forget_non_drop)]

extern crate alloc;

pub mod arch;
pub mod debug;
pub mod dev;
pub mod fs;
pub mod kernel;
pub mod mm;
pub mod util;

mod panic;
