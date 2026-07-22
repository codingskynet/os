//! Console device interface.

use core::convert::Infallible;
use core::num::NonZeroUsize;
use core::str;

use crate::dev::dt::Fdt;
use crate::dev::dt::reg::{CompatibleRegIter, IncompatibleRegError};
use crate::dev::uart::{Read, Write};

/// Byte-stream device suitable for use as the kernel console backend.
pub trait Console: Read<Error = Infallible> + Write<Error = Infallible> + Send {
    /// Enable notification when received data becomes available.
    fn enable_rx_interrupt(&mut self);

    /// Consume all currently available input in arrival order.
    ///
    /// Interrupt handlers use this to clear a level-triggered receive source
    /// even when the kernel's software input buffer is full.
    fn drain_rx(&mut self, receive: &mut dyn FnMut(u8));
}

/// Console MMIO and interrupt wiring selected by `/chosen/stdout-path`.
pub struct ConsoleConfig<'a> {
    path: &'a str,
    compatible: &'a [u8],
    base: usize,
    size: NonZeroUsize,
    interrupt_parent: Option<u32>,
    interrupt: Option<u32>,
}

impl<'a> ConsoleConfig<'a> {
    /// Parse the selected console node once and retain its device properties.
    pub fn from_fdt(fdt: &'a Fdt) -> Result<Self, Error> {
        Self::from_path(fdt, Self::console_path(fdt)?)
    }

    /// Resolve `/chosen/stdout-path` and strip an optional options suffix.
    pub fn console_path(fdt: &'a Fdt) -> Result<&'a str, Error> {
        let stdout_path = fdt
            .lookup("/chosen")
            .prop("stdout-path")
            .ok_or(Error::MissingStdout)?
            .into_str()
            .ok_or(Error::InvalidPath)?;

        Self::resolve_console_path(fdt, stdout_path)
    }

    /// Strip console options and resolve a path alias through `/aliases`.
    fn resolve_console_path<'fdt>(
        fdt: &'fdt Fdt,
        stdout_path: &'fdt str,
    ) -> Result<&'fdt str, Error> {
        let path = stdout_path
            .split(':')
            .next()
            .filter(|path| !path.is_empty())
            .ok_or(Error::InvalidPath)?;

        if path.starts_with('/') {
            return Ok(path);
        }

        // Alias values are complete node paths, so resolving them does not
        // require allocating a new string during early boot.
        fdt.lookup("/aliases")
            .prop(path)
            .and_then(|value| value.into_str())
            .filter(|path| path.starts_with('/'))
            .ok_or(Error::InvalidPath)
    }

    /// Parse the selected console node's properties in one walk.
    fn from_path(fdt: &'a Fdt, path: &'a str) -> Result<Self, Error> {
        let console = fdt.lookup(path);
        let (address_cells, size_cells) = console.reg_cells();
        let mut compatible = None;
        let mut reg = None;
        let mut interrupt_parent = None;
        let mut interrupt = None;

        for (name, value) in console.props() {
            match name {
                "compatible" => compatible = Self::parse_compatible(value.into_slice()),
                "reg" => {
                    reg = CompatibleRegIter::from(value.into_reg(address_cells, size_cells))
                        .next()
                        .transpose()?;
                }
                "interrupt-parent" => interrupt_parent = value.into_scalar_u32(),
                "interrupts" => interrupt = value.into_scalar_u32(),
                _ => {}
            }
        }

        let Some((base, size)) = reg else {
            return Err(Error::InvalidMmio);
        };
        Ok(Self {
            path,
            compatible: compatible.ok_or(Error::InvalidMmio)?,
            base,
            size,
            interrupt_parent,
            interrupt,
        })
    }

    pub const fn path(&self) -> &str {
        self.path
    }

    /// Return compatible strings in device-tree preference order.
    pub fn compatibles(&self) -> impl Iterator<Item = &str> {
        self.compatible[..self.compatible.len() - 1]
            .split(|byte| *byte == 0)
            .map(|compatible| str::from_utf8(compatible).unwrap())
    }

    /// Return the most-specific compatible string.
    pub fn compatible(&self) -> &str {
        self.compatibles().next().unwrap()
    }

    /// Return the raw physical MMIO address and host-representable size.
    pub const fn reg(&self) -> (usize, NonZeroUsize) {
        (self.base, self.size)
    }

    /// Return the direct interrupt-controller phandle and interrupt specifier.
    ///
    /// `None` indicates that either property is absent or malformed.
    pub fn interrupt(&self) -> Option<(u32, u32)> {
        self.interrupt_parent.zip(self.interrupt)
    }

    fn parse_compatible(value: &'a [u8]) -> Option<&'a [u8]> {
        let entries = value.strip_suffix(&[0])?;
        (!entries.is_empty()
            && entries
                .split(|byte| *byte == 0)
                .all(|entry| !entry.is_empty() && str::from_utf8(entry).is_ok()))
        .then_some(value)
    }
}

/// Failure while discovering the selected console device.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("missing /chosen/stdout-path")]
    MissingStdout,
    #[error("invalid console path")]
    InvalidPath,
    #[error("invalid console MMIO node")]
    InvalidMmio,
    #[error(transparent)]
    Reg(#[from] IncompatibleRegError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::dt::tests::qemu_fdt;

    #[test]
    fn qemu_virt_console_config_decodes_mmio_and_interrupt() {
        let fdt = qemu_fdt();
        let config = ConsoleConfig::from_fdt(&fdt).unwrap();

        assert_eq!(config.path(), "/soc/serial@10000000");
        assert_eq!(config.compatible(), "ns16550a");
        let (base, size) = config.reg();
        assert_eq!((base, size.get()), (0x1000_0000, 0x100));
        assert_eq!(config.interrupt(), Some((3, 10)));
    }

    #[test]
    fn console_path_resolves_alias_before_options() {
        let fdt = qemu_fdt();

        assert_eq!(
            ConsoleConfig::resolve_console_path(&fdt, "serial0:115200n8").unwrap(),
            "/soc/serial@10000000"
        );
        assert_eq!(
            ConsoleConfig::resolve_console_path(&fdt, "/soc/serial@10000000:115200n8").unwrap(),
            "/soc/serial@10000000"
        );
    }

    #[test]
    fn compatible_list_preserves_all_entries() {
        let compatible = ConsoleConfig::parse_compatible(b"vendor,uart\0ns16550a\0").unwrap();

        let config = ConsoleConfig {
            path: "/serial",
            compatible,
            base: 0,
            size: NonZeroUsize::new(1).unwrap(),
            interrupt_parent: None,
            interrupt: None,
        };

        assert_eq!(
            config.compatibles().collect::<std::vec::Vec<_>>(),
            ["vendor,uart", "ns16550a"]
        );
        assert_eq!(config.compatible(), "vendor,uart");
    }

    #[test]
    fn malformed_compatible_lists_are_rejected() {
        assert!(ConsoleConfig::parse_compatible(b"").is_none());
        assert!(ConsoleConfig::parse_compatible(b"ns16550a").is_none());
        assert!(ConsoleConfig::parse_compatible(b"vendor,uart\0\0ns16550a\0").is_none());
        assert!(ConsoleConfig::parse_compatible(b"\xff\0").is_none());
    }
}
