//! Supervisor interrupt delivery through an FDT-selected PLIC.

#[expect(dead_code, reason = "host tests use standalone discovery")]
mod registers;

use runtime::dev::Routes;
use runtime::dev::discovery::Error;
use runtime::dev::dt::{Fdt, FdtWalker};
use runtime::dev::interrupt::InterruptController;
use runtime::kernel::per_core::PerCore;
use runtime::mm::mmio::Mapper;

use self::registers::{Plic, PlicConfig};

pub(super) struct State {
    plic: Plic,
    config: PlicConfig,
    routes: Routes,
}

/// # Safety
/// The mapper's boot-time requirements apply and the FDT resource must be an
/// exclusively owned PLIC register bank with suitable device memory attributes.
unsafe fn create(
    fdt: &Fdt,
    node: FdtWalker<'_>,
    routes: Routes,
    mmio: &mut Mapper,
) -> Result<State, Error> {
    let config =
        PlicConfig::from_node(fdt, node.clone(), 9).map_err(|_| "invalid PLIC topology")?;
    let ndev = node
        .prop("riscv,ndev")
        .and_then(|v| v.into_scalar_u32())
        .ok_or("missing PLIC source count")?;
    if routes.irqs().any(|irq| irq == 0 || irq > ndev) {
        return Err("PLIC IRQ out of range");
    }
    let (physical, size) = config.region();
    let registers = unsafe { mmio.map(physical, size) }.map_err(|_| "failed to map PLIC MMIO")?;
    let plic = unsafe { Plic::new(registers.as_raw()) };
    Ok(State {
        plic,
        config,
        routes,
    })
}

impl State {
    fn context(&self) -> Option<usize> {
        self.config.context(PerCore::core_id())
    }
}

impl InterruptController for State {
    fn init_this_hart(&self) {
        let Some(context) = self.context() else {
            return;
        };
        for irq in self.routes.irqs() {
            self.plic.set_priority(irq as usize, 1);
            self.plic.enable(context, irq as usize);
        }
        self.plic.set_threshold(context, 0);
        self.routes.enable_devices();
    }
    fn handle_pending(&self) -> bool {
        let Some(context) = self.context() else {
            return false;
        };
        let mut schedule = false;
        loop {
            let irq = self.plic.claim(context);
            if irq == 0 {
                break;
            }
            let handled = self.routes.dispatch(irq as u32);
            // Complete even an unsupported source before reporting the error.
            self.plic.complete(context, irq);
            schedule |= handled.expect("unhandled PLIC interrupt");
        }
        schedule
    }
}

extern crate alloc;

pub const DRIVER: boot::drivers::InterruptDriver = boot::drivers::InterruptDriver {
    compatibles: &["sifive,plic-1.0.0", "riscv,plic0"],
    singleton: false,
    decode_irq: runtime::dev::discovery::single_cell_irq,
    create: |fdt, node, routes, mmio| {
        Ok(alloc::boxed::Box::new(unsafe {
            create(fdt, node, routes, mmio)?
        }))
    },
};

pub const C900_DRIVER: boot::drivers::InterruptDriver = boot::drivers::InterruptDriver {
    compatibles: &["thead,c900-plic"],
    decode_irq: registers::decode_c900_irq,
    ..DRIVER
};
