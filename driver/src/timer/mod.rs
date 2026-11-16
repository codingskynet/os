//! Clock and scheduling timer driver selection and installation.
#[cfg(feature = "litex-timer")]
mod litex;
#[cfg(all(feature = "riscv-sstc", target_arch = "riscv64"))]
mod sstc;

use runtime::dev::dt::Fdt;
use runtime::mm::mmio::Mapper;

use crate::Error;

#[derive(Clone, Copy)]
pub(crate) enum Timer {
    #[cfg(feature = "litex-timer")]
    LiteX(litex::Config),
    #[cfg(all(feature = "riscv-sstc", target_arch = "riscv64"))]
    Sstc(sstc::Config),
}
impl Timer {
    pub(crate) fn probe(_fdt: &Fdt) -> Result<Self, Error> {
        #[allow(unused_mut)]
        let mut selected = None;
        #[cfg(feature = "litex-timer")]
        if let Some(config) = litex::Config::probe(_fdt)? {
            selected = Some(Self::LiteX(config));
        }
        #[cfg(all(feature = "riscv-sstc", target_arch = "riscv64"))]
        if let Some(config) = sstc::Config::probe(_fdt)? {
            if selected.is_some() {
                return Err("multiple timer backends match; timer selection policy is required");
            }
            selected = Some(Self::Sstc(config));
        }
        selected.ok_or("no enabled timer driver matches the FDT")
    }
    pub(crate) fn irq(self) -> Option<(u32, u32)> {
        match self {
            #[cfg(feature = "litex-timer")]
            Self::LiteX(config) => Some(config.irq),
            #[cfg(all(feature = "riscv-sstc", target_arch = "riscv64"))]
            Self::Sstc(_) => None,
        }
    }
    pub(crate) unsafe fn install(self, _mmio: &mut Mapper) -> Result<(), Error> {
        let irq = self.irq();
        match self {
            #[cfg(feature = "litex-timer")]
            Self::LiteX(config) => runtime::dev::timer::install(unsafe { config.install(_mmio)? }),
            #[cfg(all(feature = "riscv-sstc", target_arch = "riscv64"))]
            Self::Sstc(config) => runtime::dev::timer::install(config.install()),
        }
        if let Some((parent, irq)) = irq {
            crate::interrupt::register(parent, irq, timer_event, None)?;
        }
        Ok(())
    }
}

fn timer_event() -> bool {
    runtime::dev::timer::device().take_event() && runtime::kernel::tick::on_tick()
}
