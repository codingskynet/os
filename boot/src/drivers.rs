//! Boot-specific driver sets and initial device installation.
extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::num::NonZeroUsize;

use runtime::arch::paging::MemoryType;
use runtime::dev::console::{Console as ConsoleDevice, ConsoleConfig};
use runtime::dev::discovery::Error;
use runtime::dev::dt::{Fdt, FdtWalker};
use runtime::dev::interrupt::InterruptController;
use runtime::dev::timer::TimerDevice;
use runtime::dev::{Routes, discovery, routes};
use runtime::kernel::console::Console;
use runtime::kernel::power::POWER;
use runtime::mm::mmio::Mapper;

/// A board's compile-time selection of boot drivers.
pub trait BootDevice {
    const DRIVERS: DriverSet;

    /// Install the board's runtime power callbacks after device setup.
    ///
    /// # Safety
    /// Call once on the selected board, while boot-time MMIO mapping is valid.
    #[unsafe(link_section = ".init.text")]
    unsafe fn install_power(_fdt: &Fdt, _mmio: &mut Mapper) -> Result<(), Error> {
        Ok(())
    }
}

/// Install commands for boards that reset or power off by writing one MMIO word.
///
/// # Safety
/// `address` must be an aligned, writable 32-bit device register owned by this
/// board. The supplied values must be valid commands for that register, and the
/// boot-time mapping requirements of [`Mapper::map`] must hold.
#[unsafe(link_section = ".init.text")]
pub unsafe fn install_mmio_power(
    mmio: &mut Mapper,
    address: usize,
    reset: u32,
    power_off: Option<u32>,
) -> Result<(), Error> {
    let reg = unsafe { mmio.map(address, NonZeroUsize::new(size_of::<u32>()).unwrap()) }
        .map_err(|_| "failed to map power command register")?;
    let write_command = move |value: u32| {
        // The mapping remains valid for the lifetime of the kernel.
        unsafe { reg.as_mut_ptr::<u32>().write_volatile(value) };
        Ok(())
    };
    POWER.install_reset(move || write_command(reset));
    if let Some(off) = power_off {
        POWER.install_power_off(move || write_command(off));
    }
    Ok(())
}

/// Candidate implementations; an empty role fails when that device is needed.
pub struct DriverSet {
    pub consoles: &'static [ConsoleDriver],
    pub timers: &'static [TimerDriver],
    pub interrupts: &'static [InterruptDriver],
}

/// Constructor receives the mapped console resource and may map additional outputs.
pub struct ConsoleDriver {
    /// Access type for the chosen console's `reg` resource.
    pub memory_type: MemoryType,
    /// Register the console's receive IRQ. Polled boards supply their own receive tick.
    pub rx_interrupt: bool,
    pub compatibles: &'static [&'static str],
    pub create: unsafe fn(usize, &Fdt, &mut Mapper) -> Box<dyn ConsoleDevice>,
}

/// Discovery must not access hardware. Installation runs only after selection.
pub struct TimerDriver {
    pub probe: fn(&Fdt) -> Result<bool, Error>,
    pub install: unsafe fn(&Fdt, &mut Mapper) -> Result<InstalledTimer, Error>,
}

pub struct InstalledTimer {
    pub device: &'static dyn TimerDevice,
    pub irq: Option<(u32, u32)>,
}

pub type CreateInterrupt = unsafe fn(
    &Fdt,
    FdtWalker<'_>,
    Routes,
    &mut Mapper,
) -> Result<Box<dyn InterruptController>, Error>;

pub struct InterruptDriver {
    pub compatibles: &'static [&'static str],
    /// Some controllers use one fixed CSR bank regardless of the FDT node.
    pub singleton: bool,
    /// Translate one interrupt specifier from the controller's DT binding into a source number.
    pub decode_irq: fn(&[[u8; 4]]) -> Result<u32, Error>,
    pub create: CreateInterrupt,
}

impl DriverSet {
    /// Discover and install timer, UART, then the controllers they reference.
    ///
    /// # Safety
    /// Call once on the boot hart with IRQs disabled and before cloning page
    /// tables. Firmware must describe exclusively owned device resources; all
    /// constructors must honor their descriptor's hardware and mapper contract.
    #[unsafe(link_section = ".init.text")]
    pub unsafe fn install(&self, fdt: &Fdt, mmio: &mut Mapper) -> Result<(), Error> {
        // Registration belongs to this boot invocation, never to a global lock.
        let mut registry = routes::Registry::new();
        unsafe {
            self.install_timer(fdt, mmio, &mut registry)?;
            self.install_uart(fdt, mmio, &mut registry)?;
            self.install_interrupts(fdt, mmio, registry)
        }
    }

