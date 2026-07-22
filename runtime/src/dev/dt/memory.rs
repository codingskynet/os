//! Helpers for extracting RAM ranges from a device tree.
//!
//! Reference: [Devicetree Specification v0.4], section 3.4, "`/memory` Node",
//! and section 2.3.6, "`reg`".
//!
//! [Devicetree Specification v0.4]:
//!     https://github.com/devicetree-org/devicetree-specification/releases/download/v0.4/devicetree-specification-v0.4.pdf

use core::num::NonZeroUsize;

use crate::dev::dt::reg::{CompatibleRegIter, IncompatibleRegError};
use crate::dev::dt::{Fdt, FdtToken, FdtWalker};

/// Iterator over usable memory ranges declared by `/memory*` nodes.
///
/// Every yielded range has an address and nonzero size representable by the
/// target. Incompatible tuples are returned as errors. Address and size cells
/// are decoded according to the parent node's `#address-cells` and
/// `#size-cells` values, as required for standard `reg` properties.
pub struct MemoryIter<'a> {
    walker: FdtWalker<'a>,
    reg_iter: Option<CompatibleRegIter<'a>>,
}

impl<'a> MemoryIter<'a> {
    pub fn new(fdt: &'a Fdt) -> Self {
        Self {
            walker: fdt.query(),
            reg_iter: None,
        }
    }
}

impl<'a> Iterator for MemoryIter<'a> {
    type Item = Result<(usize, NonZeroUsize), IncompatibleRegError>;

    fn next(&mut self) -> Option<Self::Item> {
        fn find_memory<'a>(walker: &mut FdtWalker<'a>) -> Option<CompatibleRegIter<'a>> {
            let mut depth = 0;
            let mut is_node = false;
            let mut is_memory = false;
            let mut reg = None;

            while let Some(token) = walker.next() {
                match token {
                    FdtToken::Node(name) => {
                        if depth == 0 && name.split('@').next() == Some("memory") {
                            is_node = true;
                            reg = None;
                        }
                        depth += 1;
                    }
                    FdtToken::NodeEnd => {
                        if depth == 0 {
                            return None;
                        }

                        if is_node && depth == 1 {
                            if is_memory && reg.is_some() {
                                return reg;
                            }
                            is_node = false;
                            is_memory = false;
                            reg = None;
                        }
                        depth -= 1;
                    }
                    FdtToken::Prop { name, value } if is_node && depth == 1 => match name {
                        "device_type" => {
                            if value.into_slice() == b"memory\0" {
                                is_memory = true;
                            }
                        }
                        "reg" => {
                            let (address_cells, size_cells) = walker.reg_cells();
                            reg = Some(value.into_reg(address_cells, size_cells).into());
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }

            None
        }

        loop {
            if let Some(reg) = self.reg_iter.as_mut().and_then(Iterator::next) {
                return Some(reg);
            }

            self.reg_iter = Some(find_memory(&mut self.walker)?);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::dt::tests::qemu_fdt;

    #[test]
    fn qemu_virt_memory_iter_returns_none_after_last_range() {
        let fdt = qemu_fdt();
        let mut iter = MemoryIter::new(&fdt);

        assert_eq!(
            iter.next()
                .transpose()
                .unwrap()
                .map(|(addr, size)| (addr, size.get())),
            Some((0x80000000, 0x08000000))
        );
        assert!(iter.next().is_none());
    }
}
