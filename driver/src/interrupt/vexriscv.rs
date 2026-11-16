//! VexRiscv supervisor CSR interrupt array.
use runtime::dev::interrupt::InterruptController;

use crate::Error;
use crate::interrupt::Routes;
pub(super) struct State {
    routes: Routes,
}

pub fn create(routes: Routes) -> Result<State, Error> {
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
