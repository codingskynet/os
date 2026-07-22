//! SiFive-compatible platform-level interrupt controller (PLIC).
//!
//! The PLIC collects interrupt requests from devices, assigns each source a
//! global interrupt ID and priority, and presents eligible requests to hart
//! contexts. This module provides both the MMIO register interface and the
//! device-tree discovery needed to associate hardware hart IDs with contexts.

extern crate alloc;

use alloc::vec::Vec;
use core::num::NonZeroUsize;
use core::ptr;

use crate::dev::console::ConsoleConfig;
use crate::dev::dt::mmio::Mmio;
use crate::dev::dt::reg::IncompatibleRegError;
use crate::dev::dt::util::{DtError, FdtWalkeraExt, ValueaExt};
use crate::dev::dt::{Fdt, FdtWalker};

/// Device-tree compatible strings accepted for the legacy RISC-V PLIC.
const COMPATIBLES: &[&str] = &["sifive,plic-1.0.0", "riscv,plic0"];

const PRIORITY_BASE: usize = 0x000000;
const ENABLE_BASE: usize = 0x002000;
const ENABLE_CONTEXT_STRIDE: usize = 0x80;
const CONTEXT_BASE: usize = 0x200000;
const CONTEXT_STRIDE: usize = 0x1000;
const CONTEXT_THRESHOLD: usize = 0x0;
const CONTEXT_CLAIM_COMPLETE: usize = 0x4;

/// PLIC topology discovered from a flattened device tree.
///
/// Context numbers are positions in the PLIC node's `interrupts-extended`
/// property. `contexts` contains only entries whose hart-local interrupt
/// specifier matches the value requested from [`Self::from_fdt`].
pub struct PlicConfig {
    base: usize,
    size: NonZeroUsize,
    phandle: u32,
    contexts: Vec<(usize, usize)>,
}

impl PlicConfig {
    /// Discover a PLIC and the hart contexts for `target_interrupt`.
    ///
    /// For example, an RV64 supervisor external interrupt caller passes the
    /// architectural interrupt code 9. Keeping that selection at the caller
    /// lets this device driver describe machine or supervisor contexts without
    /// depending on an architecture-specific trap module.
    #[inline(never)]
    #[allow(clippy::large_stack_frames)]
    pub fn from_fdt(fdt: &Fdt, target_interrupt: u32) -> Result<Self, Error> {
        let plic = Self::node(fdt)?;
        let (base, size) = Self::region_from_node(plic.clone()).map_err(Error::Reg)?;
        let phandle = plic
            .clone()
            .prop_or_err("phandle")
            .and_then(|value| value.into_u32_or_err())
            .map_err(Error::Phandle)?;
        let extended = plic
            .prop_or_err("interrupts-extended")
            .map_err(InterruptsExtendedError::Property)
            .map_err(Error::InterruptsExtended)?
            .into_slice();
        let contexts = Self::decode_contexts(fdt, extended, target_interrupt)
            .map_err(Error::InterruptsExtended)?;

        Ok(Self {
            base,
            size,
            phandle,
            contexts,
        })
    }

    /// Return the physical MMIO region of the first supported PLIC node.
    ///
    /// This lightweight form is used while constructing the direct map,
    /// before the complete interrupt topology needs to be decoded.
    pub fn region_from_fdt(fdt: &Fdt) -> Result<(usize, NonZeroUsize), Error> {
        Self::region_from_node(Self::node(fdt)?).map_err(Error::Reg)
    }

    /// Return the physical MMIO region described by the PLIC node.
    pub const fn region(&self) -> (usize, NonZeroUsize) {
        (self.base, self.size)
    }

    /// Return the PLIC node's device-tree phandle.
    pub const fn phandle(&self) -> u32 {
        self.phandle
    }

    /// Resolve the console's single-cell global interrupt ID on this PLIC.
    ///
    /// The console must name this controller directly through
    /// `interrupt-parent`. Multi-cell interrupt specifiers are not supported
    /// by the legacy PLIC binding handled here.
    pub fn interrupt(&self, console: &ConsoleConfig<'_>) -> Option<u32> {
        let (parent, interrupt) = console.interrupt()?;
        (parent == self.phandle).then_some(interrupt)
    }

