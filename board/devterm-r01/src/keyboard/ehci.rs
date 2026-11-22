//! USB1 EHCI. The first DMA page is the frame list, the second holds aligned
//! queue heads, transfer descriptors and payloads. All pointers are bus addresses.
use runtime::arch::asm::time::ticks;

use super::Result;
use crate::hw::{Registers, delay_us};

const CTRL: usize = 0x1000;
const KEY: usize = 0x1080;
const HUB: usize = 0x1100;
const SETUP_TD: usize = 0x1200;
const DATA_TD: usize = 0x1240;
const STATUS_TD: usize = 0x1280;
const KEY_TD: [usize; 2] = [0x1300, 0x1340];
const HUB_TD: [usize; 2] = [0x1380, 0x13c0];
const SETUP: usize = 0x1400;
const INPUT: usize = 0x1480;
const BITMAP: usize = 0x1500;
const DATA: usize = 0x1800;

const END: u32 = 1;
const QH: u32 = 2;
const ACTIVE: u32 = 1 << 7;
const HALT: u32 = 1 << 6;
const IN: u32 = 1 << 8;
const SETUP_PID: u32 = 2 << 8;
const DATA1: u32 = 1 << 31;
const ASYNC: u32 = 1 << 5;
const PERIODIC: u32 = 1 << 4;

