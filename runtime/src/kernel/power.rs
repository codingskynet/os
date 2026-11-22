//! System-wide halt and reboot policy.

use alloc::boxed::Box;
use core::num::NonZeroUsize;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::arch::asm::interrupt;
use crate::kernel::clock;
use crate::kernel::console::Console;
use crate::kernel::sync::LazyLock;

pub static POWER: Power = Power {
    halted: AtomicBool::new(false),
    reset: LazyLock::new(),
    power_off: LazyLock::new(),
};

type PowerRequest = dyn Fn() -> Result<(), &'static str> + Send + Sync;

/// Board power callbacks and the halt request observed by all online harts.
pub struct Power {
    halted: AtomicBool,
    reset: LazyLock<Box<PowerRequest>>,
    power_off: LazyLock<Box<PowerRequest>>,
}

impl Power {
    /// Install the board's reboot callback during boot.
    pub fn install_reset(
        &self,
        request: impl Fn() -> Result<(), &'static str> + Send + Sync + 'static,
    ) {
        self.reset.get_or_init(|| Box::new(request));
    }

    /// Keep the first power-off callback installed during boot. A board PMIC
    /// can provide this operation before boot installs a register fallback.
    pub fn install_power_off(
        &self,
        request: impl Fn() -> Result<(), &'static str> + Send + Sync + 'static,
    ) {
        self.power_off.get_or_init(|| Box::new(request));
    }

    /// Request a board reset
    pub fn reboot(&self) -> NonZeroUsize {
        const ERR: NonZeroUsize = NonZeroUsize::new(1).unwrap();

        let Some(reset) = self.reset.get() else {
            return ERR;
        };
        crate::printlnk!("Rebooting.");
        Console::flush_output();
        if reset().is_ok() {
            // Allow the hardware to assert reset before reporting failure.
            let start = clock::clock_millis();
            while clock::clock_millis().wrapping_sub(start) < 100 {
                core::hint::spin_loop();
            }
        }
        ERR
    }

    pub(crate) fn halt_init(&self, status: isize) -> ! {
        let power_off = self.power_off.get();
        let action = if power_off.is_some() {
            "Powering off."
        } else {
            "System halted."
        };
        crate::printlnk!("init (PID 1) exited with status {status}. {action}");
        Console::flush_output();
        self.halted.store(true, Ordering::Release);
        if let Some(request) = power_off
            && let Err(error) = request()
        {
            crate::printlnk!("Power-off failed: {error}. System halted.");
            Console::flush_output();
        }
        stop_hart()
    }

    // Every online hart takes scheduling ticks, including while idle.
    pub(crate) fn stop_if_halted(&self) {
        if self.halted.load(Ordering::Acquire) {
            stop_hart();
        }
    }
}

fn stop_hart() -> ! {
    interrupt::disable();
    // Mask pending per-source interrupts as well so WFI can stay asleep.
    unsafe { core::arch::asm!("csrw sie, zero", options(nostack)) };
    loop {
        interrupt::wait();
    }
}
