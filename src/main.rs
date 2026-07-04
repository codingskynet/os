#![allow(clippy::identity_op)]
#![allow(clippy::forget_non_drop)]
#![no_main]
#![no_std]

mod arch;
mod boot;
mod console;
mod dev;
mod init;
mod kernel;
mod mm;
mod panic;
mod sync;
mod util;

#[allow(unused)]
mod debug;
