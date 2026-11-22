//! One XRGB8888 UI layer, scanned through LCD0 in DSI trigger mode.
mod dsi;
mod mipi;
mod panel;
mod text;

use runtime::mm::mmio::Mapper;
pub use text::TextConsole;
use text::{HEIGHT, WIDTH};

use crate::hw::{Registers, Result, delay_us};

#[derive(Clone, Copy)]
pub(crate) struct Mode {
    pub(crate) width: u16,
    pub(crate) hbp: u16,
}

impl Mode {
    const VSYNC: u16 = 10;
    const VBACK: u16 = 2;
    const VFRONT: u16 = 16;
    const LINES: u16 = HEIGHT as u16 + Self::VSYNC + Self::VBACK + Self::VFRONT;
    const HSYNC: u16 = 40;

    fn htotal(self) -> u16 {
        self.width + 214
    }
}

fn enable_clocks(ccu: Registers) -> Result<()> {
    // D1-H 3.2. The 1x VIDEO0 output divides the 1320 MHz VCO by four.
    let pll_controls = (1 << 31) | (1 << 30) | (1 << 29) | (1 << 27);
    ccu.update(
        0x40,
        pll_controls | (7 << 24) | (255 << 8) | 3,
        pll_controls | ((55 - 1) << 8),
    );
    if !ccu.wait(0x40, 1 << 28, 1 << 28, 10_000) {
        return Err("LCD clock failed to lock");
    }
    let enable = 1 << 31;
    ccu.set(0xb60, enable); // TCON = VIDEO0(1x)
    ccu.set(0x600, enable | 3); // DE = PERI(2x)/4
    ccu.set(0xb24, enable | (1 << 24) | 3); // PHY reference = PERI(1x)/4
    for device in [0xabc, 0x60c, 0xb7c, 0xb4c] {
        ccu.update(device, 0x10001, 0x10001);
    }
    Ok(())
}

fn framebuffer(de: Registers, mixer: Registers, address: u32, width: u16) {
    // DE2.0 5. Set the layer geometry separately from the blender output:
    // the trailing scan columns are supplied by its black background.
    let extent = |w| ((HEIGHT as u32 - 1) << 16) | (w - 1);
    de.update(0x08, 1, 1);
    de.update(0x04, 1, 1);
    de.update(0x00, 1, 1);
    de.update(0x0c, 15, 0);
    let ui = mixer.offset(0x3000);
    let blend = mixer.offset(0x1000);
    mixer.set(0, 1);
    ui.set(0x10, address);
    ui.set(0x80, 0); // upper address bits
    ui.set(0x0c, WIDTH as u32 * 4);
    ui.set(0x08, 0); // layer origin
    ui.set(0x04, extent(WIDTH as u32));
    ui.set(0x88, extent(WIDTH as u32));
    ui.set(0, (255 << 24) | (4 << 8) | (1 << 1) | 1);
    blend.set(0x88, 255 << 24);
    blend.set(0x80, 1); // pipe 0 from UI channel 1
    blend.set(0x08, extent(WIDTH as u32));
    blend.set(0x0c, 0);
    blend.set(0, 1 << 8);
    blend.set(0x8c, extent(u32::from(width)));
    mixer.set(0x0c, extent(u32::from(width)));
    mixer.set(0x08, 1); // commit shadow registers
}

fn lcd0(top: Registers, lcd: Registers, mode: Mode) {
    let width = u32::from(mode.width);
    let line = u32::from(mode.htotal());
    top.update(0x1c, 0x33, 1 << 4);
    top.update(0x20, 1 << 16, 1 << 16);
    lcd.set(0x00, (1 << 31) | 2);
    lcd.set(0x40, 1 << 24); // CPU interface, enable after programming the link
    lcd.set(0x44, (15 << 28) | 4);
    lcd.set(0x48, ((width - 1) << 16) | (HEIGHT as u32 - 1));
    lcd.set(0xf8, 1 << 3);
    lcd.set(0x60, (1 << 28) | (1 << 16) | 5);
    lcd.set(0x8c, 7 << 29);
    lcd.set(0x1f0, (3000 << 16) | 3);
    // Four lanes carry 24 bits/pixel. The trigger data clock divides by four.
    let idle = line * 24 / 16 - width - u32::from(Mode::HSYNC);
    lcd.set(0x160, ((idle - 1) << 16) | (width - 1));
    lcd.set(0x164, HEIGHT as u32 - 1);
    let delay_lines = u32::from(Mode::VBACK + Mode::VFRONT - 1);
    let ticks = delay_lines * line * 149 / (55 * 8);
    lcd.set(0x168, (ticks << 16) | u32::from(Mode::VSYNC));
}

pub fn init(
    pixels: usize,
    base: u32,
    mapper: &mut Mapper,
    ccu: Registers,
    mode: Mode,
) -> Result<TextConsole> {
    let gpio = Registers::map(mapper, 0x0200_0000, 0x1000)?;
    let pwm = gpio.offset(0xc00);
    panel::hold_reset(gpio);
    crate::pmic::init(mapper, gpio, ccu)?;
    enable_clocks(ccu)?;
    let de = Registers::map(mapper, 0x0500_0000, 0x1000)?;
    let mix = Registers::map(mapper, 0x0510_0000, 0x4000)?;
    let top = Registers::map(mapper, 0x0546_0000, 0x2000)?;
    let lcd = top.offset(0x1000);
    let link = Registers::map(mapper, 0x0545_0000, 0x2000)?;
    let screen = TextConsole::new(unsafe {
        core::slice::from_raw_parts_mut(pixels as *mut u32, WIDTH * HEIGHT)
    });
    unsafe { core::arch::asm!("fence iorw, iorw", options(nostack)) };
    framebuffer(de, mix, base, mode.width);
    lcd0(top, lcd, mode);
    dsi::prepare(link, mode);
    panel::release_reset(gpio);
    dsi::wake(link)?;
    lcd.update(0x40, 1 << 31, 1 << 31);
    dsi::stream(link);
    delay_us(200_000);
    panel::illuminate(gpio, ccu, pwm);
    Ok(screen)
}
