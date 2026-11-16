//! External interrupt controller selection, routing and installation.
mod routes;
pub use routes::Routes;
#[cfg(feature = "plic")]
pub mod plic;
#[cfg(feature = "plic")]
mod plic_irq;
#[cfg(feature = "vexriscv-intc")]
mod vexriscv;

use alloc::boxed::Box;
use alloc::vec::Vec;

use runtime::dev::dt::Fdt;
use runtime::dev::interrupt::{self, InterruptController};
use runtime::kernel::sync::SpinLock;
use runtime::mm::mmio::Mapper;

use crate::{Error, discovery};

static REGISTRY: SpinLock<routes::Registry> = SpinLock::new(routes::Registry::new());

/// Register one device IRQ before controller installation closes registration.
/// The handler returns whether scheduling is needed after IRQ completion.
pub(crate) fn register(
    parent: u32,
    irq: u32,
    handler: fn() -> bool,
    enable: Option<fn()>,
) -> Result<(), Error> {
    REGISTRY.lock().register(parent, irq, handler, enable)
}

/// Boot-time selection only; runtime dispatch uses InterruptController objects.
enum Kind {
    #[cfg(feature = "plic")]
    Plic,
    #[cfg(feature = "vexriscv-intc")]
    VexRiscv,
}
impl Kind {
    fn probe(fdt: &Fdt, parent: u32) -> Result<Self, Error> {
        let _node = discovery::phandle(fdt, parent)?;
        #[cfg(feature = "plic")]
        if plic_irq::matches(&_node) {
            return Ok(Self::Plic);
        }
        #[cfg(feature = "vexriscv-intc")]
        if discovery::compatible(_node, "vexriscv,intc0") {
            return Ok(Self::VexRiscv);
        }
        Err("no enabled driver supports the interrupt parent")
    }
    unsafe fn create(
        self,
        _fdt: &Fdt,
        _routes: Routes,
        _mmio: &mut Mapper,
    ) -> Result<Box<dyn InterruptController>, Error> {
        match self {
            #[cfg(feature = "plic")]
            Self::Plic => Ok(Box::new(unsafe {
                plic_irq::create(
                    _fdt,
                    discovery::phandle(_fdt, _routes.parent)?,
                    _routes,
                    _mmio,
                )?
            })),
            #[cfg(feature = "vexriscv-intc")]
            Self::VexRiscv => Ok(Box::new(vexriscv::create(_routes)?)),
        }
    }
}

/// # Safety
/// Call once on the boot hart after devices register their IRQs. The mapper's
/// boot-time requirements apply. All registrations must refer to this FDT.
pub(crate) unsafe fn install(fdt: &Fdt, mmio: &mut Mapper) -> Result<(), Error> {
    let routes = REGISTRY.lock().finish()?;
    let mut controllers = Vec::new();
    #[cfg(feature = "vexriscv-intc")]
    let mut vexriscv_found = false;
    for routes in routes {
        let kind = Kind::probe(fdt, routes.parent)?;
        #[cfg(feature = "vexriscv-intc")]
        if matches!(kind, Kind::VexRiscv) {
            if vexriscv_found {
                return Err("multiple nodes describe the same VexRiscv CSR interrupt array");
            }
            vexriscv_found = true;
        }
        controllers.push(unsafe { kind.create(fdt, routes, mmio)? });
    }
    // Installed devices live for the kernel's lifetime. Leak only after every
    // controller has been constructed; runtime publishes immutable references.
    let controllers = controllers
        .into_iter()
        .map(|controller| {
            let controller: &'static dyn InterruptController = Box::leak(controller);
            controller
        })
        .collect();
    interrupt::install(controllers);
    Ok(())
}
