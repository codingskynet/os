//! Only selected drivers are expanded and compiled in this board.
include!(concat!(env!("OUT_DIR"), "/drivers.rs"));

use_driver!("uart/liteuart");
use_driver!("timer/litex");
use_driver!("interrupt/vexriscv");

pub struct Devices;

impl boot::BootDevice for Devices {
    const DRIVERS: boot::DriverSet = boot::DriverSet {
        consoles: &[liteuart::DRIVER],
        timers: &[litex::DRIVER],
        interrupts: &[vexriscv::DRIVER],
    };
}