    #[inline(never)]
    #[unsafe(link_section = ".init.text")]
    unsafe fn install_timer(
        &self,
        fdt: &Fdt,
        mmio: &mut Mapper,
        registry: &mut routes::Registry,
    ) -> Result<(), Error> {
        let mut timer = None;
        for candidate in self.timers {
            if (candidate.probe)(fdt)? && timer.replace(candidate).is_some() {
                return Err("multiple timer backends match; timer selection policy is required");
            }
        }
        let timer = timer.ok_or("no timer driver matches the FDT")?;
        let timer = unsafe { (timer.install)(fdt, mmio)? };
        runtime::dev::timer::install(timer.device);
        if let Some((parent, irq)) = timer.irq {
            registry.register(parent, irq, runtime::dev::timer::handle_interrupt, None)?;
        }

        Ok(())
    }

    #[inline(never)]
    #[unsafe(link_section = ".init.text")]
    unsafe fn install_uart(
        &self,
        fdt: &Fdt,
        mmio: &mut Mapper,
        registry: &mut routes::Registry,
    ) -> Result<(), Error> {
        let config = ConsoleConfig::from_fdt(fdt).map_err(|_| "invalid firmware console")?;
        if !discovery::path_enabled(fdt, config.path()) {
            return Err("chosen console is disabled");
        }
        // Preserve firmware compatible-string priority.
        let (driver, compatible) = config
            .compatibles()
            .find_map(|name| {
                self.consoles.iter().find_map(|driver| {
                    driver
                        .compatibles
                        .iter()
                        .find(|&&value| value == name)
                        .map(|&compatible| (driver, compatible))
                })
            })
            .ok_or("no driver supports the chosen console")?;
        if driver.rx_interrupt {
            let (parent, raw) = config.interrupt().ok_or("missing console interrupt")?;
            let irq = self.decode_irq(fdt, parent, raw)?;
            registry.register(
                parent,
                irq,
                Console::handle_rx_interrupt,
                Some(Console::enable_rx_interrupt),
            )?;
        }

        let (physical, size) = config.reg();
        let base = unsafe { mmio.map_with_type(physical, size, driver.memory_type) }
            .map_err(|_| "failed to map console resource")?;
        let backend = unsafe { (driver.create)(base.as_raw(), fdt, mmio) };
        Console::install(&config, compatible, backend);

        Ok(())
    }

    #[inline(never)]
    #[unsafe(link_section = ".init.text")]
    fn decode_irq(&self, fdt: &Fdt, parent: u32, raw: &[u8]) -> Result<u32, Error> {
        let node = discovery::phandle(fdt, parent)?;
        let (_, controller) = self.interrupt_driver(&node)?;
        (controller.decode_irq)(discovery::interrupt_cells(node, raw)?)
    }

    #[inline(never)]
    #[unsafe(link_section = ".init.text")]
    fn interrupt_driver(&self, node: &FdtWalker<'_>) -> Result<(usize, &InterruptDriver), Error> {
        let mut selected = None;
        for (index, candidate) in self.interrupts.iter().enumerate() {
            if candidate
                .compatibles
                .iter()
                .any(|name| discovery::compatible(node.clone(), name))
                && selected.replace((index, candidate)).is_some()
            {
                return Err("multiple interrupt drivers match the interrupt parent");
            }
        }
        selected.ok_or("no driver supports the interrupt parent")
    }

    #[inline(never)]
    #[unsafe(link_section = ".init.text")]
    unsafe fn install_interrupts(
        &self,
        fdt: &Fdt,
        mmio: &mut Mapper,
        mut registry: routes::Registry,
    ) -> Result<(), Error> {
        let mut controllers = Vec::new();
        let mut singletons = Vec::new();
        for routes in registry.finish()? {
            let node = discovery::phandle(fdt, routes.parent)?;
            let (index, driver) = self.interrupt_driver(&node)?;
            if driver.singleton {
                if singletons.contains(&index) {
                    return Err("multiple nodes describe the same singleton interrupt controller");
                }
                singletons.push(index);
            }
            controllers.push(unsafe { (driver.create)(fdt, node, routes, mmio)? });
        }
        // Publish only after all controllers have been constructed.
        runtime::dev::interrupt::install(
            controllers
                .into_iter()
                .map(|controller| {
                    let controller: &'static dyn InterruptController = Box::leak(controller);
                    controller
                })
                .collect(),
        );
        Ok(())
    }
}