fn fence() {
    unsafe { core::arch::asm!("fence iorw, iorw", options(nostack)) };
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Speed {
    Full = 0,
    Low = 1,
    High = 2,
}

#[derive(Clone, Copy)]
pub(super) struct Device {
    pub addr: u8,
    pub packet: u8,
    speed: Speed,
    translator: u8,
    port: u8,
}
impl Device {
    fn root() -> Self {
        Self {
            addr: 0,
            packet: 64,
            speed: Speed::High,
            translator: 0,
            port: 0,
        }
    }

    pub fn child(self, port: u8, speed: Speed) -> Self {
        let (translator, port) = if speed == Speed::High {
            (0, 0)
        } else if self.speed == Speed::High {
            (self.addr, port)
        } else {
            (self.translator, self.port)
        };
        Self {
            addr: 0,
            packet: if speed == Speed::High { 64 } else { 8 },
            speed,
            translator,
            port,
        }
    }
    fn caps(self) -> u32 {
        (1 << 30) | (u32::from(self.translator) << 16) | (u32::from(self.port) << 23)
    }
    fn characteristics(self, ep: u8, packet: u8) -> u32 {
        u32::from(self.addr)
            | u32::from(ep) << 8
            | (self.speed as u32) << 12
            | u32::from(packet) << 16
    }
}

#[derive(Clone, Copy)]
pub(super) struct Hub {
    pub dev: Device,
    pub number: u8,
    pub packet: u8,
    pub interval: u8,
}

pub(super) struct Host {
    op: Registers,
    dma: Registers,
    physical: u32,
    key_td: usize,
    key_packet: u8,
    hub_td: usize,
    port: Option<(Hub, u16)>,
}

impl Host {
    pub fn new(base: Registers, dma: Registers, physical: u32) -> Result<Self> {
        let caplen = (base.get(0) & 255) as usize;
        if !(0x10..=0x80).contains(&caplen) || base.get(4) & 15 == 0 {
            return Err("EHCI capabilities");
        }
        let op = base.offset(caplen);
        op.set(0, 0);
        if !op.wait(4, 1 << 12, 1 << 12, 100_000) {
            return Err("EHCI halt");
        }
        op.set(0, 2);
        if !op.wait(0, 2, 0, 100_000) {
            return Err("EHCI reset");
        }
        for off in (0..8192).step_by(4) {
            dma.set(off, if off < 4096 { END } else { 0 });
        }
        op.set(8, 0); // completion is polled
        if base.get(8) & 1 != 0 {
            op.set(0x10, 0);
        } // upper 32 bits of all DMA addresses
        op.set(0x14, physical);
        op.set(0x18, physical + CTRL as u32);
        op.set(4, 0x3f);
        fence();
        op.set(0, 1 | (1 << 16)); // run, one-microframe interrupt threshold
        op.set(0x40, 1); // route USB1 to EHCI
        op.set(0x44, (op.get(0x44) & !(0x2a | 1 << 13)) | 1 << 12);
        delay_us(100_000);
        Ok(Self {
            op,
            dma,
            physical,
            key_td: 0,
            key_packet: 0,
            hub_td: 0,
            port: None,
        })
    }

    pub fn root_reset(&self) -> Result<Device> {
        if !self.op.wait(0x44, 1, 1, 1_500_000) {
            return Err("EHCI root disconnected");
        }
        let port = self.op.get(0x44) & !(0x2a | 4 | 1 << 13);
        self.op.set(0x44, port | (1 << 8));
        delay_us(55_000);
        self.op.set(0x44, port);
        if !self.op.wait(0x44, 0x105, 5, 250_000) {
            return Err("EHCI high-speed root port reset failed");
        }
        delay_us(20_000);
        Ok(Device::root())
    }

    fn bus(&self, offset: usize) -> u32 {
        self.physical + offset as u32
    }

    fn schedule(&self, bit: u32, enabled: bool) -> Result<()> {
        self.op.update(0, bit, if enabled { bit } else { 0 });
        let status = bit << 10;
        if !self
            .op
            .wait(4, status, if enabled { status } else { 0 }, 100_000)
        {
            return Err("EHCI schedule timeout");
        }
        Ok(())
    }

    fn qh(&self, qh: usize, link: u32, flags: u32, caps: u32, td: usize) {
        for off in (0..68).step_by(4) {
            self.dma.set(qh + off, 0);
        }
        self.dma.set(qh, link);
        self.dma.set(qh + 4, flags);
        self.dma.set(qh + 8, caps);
        self.dma.set(qh + 16, self.bus(td));
        self.dma.set(qh + 20, END);
    }

    fn td(&self, td: usize, next: u32, alternate: u32, flags: u32, buf: usize, len: usize) {
        self.dma.set(td, next);
        self.dma.set(td + 4, alternate);
        self.dma.set(td + 12, self.bus(buf));
        // Each buffer fits within one physical page. Publish Active last.
        fence();
        self.dma
            .set(td + 8, ACTIVE | (3 << 10) | ((len as u32) << 16) | flags);
    }

    pub fn control(
        &mut self,
        dev: Device,
        kind: u8,
        req: u8,
        value: u16,
        index: u16,
        data: &mut [u8],
    ) -> Result<usize> {
        let len = data.len();
        if len > 1024 || (len != 0 && kind & 0x80 == 0) {
            return Err("unsupported control length/direction");
        }
        self.dma.set(
            SETUP,
            u32::from(kind) | u32::from(req) << 8 | u32::from(value) << 16,
        );
        self.dma
            .set(SETUP + 4, u32::from(index) | (len as u32) << 16);
        let status = self.bus(STATUS_TD);
        self.td(
            STATUS_TD,
            END,
            END,
            DATA1 | if len == 0 { IN } else { 0 },
            0,
            0,
        );
        if len != 0 {
            self.td(DATA_TD, status, status, DATA1 | IN, DATA, len);
        }
        self.td(
            SETUP_TD,
            if len == 0 { status } else { self.bus(DATA_TD) },
            END,
            SETUP_PID,
            SETUP,
            8,
        );
        // Control data toggles come from qTDs. C selects split control protocol.
        let flags = dev.characteristics(0, dev.packet)
            | (1 << 14)
            | (1 << 15)
            | (4 << 28)
            | (u32::from(dev.speed != Speed::High) << 27);
        self.qh(CTRL, self.bus(CTRL) | QH, flags, dev.caps(), SETUP_TD);
        self.op.set(0x18, self.bus(CTRL));
        fence();
        self.schedule(ASYNC, true)?;
        let start = ticks();
        loop {
            if self.dma.get(STATUS_TD + 8) & ACTIVE == 0
                || self.dma.get(CTRL + 24) & HALT != 0
                || ticks().wrapping_sub(start) > 24_000_000
            {
                break;
            }
            core::hint::spin_loop();
        }
        self.schedule(ASYNC, false)?;
        fence();
        let token = self.dma.get(STATUS_TD + 8);
        if token & (ACTIVE | HALT) != 0 || self.dma.get(CTRL + 24) & HALT != 0 {
            return Err("EHCI control transfer failed");
        }
        let residual = if len == 0 {
            0
        } else {
            ((self.dma.get(DATA_TD + 8) >> 16) & 0x7fff) as usize
        };
        let got = len.checked_sub(residual).ok_or("EHCI control count")?;
        self.dma.read_bytes(DATA, &mut data[..got]);
        Ok(got)
    }

    pub fn stop_input(&self) -> Result<()> {
        self.schedule(PERIODIC, false)
    }

    fn periodic_pipe(&self, pipe: Hub, qh: usize, tds: [usize; 2], buffer: usize) -> usize {
        let (mask, period) = match pipe.dev.speed {
            Speed::High => {
                let microframes = 1usize << (pipe.interval.clamp(1, 16) - 1);
                let mut mask = 0;
                for microframe in (0..8).step_by(microframes) {
                    mask |= 1 << microframe;
                }
                (mask, (microframes / 8).clamp(1, 1024))
            }
            _ => (0x1c01, 1usize << pipe.interval.max(1).ilog2()), // start split in uFrame 0, complete in 2–4
        };
        self.dma.set(tds[1] + 8, 0);
        self.td(
            tds[0],
            self.bus(tds[1]),
            self.bus(tds[1]),
            IN,
            buffer,
            usize::from(pipe.packet),
        );
        self.qh(
            qh,
            END,
            pipe.dev.characteristics(pipe.number, pipe.packet),
            pipe.dev.caps() | mask,
            tds[0],
        );
        period
    }

    pub fn start_input(
        &mut self,
        dev: Device,
        number: u8,
        packet: u8,
        interval: u8,
        parent: Option<(Hub, u16)>,
    ) -> Result<()> {
        self.key_td = 0;
        self.hub_td = 0;
        self.key_packet = packet;
        self.port = parent;
        let key = self.periodic_pipe(
            Hub {
                dev,
                number,
                packet,
                interval,
            },
            KEY,
            KEY_TD,
            INPUT,
        );
        for slot in 0..1024 {
            self.dma.set(slot * 4, END);
        }
        if let Some((hub, port)) = parent {
            self.dma.set(BITMAP + usize::from(port / 32) * 4, 0);
            let hp = self.periodic_pipe(hub, HUB, HUB_TD, BITMAP);
            let (first, second) = if hp >= key { (HUB, KEY) } else { (KEY, HUB) };
            self.dma.set(first, self.bus(second) | QH);
            for slot in (0..1024).step_by(key.min(hp)) {
                let qh = if slot % key.max(hp) == 0 {
                    first
                } else {
                    second
                };
                self.dma.set(slot * 4, self.bus(qh) | QH);
            }
        } else {
            for slot in (0..1024).step_by(key) {
                self.dma.set(slot * 4, self.bus(KEY) | QH);
            }
        }
        fence();
        self.schedule(PERIODIC, true)
    }

    fn received(&self, td: usize, len: u8) -> Result<Option<usize>> {
        let token = self.dma.get(td + 8);
        if token & ACTIVE != 0 {
            return Ok(None);
        }
        if token & HALT != 0 {
            return Err("EHCI interrupt transfer halted");
        }
        fence();
        let residual = ((token >> 16) & 0x7fff) as usize;
        Ok(Some(
            usize::from(len)
                .checked_sub(residual)
                .ok_or("EHCI interrupt count")?,
        ))
    }

    fn rearm(&self, tds: [usize; 2], done: usize, buffer: usize, packet: u8) {
        // The inactive dummy is already linked from the retired qTD. Make the
        // old qTD the next dummy before making the current one visible to EHCI.
        self.dma.set(tds[done] + 8, 0);
        let dummy = self.bus(tds[done]);
        self.td(tds[done ^ 1], dummy, dummy, IN, buffer, usize::from(packet));
    }

    pub fn input(&mut self, data: &mut [u8; 64]) -> Result<usize> {
        if let Some((hub, port)) = self.port
            && let Some(size) = self.received(HUB_TD[self.hub_td], hub.packet)?
        {
            let offset = usize::from(port / 8);
            if offset < size && self.dma.get(BITMAP + (offset & !3)) & (1 << (port % 32)) != 0 {
                return Err("hub port changed");
            }
            self.rearm(HUB_TD, self.hub_td, BITMAP, hub.packet);
            self.hub_td ^= 1;
        }
        let Some(size) = self.received(KEY_TD[self.key_td], self.key_packet)? else {
            return Ok(0);
        };
        self.dma.read_bytes(INPUT, &mut data[..size]);
        self.rearm(KEY_TD, self.key_td, INPUT, self.key_packet);
        self.key_td ^= 1;
        Ok(size)
    }
}
