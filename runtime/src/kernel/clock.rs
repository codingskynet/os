//! Monotonic kernel clock derived from the platform timebase.

use core::num::NonZeroU64;

use crate::arch;
use crate::dev::dt::Fdt;
use crate::kernel::dt::{self, FdtWalkeraExt, ValueaExt};
use crate::kernel::sync::freezable::{Freezable, FreezableToken};
use crate::util::consts::{MICROS_PER_SEC, MILLIS_PER_SEC, NANOS_PER_SEC};

pub static CLOCK_META: Freezable<ClockMeta> = Freezable::new(ClockMeta::empty());

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
    pub const fn empty() -> Self {
        Self {
            base: 0,
            freq: NonZeroU64::new(1).unwrap(),
        }
    }

    pub fn elapsed_ns(&self, ticks: u64) -> u64 {
        let elapsed_ticks = ticks.saturating_sub(self.base) as u128;
        let elapsed_ns = elapsed_ticks * (NANOS_PER_SEC as u128) / self.freq.get() as u128;
        elapsed_ns.min(u64::MAX as u128) as u64
    }

    pub fn init(token: &mut FreezableToken, fdt: &Fdt) -> Result<(), dt::Error> {
        let freq = fdt
            .lookup("/cpus")
            .prop_or_err("timebase-frequency")?
            .into_nonzero_scalar_or_err()?;
        let base = arch::asm::time::ticks();

        token.write(&CLOCK_META, |meta| {
            meta.base = base;
            meta.freq = freq;
        });
        token.mark_shared(&CLOCK_META);

        Ok(())
    }
}
