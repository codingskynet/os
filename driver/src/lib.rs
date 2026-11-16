//! Feature-selected device implementations, discovered independently from FDT.
//!
//! Board crates only choose which implementations are compiled. Probing is
//! allocation-free and does not touch hardware. Device installation maps each
//! required MMIO resource after the kernel page allocator is ready.
#![no_std]
extern crate alloc;

pub mod console;
pub mod discovery;
pub mod interrupt;
pub mod timer;

use console::Uart;
pub use discovery::Error;
use runtime::dev::console::ConsoleConfig;
use runtime::dev::dt::Fdt;
use runtime::mm::mmio::Mapper;
use timer::Timer;

pub struct Device;

impl Device {
    fn console_config(fdt: &Fdt) -> Result<ConsoleConfig<'_>, Error> {
        let console = ConsoleConfig::from_fdt(fdt).map_err(|_| "invalid firmware console")?;
        if !discovery::path_enabled(fdt, console.path()) {
            return Err("chosen console is disabled");
        }
        Ok(console)
    }

    /// Discover, map and install the timer before ClockMeta initialization.
    ///
    /// # Safety
    /// Call once on the boot hart. The mapper's boot-time requirements apply;
    /// firmware resources must describe device memory exclusively owned by the OS.
    /// Call before install_intc closes IRQ registration.
    pub unsafe fn install_timer(fdt: &Fdt, mmio: &mut Mapper) -> Result<(), Error> {
        unsafe { Timer::probe(fdt)?.install(mmio) }
    }

    /// Discover, map and install the UART selected by the firmware console.
    ///
    /// # Safety
    /// Same ownership and mapper requirements as install_timer. Call before
    /// install_intc closes IRQ registration.
    pub unsafe fn install_uart(fdt: &Fdt, mmio: &mut Mapper) -> Result<(), Error> {
        let console = Self::console_config(fdt)?;
        let (uart, compatible) = Uart::probe(&console)?;
        unsafe { uart.install(&console, compatible, mmio) }
    }

    /// Install controllers for independently registered device IRQs.
    /// Runtime enables interrupts after all devices have been installed.
    ///
    /// # Safety
    /// Same ownership and mapper requirements as install_timer. Install devices
    /// first, using the same FDT; this call closes IRQ registration.
    pub unsafe fn install_intc(fdt: &Fdt, mmio: &mut Mapper) -> Result<(), Error> {
        unsafe { interrupt::install(fdt, mmio) }
    }
}
