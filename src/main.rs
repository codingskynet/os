#![allow(clippy::identity_op)]
#![allow(clippy::forget_non_drop)]
#![no_main]
#![no_std]

extern crate alloc;

mod arch;
mod boot;
mod dev;
mod init;
mod kernel;
mod mm;
mod panic;
mod util;

#[allow(unused)]
mod debug;