    /// Look up the selected PLIC context assigned to `hart_id`.
    pub fn context(&self, hart_id: usize) -> Option<usize> {
        self.contexts
            .iter()
            .find_map(|&(hart, context)| (hart == hart_id).then_some(context))
    }

    /// Return all discovered `(hart ID, context number)` associations.
    pub fn contexts(&self) -> &[(usize, usize)] {
        &self.contexts
    }

    /// Locate the first PLIC node whose compatible list this driver supports.
    fn node<'a>(fdt: &'a Fdt) -> Result<FdtWalker<'a>, Error> {
        COMPATIBLES
            .iter()
            .find_map(|compatible| fdt.find_compatible(compatible))
            .ok_or(Error::Missing)
    }

    /// Decode the first MMIO tuple from an already-located PLIC node.
    fn region_from_node(plic: FdtWalker<'_>) -> Result<(usize, NonZeroUsize), RegError> {
        Mmio::from_walker(plic)
            .and_then(|mut mmio| mmio.next())
            .transpose()?
            .ok_or(RegError::Missing)
    }

    /// Decode matching hart contexts from `interrupts-extended`.
    ///
    /// The legacy RISC-V PLIC binding uses one big-endian phandle cell followed
    /// by one big-endian hart-local interrupt-specifier cell per entry. All
    /// entries remain part of the context-number index space, including those
    /// selected for other privilege modes.
    fn decode_contexts(
        fdt: &Fdt,
        extended: &[u8],
        target_interrupt: u32,
    ) -> Result<Vec<(usize, usize)>, InterruptsExtendedError> {
        let (entries, remainder) = extended.as_chunks::<8>();
        if !remainder.is_empty() {
            return Err(InterruptsExtendedError::InvalidLength(extended.len()));
        }

        let mut contexts = Vec::new();
        for (context, entry) in entries.iter().enumerate() {
            let phandle = u32::from_be_bytes(entry[..4].try_into().unwrap());
            let interrupt = u32::from_be_bytes(entry[4..].try_into().unwrap());
            if interrupt != target_interrupt {
                continue;
            }
            contexts.push((Self::hart_for_interrupt_controller(fdt, phandle)?, context));
        }

        if contexts.is_empty() {
            return Err(InterruptsExtendedError::MissingContext(target_interrupt));
        }
        Ok(contexts)
    }

    /// Resolve a hart-local interrupt-controller phandle to its parent hart ID.
    fn hart_for_interrupt_controller(
        fdt: &Fdt,
        target: u32,
    ) -> Result<usize, InterruptsExtendedError> {
        fdt.cpus()
            .into_iter()
            .find_map(|cpu| (cpu.interrupt_controller() == Some(target)).then_some(cpu.hart_id()))
            .ok_or(InterruptsExtendedError::UnknownInterruptController(target))
    }
}

