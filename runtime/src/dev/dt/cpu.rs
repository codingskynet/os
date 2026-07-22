//! Helpers for extracting enabled CPUs from a device tree.
//!
//! Reference: [Devicetree Specification v0.4], section 3.7, "`/cpus` Node".
//!
//! [Devicetree Specification v0.4]:
//!     https://github.com/devicetree-org/devicetree-specification/releases/download/v0.4/devicetree-specification-v0.4.pdf

use crate::dev::dt::{Fdt, FdtToken, FdtWalker};

/// The enabled CPUs declared below the `/cpus` node.
#[derive(Clone, Copy)]
pub struct Cpus<'a> {
    fdt: &'a Fdt,
}

impl<'a> Cpus<'a> {
    pub const fn new(fdt: &'a Fdt) -> Self {
        Self { fdt }
    }

    /// Return the number of enabled, well-formed CPU nodes.
    pub fn count(&self) -> usize {
        self.iter().count()
    }

    pub fn iter(&self) -> CpuIter<'a> {
        CpuIter::new(self.fdt)
    }
}

impl<'a> IntoIterator for Cpus<'a> {
    type Item = Cpu;
    type IntoIter = CpuIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> IntoIterator for &Cpus<'a> {
    type Item = Cpu;
    type IntoIter = CpuIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// One enabled CPU and its local interrupt controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cpu {
    hart_id: usize,
    interrupt_controller: Option<u32>,
}

impl Cpu {
    pub const fn hart_id(self) -> usize {
        self.hart_id
    }

    pub const fn interrupt_controller(self) -> Option<u32> {
        self.interrupt_controller
    }
}

/// Iterator over enabled CPU nodes below `/cpus`.
pub struct CpuIter<'a> {
    walker: FdtWalker<'a>,
}

impl<'a> CpuIter<'a> {
    pub fn new(fdt: &'a Fdt) -> Self {
        Self {
            walker: fdt.lookup("/cpus"),
        }
    }
}

impl Iterator for CpuIter<'_> {
    type Item = Cpu;

    fn next(&mut self) -> Option<Self::Item> {
        let mut depth = 0;
        let mut is_cpu = false;
        let mut is_enabled = true;
        let mut hart_id = None;
        let mut interrupt_controller = None;
        let mut is_interrupt_controller = false;
        let mut interrupt_controller_phandle = None;

        for token in self.walker.by_ref() {
            match token {
                FdtToken::Node(_) => {
                    if depth == 0 {
                        is_cpu = false;
                        is_enabled = true;
                        hart_id = None;
                        interrupt_controller = None;
                    } else if depth == 1 {
                        is_interrupt_controller = false;
                        interrupt_controller_phandle = None;
                    }
                    depth += 1;
                }
                FdtToken::NodeEnd => {
                    if depth == 0 {
                        return None;
                    }

                    if depth == 2 && is_interrupt_controller {
                        interrupt_controller = interrupt_controller_phandle;
                    }
                    if depth == 1 {
                        depth -= 1;
                        if let (true, true, Some(hart_id)) = (is_cpu, is_enabled, hart_id) {
                            return Some(Cpu {
                                hart_id,
                                interrupt_controller,
                            });
                        }
                        continue;
                    }
                    depth -= 1;
                }
                FdtToken::Prop { name, value } if depth == 1 => match name {
                    "device_type" => is_cpu = value.into_str() == Some("cpu"),
                    "status" => {
                        is_enabled = matches!(value.into_str(), Some("okay" | "ok"));
                    }
                    "reg" => {
                        hart_id = value
                            .into_scalar_u64()
                            .and_then(|hart_id| usize::try_from(hart_id).ok());
                    }
                    _ => {}
                },
                FdtToken::Prop { name, value } if depth == 2 => match name {
                    "interrupt-controller" => is_interrupt_controller = true,
                    "phandle" | "linux,phandle" => {
                        interrupt_controller_phandle = value
                            .into_scalar_u64()
                            .and_then(|phandle| u32::try_from(phandle).ok());
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        None
    }
}

impl Fdt {
    pub fn cpus(&self) -> Cpus<'_> {
        Cpus::new(self)
    }
}

#[cfg(test)]
mod tests {
    use std::vec::Vec;

    use super::*;
    use crate::dev::dt::tests::qemu_fdt;

    #[test]
    fn qemu_virt_cpus_finds_enabled_harts() {
        let fdt = qemu_fdt();

        let cpus: Vec<_> = fdt.cpus().into_iter().collect();

        assert_eq!(
            cpus,
            [Cpu {
                hart_id: 0,
                interrupt_controller: Some(2),
            }]
        );
        assert_eq!(fdt.cpus().count(), cpus.len());
    }

    #[test]
    fn qemu_virt_cpu_iter_returns_none_after_last_cpu() {
        let fdt = qemu_fdt();
        let mut iter = fdt.cpus().iter();

        assert_eq!(iter.next().map(Cpu::hart_id), Some(0));
        assert_eq!(iter.next(), None);
    }
}
