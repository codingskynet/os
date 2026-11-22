#![no_main]
#![no_std]

extern crate alloc;

use boot::LeafAttributes;
use runtime::dev::dt::Fdt;
use runtime::mm::mmio::Mapper;

mod cdc;
mod console;
mod display;
mod hw;
mod keyboard;
mod pmic;
mod timer;

include!(concat!(env!("OUT_DIR"), "/drivers.rs"));
use_driver!("interrupt/plic");

struct Devices;
impl boot::BootDevice for Devices {
    const DRIVERS: boot::DriverSet = boot::DriverSet {
        consoles: &[console::DRIVER],
        timers: &[timer::DRIVER],
        interrupts: &[plic::C900_DRIVER],
    };

    #[unsafe(link_section = ".init.text")]
    unsafe fn install_power(_fdt: &Fdt, mmio: &mut Mapper) -> Result<(), &'static str> {
        // D1 WDT_SOFT_RST_REG, key 0x16aa and system reset request.
        unsafe { boot::drivers::install_mmio_power(mmio, 0x0205_00a8, 0x16aa_0001, None) }
    }
}
boot::entry!(Devices, paging = LeafAttributes::THEAD_C906);
