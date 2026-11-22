//! Kernel policy at the external interrupt trap boundary.

use crate::dev::interrupt;
use crate::kernel::scheduler::Scheduler;

/// Handle external interrupts with local interrupts disabled. Controllers must
/// complete all pending IRQ processing before the kernel switches tasks.
pub fn handle_external() {
    if interrupt::system().handle_pending() {
        Scheduler::run_next();
    }
}
