//! LiteX periodic timer/uptime device, discovered from firmware's FDT.
use core::num::{NonZeroU32, NonZeroU64, NonZeroUsize};
use core::ptr::{read_volatile, write_volatile};

use runtime::dev::discovery;
use runtime::dev::dt::mmio::Mmio;
use runtime::dev::dt::{Fdt, FdtWalker};
use runtime::dev::timer::TimerDevice;
use runtime::kernel::sync::LazyLock;
use runtime::mm::mmio::Mapper;
fn read(addr: usize) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}
fn write(addr: usize, value: u32) {
    unsafe { write_volatile(addr as *mut u32, value) }
}

pub struct Timer {
    base: usize,
    frequency: u32,
}
static CONFIG: LazyLock<Timer> = LazyLock::new();

#[derive(Clone, Copy)]
pub struct Config {
    pub region: (usize, NonZeroUsize),
    frequency: u32,
    pub irq: (u32, u32),
}
impl Config {
    pub fn probe(fdt: &Fdt) -> Result<Option<Self>, &'static str> {
        discovery::unique(fdt, |node| discovery::compatible(node, "litex,timer"))?
            .map(Self::from_node)
            .transpose()
    }
    #[inline(never)]
    fn from_node(node: FdtWalker<'_>) -> Result<Self, &'static str> {
        let region = Mmio::from_walker(node.clone())
            .and_then(|mut regs| regs.next())
            .ok_or("missing LiteX timer reg")?
            .map_err(|_| "invalid timer reg")?;
        if region.1.get() < 44 {
            return Err("timer has no 64-bit uptime counter");
        }
        let mut frequency = None;
        let mut irq = None;
        let mut parent = None;
        for (name, value) in node.props() {
            match name {
                "clock-frequency" => frequency = value.into_scalar_u32().filter(|v| *v != 0),
                "interrupts" => irq = value.into_scalar_u32(),
                "interrupt-parent" => parent = value.into_scalar_u32(),
                _ => {}
            }
        }
        Ok(Self {
            region,
            frequency: frequency.ok_or("missing timer clock-frequency")?,
            irq: (
                parent.ok_or("missing timer interrupt-parent")?,
                irq.ok_or("invalid timer interrupt")?,
            ),
        })
    }
    /// Map the CSR bank before publishing the timer.
    ///
    /// # Safety
    /// The mapper's boot-time requirements apply and firmware must describe
    /// an exclusively owned LiteX timer with suitable device memory attributes.
    pub unsafe fn install(
        self,
        mmio: &mut Mapper,
    ) -> Result<&'static dyn TimerDevice, &'static str> {
        let registers = unsafe { mmio.map(self.region.0, self.region.1) }
            .map_err(|_| "failed to map LiteX timer MMIO")?;
        Ok(CONFIG.get_or_init(|| Timer {
            base: registers.as_raw(),
            frequency: self.frequency,
        }))
    }
}

impl TimerDevice for Timer {
    fn ticks(&self) -> u64 {
        let _guard = runtime::arch::interrupt::InterruptGuard::new();
        write(self.base + 32, 1);
        let high = read(self.base + 36);
        let low = read(self.base + 40);
        (u64::from(high) << 32) | u64::from(low)
    }

    fn frequency(&self) -> NonZeroU64 {
        NonZeroU64::new(u64::from(self.frequency)).unwrap()
    }

    fn start_periodic(&self, hz: NonZeroU32) {
        let base = self.base;
        let interval = (self.frequency / hz.get()).max(1);
        write(base + 8, 0);
        write(base, interval);
        write(base + 4, interval);
        write(base + 24, 1);
        write(base + 28, 1);
        write(base + 8, 1);
    }

    fn take_event(&self) -> bool {
        if read(self.base + 24) & 1 == 0 {
            return false;
        }
        write(self.base + 24, 1);
        true
    }
}

pub const DRIVER: boot::drivers::TimerDriver = boot::drivers::TimerDriver {
    probe: |fdt| Ok(Config::probe(fdt)?.is_some()),
    install: |fdt, mmio| {
        let config = Config::probe(fdt)?.ok_or("timer disappeared after discovery")?;
        Ok(boot::drivers::InstalledTimer {
            device: unsafe { config.install(mmio)? },
            irq: Some(config.irq),
        })
    },
};
