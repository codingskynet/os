//! Monotonic kernel clock derived from the platform timebase.

use core::num::NonZeroU64;

use crate::dev::timer;
use crate::kernel::sync::LazyLock;
use crate::util::consts::{MICROS_PER_SEC, MILLIS_PER_SEC, NANOS_PER_SEC};

pub static CLOCK_META: LazyLock<ClockMeta> = LazyLock::new();

/// Early logging must not read timer registers until clock installation completes.
pub fn try_clock_micros() -> Option<u64> {
    CLOCK_META
        .get()
        .map(|clock| clock.elapsed_ns(timer::device().ticks()) / (NANOS_PER_SEC / MICROS_PER_SEC))
}

#[allow(unused)]
pub fn clock() -> u64 {
    CLOCK_META.elapsed_ns(timer::device().ticks())
}

pub fn clock_micros() -> u64 {
    let ns = CLOCK_META.elapsed_ns(timer::device().ticks());
    ns / (NANOS_PER_SEC / MICROS_PER_SEC)
}

#[allow(unused)]
pub fn clock_millis() -> u64 {
    let ns = CLOCK_META.elapsed_ns(timer::device().ticks());
    ns / (NANOS_PER_SEC / MILLIS_PER_SEC)
}

/// Timebase calibration captured during boot.
pub struct ClockMeta {
    pub base: u64,
    pub freq: NonZeroU64,
}

impl ClockMeta {
    pub fn elapsed_ns(&self, ticks: u64) -> u64 {
        let elapsed_ticks = ticks.saturating_sub(self.base) as u128;
        let elapsed_ns = elapsed_ticks * (NANOS_PER_SEC as u128) / self.freq.get() as u128;
        elapsed_ns.min(u64::MAX as u128) as u64
    }

    pub fn init() {
        let timer = timer::device();
        let freq = timer.frequency();
        let base = timer.ticks();

        CLOCK_META.get_or_init(|| Self { base, freq });
    }
}
