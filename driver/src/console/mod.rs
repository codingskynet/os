//! Console driver selection and installation from the chosen UART node.
#[cfg(feature = "liteuart")]
mod liteuart;
#[cfg(feature = "ns16550")]
mod ns16550;

use alloc::boxed::Box;

use runtime::dev::console::{Console as ConsoleDevice, ConsoleConfig};
use runtime::kernel::console::Console;
use runtime::mm::mmio::Mapper;

use crate::Error;

#[derive(Clone, Copy)]
pub(crate) enum Uart {
    #[cfg(feature = "liteuart")]
    LiteX,
    #[cfg(feature = "ns16550")]
    Ns16550,
}
impl Uart {
    pub(crate) fn probe(config: &ConsoleConfig<'_>) -> Result<(Self, &'static str), Error> {
        for compatible in config.compatibles() {
            match compatible {
                #[cfg(feature = "liteuart")]
                "litex,liteuart" => return Ok((Self::LiteX, "litex,liteuart")),
                #[cfg(feature = "ns16550")]
                "ns16550" => return Ok((Self::Ns16550, "ns16550")),
                #[cfg(feature = "ns16550")]
                "ns16550a" => return Ok((Self::Ns16550, "ns16550a")),
                _ => {}
            }
        }
        Err("no enabled driver supports the chosen console")
    }
    unsafe fn create(self, base: usize) -> Box<dyn ConsoleDevice> {
        match self {
            #[cfg(feature = "liteuart")]
            Self::LiteX => Box::new(unsafe { liteuart::Uart::new(base) }),
            #[cfg(feature = "ns16550")]
            Self::Ns16550 => Box::new(unsafe { ns16550::NS16550::new(base) }),
        }
    }
}

impl Uart {
    /// Map the chosen UART resource before constructing its console backend.
    ///
    /// # Safety
    /// The mapper's boot-time requirements apply and the resource must describe
    /// device memory exclusively owned by the kernel.
    pub(crate) unsafe fn install(
        self,
        config: &ConsoleConfig<'_>,
        compatible: &str,
        mmio: &mut Mapper,
    ) -> Result<(), Error> {
        let (parent, irq) = config.interrupt().ok_or("missing console interrupt")?;
        let (physical, size) = config.reg();
        let registers =
            unsafe { mmio.map(physical, size) }.map_err(|_| "failed to map console MMIO")?;
        let backend = unsafe { self.create(registers.as_raw()) };
        Console::install(config, compatible, backend);
        crate::interrupt::register(
            parent,
            irq,
            Console::handle_rx_interrupt,
            Some(Console::enable_rx_interrupt),
        )
    }
}
