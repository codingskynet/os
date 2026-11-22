//! RV64 Sstc timer; firmware must permit supervisor time/stimecmp access.
#![cfg(target_arch = "riscv64")]
use core::num::{NonZeroU32, NonZeroU64};
use core::sync::atomic::{AtomicU64, Ordering};

use runtime::arch::asm::interrupt::allow_timer;
use runtime::arch::asm::time::ticks;
use runtime::arch::asm::timer::set_deadline;
use runtime::dev::discovery::{self, Error};
use runtime::dev::dt::Fdt;
use runtime::dev::timer::TimerDevice;
use runtime::kernel::sync::LazyLock;

#[derive(Clone, Copy)]
pub struct Config {
    frequency: NonZeroU64,
}
pub struct Timer {
    frequency: NonZeroU64,
    interval: AtomicU64,
}
static TIMER: LazyLock<Timer> = LazyLock::new();
impl Config {
    pub fn probe(fdt: &Fdt) -> Result<Option<Self>, Error> {
        if !discovery::sstc_supported(fdt)? {
            return Ok(None);
        }
        let frequency = fdt
            .lookup("/cpus")
            .prop("timebase-frequency")
            .and_then(|v| v.into_scalar_u64())
            .and_then(NonZeroU64::new)
            .ok_or("missing CPU timebase-frequency")?;
        Ok(Some(Self { frequency }))
    }
    pub fn install(self) -> &'static dyn TimerDevice {
        TIMER.get_or_init(|| Timer {
            frequency: self.frequency,
            interval: AtomicU64::new(0),
        })
    }
}
impl Timer {
    fn rearm(&self) {
        set_deadline(ticks().saturating_add(self.interval.load(Ordering::Relaxed)));
    }
}
impl TimerDevice for Timer {
    fn ticks(&self) -> u64 {
        ticks()
    }
    fn frequency(&self) -> NonZeroU64 {
        self.frequency
    }
    fn start_periodic(&self, hz: NonZeroU32) {
        self.interval.store(
            (self.frequency.get() / u64::from(hz.get())).max(1),
            Ordering::Relaxed,
        );
        self.rearm();
        allow_timer();
    }
    fn take_event(&self) -> bool {
        self.rearm();
        true
    }
}

pub const DRIVER: boot::drivers::TimerDriver = boot::drivers::TimerDriver {
    probe: |fdt| Ok(Config::probe(fdt)?.is_some()),
    install: |fdt, _mmio| {
        let config = Config::probe(fdt)?.ok_or("timer disappeared after discovery")?;
        Ok(boot::drivers::InstalledTimer {
            device: config.install(),
            irq: None,
        })
    },
};
