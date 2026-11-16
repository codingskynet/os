//! External interrupt dispatch installed after device discovery.
use alloc::vec::Vec;

use crate::kernel::sync::LazyLock;

/// An external interrupt controller and its device IRQ routes.
///
/// Implementations and their state must outlive boot-section reclamation.
/// Both methods run with local interrupts disabled and may run on multiple
/// harts concurrently. Implementations must not sleep or switch tasks.
pub trait InterruptController: Sync {
    /// Configure this hart's interrupt delivery and enable its device sources.
    fn init_this_hart(&self);

    /// Dispatch pending IRQs and finish controller acknowledgement/completion.
    /// Return whether scheduling is needed after all controllers are serviced.
    fn handle_pending(&self) -> bool;
}

/// The installed controllers, shared by all harts and immutable after boot.
pub struct Interrupts {
    controllers: Vec<&'static dyn InterruptController>,
}

impl Interrupts {
    /// Configure interrupt delivery on this hart with local interrupts disabled.
    pub fn init_this_hart(&self) {
        for controller in &self.controllers {
            controller.init_this_hart();
        }
        if !self.controllers.is_empty() {
            crate::arch::asm::interrupt::allow_external();
        }
    }

    /// Service every controller with local interrupts disabled, returning a
    /// scheduling request only after all IRQ acknowledgement/completion finishes.
    pub fn handle_pending(&self) -> bool {
        let mut schedule = false;
        for controller in &self.controllers {
            schedule |= controller.handle_pending();
        }
        schedule
    }
}

static SYSTEM: LazyLock<Interrupts> = LazyLock::new();

/// Publish the complete controller list once on the boot hart, before IRQs
/// are enabled or secondary harts are released. The list is immutable afterward.
pub fn install(controllers: Vec<&'static dyn InterruptController>) {
    assert!(
        SYSTEM.get().is_none(),
        "interrupt controllers already installed"
    );
    SYSTEM.get_or_init(|| Interrupts { controllers });
}

/// Return the interrupt system. Call after boot-time controller installation.
pub fn system() -> &'static Interrupts {
    &SYSTEM
}
