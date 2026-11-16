//! Common scheduling-tick policy, independent of the timer hardware.
use core::num::NonZeroU32;

use crate::dev::timer;
use crate::kernel::scheduler::Scheduler;
use crate::kernel::timer::Timer;

const TICK_HZ: NonZeroU32 = NonZeroU32::new(200).unwrap();

pub fn init() {
    timer::device().start_periodic(TICK_HZ);
}

/// Called after hardware acknowledgement, with interrupts disabled.
/// Returns a scheduling request so external IRQ handlers can finish processing
/// other pending devices before switching tasks.
pub fn on_tick() -> bool {
    Timer::wake();
    true
}

/// Standard supervisor-timer trap entry. Platforms using an external IRQ call
/// `on_tick` from their interrupt dispatcher after acknowledging the device.
pub fn handle_timer() {
    if timer::device().take_event() && on_tick() {
        Scheduler::run_next();
    }
}
