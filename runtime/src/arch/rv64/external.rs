//! Supervisor external interrupt routing through the platform PLIC.
//!
//! The Platform-Level Interrupt Controller (PLIC) collects interrupt requests
//! from devices outside a RISC-V hart, assigns each source a global interrupt
//! ID and priority, and presents the highest-priority eligible request to an
//! interrupt target. A target is a *hart context*: a particular privilege mode
//! on a particular hart. Each context has its own source-enable bits and
//! priority threshold, so the same global source can be routed independently
//! to different contexts. The ratified [RISC-V PLIC specification] defines
//! this model and the standard [memory-mapped register layout].
//!
//! For supervisor delivery, the PLIC asserts the hart's supervisor external
//! interrupt. After the hart traps with `scause` code 9, software reads the
//! context's claim register. That read returns the eligible interrupt ID and
//! atomically claims it; zero means that no interrupt is available. After
//! servicing the device, software writes the same ID to the completion
//! register. See the specification's [claim] and [completion] procedures.
//!
//! Platform wiring is discovered from the flattened device tree:
//!
//! - the PLIC node's `reg` property supplies the MMIO range mapped by boot;
//! - `interrupts-extended` orders the PLIC contexts and connects each one to a
//!   hart-local interrupt controller;
//! - entries whose interrupt specifier is 9 select supervisor contexts; and
//! - `/chosen/stdout-path`, `interrupt-parent`, and `interrupts` identify the
//!   UART source routed through this PLIC.
//!
//! [`PlicConfig`] handles the controller topology, while this module resolves
//! the console UART source attached to that controller. [`ExternalMeta::init`]
//! performs this platform-wide discovery once.
//! [`init`] then programs the UART priority, enable bit, and threshold for each
//! hart's supervisor context before enabling supervisor external interrupts.
//! [`handle`] implements the claim, service, and completion loop. At present,
//! UART receive is the only supported external interrupt source.
//!
//! [RISC-V PLIC specification]: https://docs.riscv.org/reference/plic/v1.0.0/index.html
//! [memory-mapped register layout]: https://docs.riscv.org/reference/plic/v1.0.0/plic-memory-map.html
//! [claim]: https://docs.riscv.org/reference/plic/v1.0.0/plic-claims.html
//! [completion]: https://docs.riscv.org/reference/plic/v1.0.0/plic-completion.html

use crate::arch;
use crate::dev::console::ConsoleConfig;
use crate::dev::dt::Fdt;
use crate::dev::plic::{Error as PlicError, Plic, PlicConfig};
use crate::kernel::console::Console;
use crate::kernel::per_core::PerCore;
use crate::kernel::scheduler::Scheduler;
use crate::kernel::sync::LazyLock;
use crate::mm::addr::Pa;

/// Supervisor external interrupt code in a hart-local interrupt specifier.
///
/// This is the same architectural code reported by `scause` for a supervisor
/// external interrupt. See the [RISC-V `scause` register] definition.
///
/// [RISC-V `scause` register]: https://docs.riscv.org/reference/isa/v20260120/priv/supervisor.html#scause
const SUPERVISOR_EXTERNAL_INTERRUPT: u32 = 9;

/// Platform-wide PLIC topology discovered from the boot device tree.
///
/// This is initialized once by [`ExternalMeta::init`] before per-hart
/// interrupt initialization starts.
static EXTERNAL_META: LazyLock<ExternalMeta> = LazyLock::new();

/// Immutable PLIC routing metadata shared by every hart.
///
/// The PLIC configuration maps a hardware hart ID to the context number whose
/// MMIO registers control supervisor delivery for that hart. Context numbers
/// are positions in the device tree's `interrupts-extended` property, not hart
/// IDs and not necessarily a dense sequence of supervisor contexts.
pub struct ExternalMeta {
    plic: Plic,
    uart_interrupt: usize,
    config: PlicConfig,
}

impl ExternalMeta {
    /// Discover and publish the platform's PLIC and UART interrupt wiring.
    ///
    /// Boot must already have mapped the complete region returned by
    /// [`PlicConfig::region_from_fdt`] into the direct map. Repeated successful
    /// calls retain the first published value.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] if the device tree lacks a supported PLIC or console
    /// UART, if their properties are malformed, or if no supervisor PLIC
    /// context can be associated with a hart.
    pub fn init(fdt: &Fdt, console: &ConsoleConfig<'_>) -> Result<(), Error> {
        let config = Config::from_fdt(fdt, console)?;
        EXTERNAL_META.get_or_init(|| Self {
            // SAFETY: boot maps the complete PLIC region at its direct-map
            // virtual address before any interrupt-controller access occurs.
            plic: unsafe { Plic::new(Pa::new(config.plic.region().0).into_va().as_raw()) },
            uart_interrupt: config.uart_interrupt,
            config: config.plic,
        });
        Ok(())
    }

