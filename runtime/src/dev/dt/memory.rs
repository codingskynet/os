use core::num::NonZeroUsize;

use crate::dev::dt::prop::reg::RegIter;
use crate::dev::dt::{Fdt, FdtToken, FdtWalker};

pub struct MemoryIter<'a> {
    walker: FdtWalker<'a>,
    reg_iter: Option<RegIter<'a>>,
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
    type Item = (u64, NonZeroUsize);

    fn next(&mut self) -> Option<Self::Item> {
        fn find_memory<'a>(walker: &mut FdtWalker<'a>) -> Option<RegIter<'a>> {
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
                            reg = Some(value.into_reg(address_cells, size_cells));
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }

            None
        }

        loop {
            if let Some((addr, Some(size))) = self.reg_iter.as_mut().and_then(Iterator::next)
                && let Some(size) = NonZeroUsize::new(size as usize)
            {
                return Some((addr, size));
            }

            self.reg_iter = Some(find_memory(&mut self.walker)?);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::vec::Vec;

    use super::*;

    static QEMU_VIRT_DTB: &[u8] = include_bytes!("test_data/qemu_virt.dtb");

    fn qemu_fdt() -> Fdt {
        unsafe { Fdt::new(QEMU_VIRT_DTB.as_ptr()).unwrap() }
    }

    #[test]
    fn qemu_virt_memory_iter_finds_ram() {
        let fdt = qemu_fdt();

        let ranges: Vec<_> = MemoryIter::new(&fdt)
            .map(|(addr, size)| (addr, size.get()))
            .collect();

        assert_eq!(ranges, [(0x80000000, 0x08000000)]);
    }

    #[test]
    fn qemu_virt_memory_iter_returns_none_after_last_range() {
        let fdt = qemu_fdt();
        let mut iter = MemoryIter::new(&fdt);

        assert_eq!(
            iter.next().map(|(addr, size)| (addr, size.get())),
            Some((0x80000000, 0x08000000))
        );
        assert_eq!(iter.next(), None);
    }
}
