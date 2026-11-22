//! VexRiscv supervisor CSR interrupt array.
use runtime::dev::Routes;
use runtime::dev::interrupt::InterruptController;
pub(super) struct State {
    routes: Routes,
}

fn create(routes: Routes) -> Result<State, &'static str> {
    if routes.irqs().any(|irq| irq >= 32) {
        return Err("VexRiscv IRQ out of range");
    }
    Ok(State { routes })
}
impl InterruptController for State {
    fn init_this_hart(&self) {
        self.routes.enable_devices();
        let mask = self
            .routes
            .irqs()
            .fold(0usize, |mask, irq| mask | (1 << irq));
        unsafe {
            core::arch::asm!("csrw 0x9c0, {mask}", mask = in(reg) mask);
        }
    }
    fn handle_pending(&self) -> bool {
        let pending: usize;
        unsafe {
            core::arch::asm!("csrr {p}, 0xdc0", p = out(reg) pending);
        }
        let mut schedule = false;
        for irq in self.routes.irqs() {
            if pending & (1 << irq) != 0 {
                schedule |= self
                    .routes
                    .dispatch(irq)
                    .expect("missing VexRiscv interrupt route");
            }
        }
        schedule
    }
}

extern crate alloc;

pub const DRIVER: boot::drivers::InterruptDriver = boot::drivers::InterruptDriver {
    compatibles: &["vexriscv,intc0"],
    singleton: true,
    decode_irq: runtime::dev::discovery::single_cell_irq,
    create: |_fdt, _node, routes, _mmio| Ok(alloc::boxed::Box::new(create(routes)?)),
};
