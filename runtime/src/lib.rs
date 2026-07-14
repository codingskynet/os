//! Runtime kernel crate.
//!
//! This crate owns the code and state that must remain valid after boot-time
//! initialization has finished.

#![no_std]
#![feature(allocator_api)]
#![allow(clippy::forget_non_drop)]
#![allow(unused)]

extern crate alloc;

pub mod dev;
pub mod util;

pub mod arch;
pub mod debug;
pub mod fs;
pub mod kernel;
pub mod mm;
mod panic;
