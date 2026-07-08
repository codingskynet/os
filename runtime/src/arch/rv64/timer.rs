use crate::arch;
use crate::arch::asm::interrupt::allow_timer;
use crate::arch::asm::timer::set_deadline;
use crate::kernel::clock::CLOCK_META;
use crate::kernel::scheduler::SCHEDULER;

const TIMER_HZ: u64 = 200;

pub fn init() {
    schedule_next_tick();
    allow_timer();
}

pub fn handle_timer() {
    schedule_next_tick();
    SCHEDULER.run_next();
}

fn schedule_next_tick() {
    let interval = (CLOCK_META.freq.get() / TIMER_HZ).max(1);
    let deadline = arch::asm::time::ticks().saturating_add(interval);
    set_deadline(deadline);
}
