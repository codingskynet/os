#![no_main]
#![no_std]

use runtime::dev::dt::Fdt;
use runtime::mm::mmio::Mapper;

struct Virt;
impl boot::BootDevice for Virt {
    const DRIVERS: boot::DriverSet = <driver::AllDevices as boot::BootDevice>::DRIVERS;

    #[unsafe(link_section = ".init.text")]
    unsafe fn install_power(_fdt: &Fdt, mmio: &mut Mapper) -> Result<(), &'static str> {
        // QEMU virt's SiFive test finisher accepts both commands at the same address.
        unsafe { boot::drivers::install_mmio_power(mmio, 0x0010_0000, 0x7777, Some(0x5555)) }
    }
}
boot::entry!(Virt);
