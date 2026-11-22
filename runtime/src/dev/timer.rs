//! Hardware timer selection. Kernel tick policy lives in `kernel::tick`.
use core::num::{NonZeroU32, NonZeroU64};

/// A clock source and periodic interrupt device with a common timebase.
///
/// Implementations and their state must outlive boot-section reclamation.
/// Boot installs the device before clock initialization. `start_periodic` runs
/// with local interrupts disabled; interrupt acknowledgement runs in trap context.
pub trait TimerDevice: Sync {
    fn ticks(&self) -> u64;
    fn frequency(&self) -> NonZeroU64;
    fn start_periodic(&self, hz: NonZeroU32);
    /// Acknowledge/rearm the device, returning whether a tick was pending.
    /// External IRQ dispatchers call this only for this device's IRQ source.
    fn take_event(&self) -> bool;
}

use crate::kernel::sync::LazyLock;

static DEVICE: LazyLock<&'static dyn TimerDevice> = LazyLock::new();

/// Call once on the boot hart, after MMIO mapping and before clock init.
pub fn install(device: &'static dyn TimerDevice) {
    assert!(DEVICE.get().is_none(), "timer already installed");
    DEVICE.get_or_init(|| device);
}

/// Return the installed timer. Call after boot-time device installation.
pub fn device() -> &'static dyn TimerDevice {
    *DEVICE
}

/// Dispatch an external timer IRQ and report whether scheduling is needed.
/// The installed IRQ route retains this callback after boot memory is reclaimed.
pub fn handle_interrupt() -> bool {
    device().take_event() && crate::kernel::tick::on_tick()
}