    /// Look up the PLIC supervisor-context number assigned to `hart_id`.
    fn context(&self, hart_id: usize) -> usize {
        self.config
            .context(hart_id)
            .unwrap_or_else(|| panic!("PLIC has no supervisor context for hart {hart_id}"))
    }
}

/// Enable the UART source for the calling hart's supervisor PLIC context.
///
/// The source receives priority 1, the lowest active priority. Its enable bit
/// is set only in this hart's supervisor context, and threshold 0 admits every
/// active priority. These values follow the PLIC rules for [priorities],
/// [per-context enables], and [thresholds]. The function then enables UART RX
/// interrupts at the device and supervisor external interrupts at the hart.
///
/// # Panics
///
/// Panics if [`ExternalMeta::init`] has not completed or the calling hart has
/// no supervisor context in the device tree.
///
/// [priorities]: https://docs.riscv.org/reference/plic/v1.0.0/plic-priority.html
/// [per-context enables]: https://docs.riscv.org/reference/plic/v1.0.0/plic-enables.html
/// [thresholds]: https://docs.riscv.org/reference/plic/v1.0.0/plic-thresholds.html
pub fn init() {
    let context = EXTERNAL_META.context(PerCore::core_id());
    EXTERNAL_META
        .plic
        .set_priority(EXTERNAL_META.uart_interrupt, 1);
    EXTERNAL_META
        .plic
        .enable(context, EXTERNAL_META.uart_interrupt);
    EXTERNAL_META.plic.set_threshold(context, 0);
    Console::enable_rx_interrupt();
    arch::asm::interrupt::allow_external();
}

/// Claim and service all external interrupts pending for the calling hart.
///
/// A claim returns the highest-priority eligible source for this supervisor
/// context and marks it in service. Each nonzero ID must be completed after
/// its device handler runs, so this loop always writes completion before
/// checking whether the source is supported. It repeats until claim returns
/// zero because multiple requests may be pending behind the interrupt that
/// caused the trap.
///
/// UART service drains received bytes into the console buffer and makes one
/// blocked console reader runnable. Scheduling is deferred until every pending
/// PLIC request has been completed.
///
/// # Panics
///
/// Panics if the claimed source is not the configured console UART, or if the
/// calling hart has no discovered supervisor context.
pub fn handle() {
    let context = EXTERNAL_META.context(PerCore::core_id());
    let mut woke_reader = false;
    loop {
        let interrupt = EXTERNAL_META.plic.claim(context);
        if interrupt == 0 {
            break;
        }

        if interrupt == EXTERNAL_META.uart_interrupt {
            woke_reader = Console::handle_rx_interrupt();
        }
        EXTERNAL_META.plic.complete(context, interrupt);

        assert_eq!(
            interrupt, EXTERNAL_META.uart_interrupt,
            "unhandled PLIC interrupt"
        );
    }

    if woke_reader {
        Scheduler::run_next();
    }
}

/// Validated device-tree configuration used to construct [`ExternalMeta`].
struct Config {
    plic: PlicConfig,
    uart_interrupt: usize,
}

impl Config {
    /// Decode the PLIC region, supervisor contexts, and console UART source.
    #[inline(never)]
    fn from_fdt(fdt: &Fdt, console: &ConsoleConfig<'_>) -> Result<Self, Error> {
        let plic = PlicConfig::from_fdt(fdt, SUPERVISOR_EXTERNAL_INTERRUPT)?;
        let uart_interrupt = plic.interrupt(console).ok_or(Error::InvalidUart)?;

        Ok(Self {
            plic,
            uart_interrupt: uart_interrupt as usize,
        })
    }
}

/// Failure while discovering PLIC or console interrupt wiring.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No node advertises a supported PLIC compatible string, or its MMIO
    /// `reg` tuple is absent.
    #[error("plic")]
    Plic(#[from] PlicError),

    /// The console UART interrupt properties are malformed or refer to a
    /// different interrupt controller.
    #[error("invalid uart")]
    InvalidUart,
}
