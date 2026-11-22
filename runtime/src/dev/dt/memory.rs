//! Physical RAM ranges and fixed memory reservations in a device tree.
//!
//! [`MemoryIter`] yields the full RAM banks described by `/memory` nodes.
//! [`reservations`] yields entries from the DTB memory reservation block and
//! fixed `reg` ranges from enabled children of `/reserved-memory`. The two
//! sources can overlap. A child's `no-map` property marks its range for
//! exclusion from the kernel's ordinary RAM mappings.
//!
//! See [Devicetree Specification v0.4], sections 2.3.6 (`reg`), 3.4
//! (`/memory`), 3.5 (`/reserved-memory`), and 5.3 (memory reservation block).
//!
//! [Devicetree Specification v0.4]:
//!     https://github.com/devicetree-org/devicetree-specification/releases/download/v0.4/devicetree-specification-v0.4.pdf

use core::num::NonZeroUsize;

use crate::dev::dt::reg::{CompatibleRegIter, IncompatibleRegError};
use crate::dev::dt::{Fdt, FdtToken, FdtWalker};

/// Iterator over physical RAM ranges declared by `/memory*` nodes.
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

/// A fixed firmware reservation, in physical bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reservation {
    pub base: u64,
    pub size: u64,
    pub no_map: bool,
}

/// Read the DTB reservation block and the enabled static children of
/// `/reserved-memory`. Both sources may describe the same physical bytes.
pub fn reservations(fdt: &Fdt) -> impl Iterator<Item = Reservation> + '_ {
    let header = fdt
        .reservation_entries()
        .iter()
        .map(|entry| Reservation {
            base: u64::from_be_bytes(entry[..8].try_into().unwrap()),
            size: u64::from_be_bytes(entry[8..].try_into().unwrap()),
            no_map: false,
        })
        .take_while(|r| r.base != 0 || r.size != 0)
        .filter(|r| r.size != 0);

    let mut parent = fdt.lookup("/reserved-memory");
    let depth = parent.depth();
    let enabled = |node: FdtWalker<'_>| {
        node.prop("status")
            .is_none_or(|v| matches!(v.into_str(), Some("ok" | "okay")))
    };
    let active = enabled(parent.clone());
    let children = core::iter::from_fn(move || {
        if !active {
            return None;
        }
        while let Some(token) = parent.next() {
            if matches!(token, FdtToken::Node(_))
                && parent.depth() == depth + 1
                && enabled(parent.clone())
            {
                let (ac, sc) = parent.reg_cells();
                if let Some(reg) = parent.clone().prop("reg") {
                    let no_map = parent.clone().prop("no-map").is_some();
                    return Some(reg.into_reg(ac, sc).map(move |(base, size)| Reservation {
                        base,
                        size: size.expect("zero reserved-memory size").get(),
                        no_map,
                    }));
                }
                assert!(
                    parent.clone().prop("size").is_none(),
                    "dynamic reserved-memory allocation is unsupported"
                );
            }
        }
        None
    })
    .flatten();
    header.chain(children)
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
