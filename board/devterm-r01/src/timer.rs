//! R-01 has a machine timer managed by OpenSBI (SBI TIME, function 0).
use core::num::{NonZeroU32, NonZeroU64};
use core::sync::atomic::{AtomicU64, Ordering};

use runtime::arch::asm::interrupt::allow_timer;
use runtime::arch::asm::time::ticks;
use runtime::dev::timer::TimerDevice;
use runtime::kernel::sync::LazyLock;

struct Timer {
    frequency: NonZeroU64,
    interval: AtomicU64,
}

impl Timer {
    fn rearm(&self) {
        let deadline = ticks().saturating_add(self.interval.load(Ordering::Relaxed));
        let error: isize;
        unsafe {
            core::arch::asm!("ecall", in("a7") 0x54494d45usize, in("a6") 0usize,
                inlateout("a0") deadline => error, lateout("a1") _, options(nostack));
        }
        assert_eq!(error, 0, "SBI set_timer failed");
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
        // Deliver input to the console buffer and wake a blocked reader.
        runtime::kernel::console::Console::handle_rx_interrupt();
        true
    }
}
pub const DRIVER: boot::drivers::TimerDriver = boot::drivers::TimerDriver {
    probe: |fdt| {
        Ok(runtime::dev::discovery::compatible(
            fdt.query(),
            "allwinner,sun20i-d1",
        ))
    },
    install: |fdt, _| {
        static TIMER: LazyLock<Timer> = LazyLock::new();
        let frequency = fdt
            .lookup("/cpus")
            .prop("timebase-frequency")
            .and_then(|v| v.into_scalar_u64())
            .and_then(NonZeroU64::new)
            .ok_or("invalid CPU timebase-frequency")?;
        Ok(boot::drivers::InstalledTimer {
            device: TIMER.get_or_init(|| Timer {
                frequency,
                interval: AtomicU64::new(0),
            }),
            irq: None,
        })
    },
};
