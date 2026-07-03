#![allow(clippy::identity_op)]
#![allow(clippy::forget_non_drop)]
#![allow(unused)]
#![no_main]
#![no_std]

mod arch;
mod boot;
mod console;
mod debug;
mod dev;
mod init;
mod kernel;
mod mm;
mod panic;
mod sync;
mod util;