/// Failure while discovering PLIC MMIO or context topology.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No node advertises a supported PLIC compatible string.
    #[error("missing PLIC node")]
    Missing,

    /// The PLIC `reg` property is absent or cannot be represented by this target.
    #[error("reg")]
    Reg(#[source] RegError),

    /// The PLIC `phandle` property is absent or malformed.
    #[error("phandle")]
    Phandle(#[source] DtError),

    /// The PLIC `interrupts-extended` property or its topology is invalid.
    #[error("interrupts-extended")]
    InterruptsExtended(#[source] InterruptsExtendedError),
}

/// Failure while decoding the PLIC's first `reg` tuple.
#[derive(Debug, thiserror::Error)]
pub enum RegError {
    /// The PLIC node has no decodable `reg` tuple.
    #[error("missing")]
    Missing,

    /// The tuple cannot be represented by this target.
    #[error(transparent)]
    Incompatible(#[from] IncompatibleRegError),
}

/// Failure while decoding PLIC context topology.
#[derive(Debug, thiserror::Error)]
pub enum InterruptsExtendedError {
    /// The `interrupts-extended` property is absent.
    #[error(transparent)]
    Property(#[from] DtError),

    /// The property does not consist of complete phandle/specifier pairs.
    #[error("invalid byte length {0}")]
    InvalidLength(usize),

    /// No context selects the requested hart-local interrupt.
    #[error("missing context for interrupt {0}")]
    MissingContext(u32),

    /// A selected context points at no enabled CPU's interrupt controller.
    #[error("unknown interrupt controller {0}")]
    UnknownInterruptController(u32),
}

/// MMIO handle for a SiFive-compatible PLIC.
#[derive(Clone, Copy)]
pub struct Plic {
    addr: usize,
}

impl Plic {
    /// Construct a PLIC handle over an already mapped MMIO region.
    ///
    /// # Safety
    ///
    /// `addr` must be the virtual base of a live SiFive-compatible PLIC MMIO
    /// region and remain mapped for every use of this handle.
    pub const unsafe fn new(addr: usize) -> Self {
        Self { addr }
    }

    pub fn set_priority(self, interrupt: usize, priority: u32) {
        unsafe { ptr::write_volatile(self.reg(PRIORITY_BASE + interrupt * 4), priority) };
    }

    pub fn enable(self, context: usize, interrupt: usize) {
        let word = interrupt / u32::BITS as usize;
        let bit = interrupt % u32::BITS as usize;
        let reg = self.reg(ENABLE_BASE + context * ENABLE_CONTEXT_STRIDE + word * 4);
        let enabled = unsafe { ptr::read_volatile(reg) };
        unsafe { ptr::write_volatile(reg, enabled | (1 << bit)) };
    }

    pub fn set_threshold(self, context: usize, threshold: u32) {
        unsafe {
            ptr::write_volatile(
                self.reg(CONTEXT_BASE + context * CONTEXT_STRIDE + CONTEXT_THRESHOLD),
                threshold,
            )
        };
    }

    pub fn claim(self, context: usize) -> usize {
        unsafe {
            ptr::read_volatile(
                self.reg(CONTEXT_BASE + context * CONTEXT_STRIDE + CONTEXT_CLAIM_COMPLETE),
            ) as usize
        }
    }

    pub fn complete(self, context: usize, interrupt: usize) {
        unsafe {
            ptr::write_volatile(
                self.reg(CONTEXT_BASE + context * CONTEXT_STRIDE + CONTEXT_CLAIM_COMPLETE),
                interrupt as u32,
            )
        };
    }

    fn reg(self, offset: usize) -> *mut u32 {
        (self.addr + offset) as *mut u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::dt::tests::qemu_fdt;

    #[test]
    fn qemu_virt_supervisor_topology_is_discovered() {
        let fdt = qemu_fdt();
        let config = PlicConfig::from_fdt(&fdt, 9).unwrap();
        let console = ConsoleConfig::from_fdt(&fdt).unwrap();

        assert_eq!(config.region().0, 0x0c00_0000);
        assert_eq!(config.phandle(), 3);
        assert_eq!(config.contexts(), [(0, 1)]);
        assert_eq!(config.context(0), Some(1));
        assert_eq!(config.context(1), None);
        assert_eq!(config.interrupt(&console), Some(10));
    }

    #[test]
    fn qemu_virt_plic_region_can_be_discovered_independently() {
        let fdt = qemu_fdt();
        let (base, size) = PlicConfig::region_from_fdt(&fdt).unwrap();

        assert_eq!((base, size.get()), (0x0c00_0000, 0x0060_0000));
    }

    #[test]
    fn missing_target_context_is_rejected() {
        let fdt = qemu_fdt();

        assert!(matches!(
            PlicConfig::from_fdt(&fdt, u32::MAX),
            Err(Error::InterruptsExtended(
                InterruptsExtendedError::MissingContext(u32::MAX)
            ))
        ));
    }
}
