//! Monotonic kernel clock derived from the platform timebase.

use core::num::NonZeroU64;

use crate::arch;
use crate::dev::dt::Fdt;
use crate::dev::dt::util::{DtError, FdtWalkeraExt, ValueaExt};
use crate::kernel::sync::LazyLock;
use crate::util::consts::{MICROS_PER_SEC, MILLIS_PER_SEC, NANOS_PER_SEC};

pub static CLOCK_META: LazyLock<ClockMeta> = LazyLock::new();

#[allow(unused)]
pub fn clock() -> u64 {
    CLOCK_META.elapsed_ns(arch::asm::time::ticks())
}

pub fn clock_micros() -> u64 {
    let ns = CLOCK_META.elapsed_ns(arch::asm::time::ticks());
    ns / (NANOS_PER_SEC / MICROS_PER_SEC)
}

#[allow(unused)]
pub fn clock_millis() -> u64 {
    let ns = CLOCK_META.elapsed_ns(arch::asm::time::ticks());
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

    pub fn init(fdt: &Fdt) -> Result<(), DtError> {
        let freq = fdt
            .lookup("/cpus")
            .prop_or_err("timebase-frequency")?
            .into_nonzero_u64_or_err()?;
        let base = arch::asm::time::ticks();

        CLOCK_META.get_or_init(|| Self { base, freq });

        Ok(())
    }
}
