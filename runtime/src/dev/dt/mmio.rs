//! Helpers for interpreting MMIO device nodes in a device tree.
//!
//! The node's `reg` property is decoded using its parent's address and size
//! cell widths. Tuples that cannot be represented by the target are returned
//! as errors.

use core::num::NonZeroUsize;

use crate::dev::dt::reg::{CompatibleRegIter, IncompatibleRegError};
use crate::dev::dt::{Fdt, FdtWalker};

/// A device-tree MMIO node and its decoded registers.
pub struct Mmio<'a> {
    compatible: &'a str,
    reg_iter: CompatibleRegIter<'a>,
}

impl<'a> Mmio<'a> {
    /// Look up and decode the MMIO node at `path`.
    pub fn new(fdt: &'a Fdt, path: &str) -> Option<Self> {
        Self::from_walker(fdt.lookup(path))
    }

    /// Decode an MMIO node at the current walker position.
    pub fn from_walker(walker: FdtWalker<'a>) -> Option<Self> {
        let (address_cells, size_cells) = walker.reg_cells();
        let mut compatible = None;
        let mut reg_iter = None;
        for (name, value) in walker.props() {
            match name {
                "compatible" => compatible = value.into_str(),
                "reg" => reg_iter = Some(value.into_reg(address_cells, size_cells)),
                _ => {}
            }
        }

        Some(Self {
            compatible: compatible?,
            reg_iter: reg_iter?.into(),
        })
    }

    /// Return the preferred entry in the node's `compatible` string-list.
    pub const fn compatible(&self) -> &'a str {
        self.compatible
    }

    pub fn reg_iter(&'a self) -> CompatibleRegIter<'a> {
        self.reg_iter.clone()
    }
}

impl Iterator for Mmio<'_> {
    type Item = Result<(usize, NonZeroUsize), IncompatibleRegError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.reg_iter.next()
    }
}

#[cfg(test)]
mod tests {
    use std::vec::Vec;

    use super::*;
    use crate::dev::dt::tests::qemu_fdt;

    #[test]
    fn qemu_virt_mmio_decodes_serial_node() {
        let fdt = qemu_fdt();
        let mut serial = Mmio::new(&fdt, "/soc/serial@10000000").unwrap();

        assert_eq!(serial.compatible(), "ns16550a");
        assert_eq!(
            serial
                .next()
                .transpose()
                .unwrap()
                .map(|(addr, size)| (addr, size.get())),
            Some((0x10000000, 0x100))
        );
        assert!(serial.next().is_none());
    }

    #[test]
    fn qemu_virt_mmio_decodes_multiple_flash_regions() {
        let fdt = qemu_fdt();
        let flash = Mmio::new(&fdt, "/flash@20000000").unwrap();

        let regions: Vec<_> = flash
            .map(|reg| reg.map(|(addr, size)| (addr, size.get())))
            .collect::<Result<_, _>>()
            .unwrap();

        assert_eq!(
            regions,
            [(0x20000000, 0x02000000), (0x22000000, 0x02000000),]
        );
    }

    #[test]
    fn missing_mmio_node_returns_none() {
        let fdt = qemu_fdt();

        assert!(Mmio::new(&fdt, "/missing").is_none());
    }
}
