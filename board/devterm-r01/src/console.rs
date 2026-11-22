//! Mirror stdout to the LCD and USB CDC. Poll USB HID and CDC for stdin.
use alloc::boxed::Box;
use alloc::sync::Arc;
use core::convert::Infallible;

use runtime::dev::console::Console;
use runtime::dev::dt::Fdt;
use runtime::dev::dt::reg::CompatibleRegIter;
use runtime::dev::uart::{Read, Write};
use runtime::kernel::sync::SpinLock;
use runtime::kernel::{clock, thread, timer};
use runtime::mm::mmio::Mapper;

use crate::hw::{Registers, Result};
use crate::{display, keyboard};

struct LocalConsole {
    screen: Option<display::TextConsole>,
    usb: crate::cdc::Cdc,
    keyboard: Option<keyboard::Keyboard>,
    recovered: Arc<SpinLock<Option<keyboard::Keyboard>>>,
}

impl Write for LocalConsole {
    type Error = Infallible;
    fn write(&mut self, data: &[u8]) -> core::result::Result<usize, Infallible> {
        if let Some(screen) = &mut self.screen {
            screen.write(data);
        }
        self.usb.write(data);
        Ok(data.len())
    }
}
impl Read for LocalConsole {
    type Error = Infallible;
    fn read(&mut self, data: &mut [u8]) -> core::result::Result<Option<usize>, Infallible> {
        Ok(data.is_empty().then_some(0))
    }
}
impl Console for LocalConsole {
    fn enable_rx_interrupt(&mut self) {}
    fn flush_output(&mut self) {
        self.usb.flush();
    }
    fn drain_rx(&mut self, mut space: usize, receive: &mut dyn FnMut(u8)) {
        if self.keyboard.is_none() {
            self.keyboard = self.recovered.lock().take();
        }
        if let Some(kbd) = self.keyboard.as_mut()
            && kbd
                .poll(&mut |b| {
                    space = space.saturating_sub(1);
                    receive(b);
                })
                .is_err()
        {
            let mut keyboard = self.keyboard.take().unwrap();
            let recovered = self.recovered.clone();
            // Wait for port reset and enumeration outside the console lock.
            thread::spawn(move || {
                while keyboard.reconnect().is_err() {
                    let next = clock::clock_millis() + 1000;
                    thread::CurrentThread::wait(|cx| timer::poll_sleep(next, cx));
                }
                *recovered.lock() = Some(keyboard);
            });
        }
        self.usb.poll(space, receive);
    }
}

/// These DMA leases must be the board's no-map carveouts, outside usable RAM.
fn reserved(fdt: &Fdt, path: &str, expected: u32, size: usize) -> Result<u32> {
    let node = fdt.lookup(path);
    let (ac, sc) = node.reg_cells();
    let reg = node.clone().prop("reg").ok_or("missing reserved memory")?;
    let (base, len) = CompatibleRegIter::from(reg.into_reg(ac, sc))
        .next()
        .ok_or("missing reserved range")?
        .map_err(|_| "invalid reserved range")?;
    if node.prop("no-map").is_none() || base != expected as usize || len.get() != size {
        return Err("invalid board DMA carveout");
    }
    Ok(expected)
}

fn display_settings(fdt: &Fdt) -> display::Mode {
    let lcd = fdt.lookup("/reserved-memory/framebuffer@7fd00000");
    let hbp = lcd
        .clone()
        .prop("clockwork,lcd-hbp")
        .and_then(|p| p.into_scalar_u32())
        .filter(|v| (146..=154).contains(v))
        .unwrap_or(150) as u16;
    let width = lcd
        .prop("clockwork,lcd-scan-width")
        .and_then(|p| p.into_scalar_u32())
        .filter(|v| (480..=488).contains(v))
        .unwrap_or(484) as u16;
    display::Mode { width, hbp }
}

fn attach_display(
    pixels: usize,
    fb: u32,
    mapper: &mut Mapper,
    ccu: Registers,
    mode: display::Mode,
    usb: &mut crate::cdc::Cdc,
) -> Option<display::TextConsole> {
    match display::init(pixels, fb, mapper, ccu, mode) {
        Ok(screen) => Some(screen),
        Err(error) => {
            usb.write(b"LCD initialization failed: ");
            usb.write(error.as_bytes());
            usb.write(b"\r\n");
            None
        }
    }
}

pub const DRIVER: boot::drivers::ConsoleDriver = boot::drivers::ConsoleDriver {
    memory_type: runtime::arch::paging::MemoryType::NonCacheable,
    rx_interrupt: false,
    compatibles: &["clockwork,devterm-lcd-console"],
    create: |pixels, fdt, mapper| {
        let fb = reserved(
            fdt,
            "/reserved-memory/framebuffer@7fd00000",
            0x7fd00000,
            480 * 1280 * 4,
        )
        .expect("invalid LCD framebuffer");
        let ccu = Registers::map(mapper, 0x02001000, 4096).expect("D1 CCU");
        ccu.update(0x90c, 0x10001, 0); // assert UART0 reset and gate its bus clock
        let mode = display_settings(fdt);
        let mut usb = crate::cdc::Cdc::new(mapper, ccu).expect("USB0 mapping");
        let screen = attach_display(pixels, fb, mapper, ccu, mode, &mut usb);
        let keyboard = reserved(fdt, "/reserved-memory/usb-dma@7ff60000", 0x7ff60000, 8192)
            .and_then(|dma| keyboard::Keyboard::init(mapper, ccu, dma))
            .ok();
        Box::new(LocalConsole {
            screen,
            usb,
            keyboard,
            recovered: Arc::new(SpinLock::new(None)),
        })
    },
};
