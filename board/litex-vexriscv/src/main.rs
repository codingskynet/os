#![no_main]
#![no_std]

mod drivers;

boot::entry!(drivers::Devices);
