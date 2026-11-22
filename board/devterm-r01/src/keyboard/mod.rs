//! Enumerate the onboard USB hub and the STM32 composite HID keyboard through the D1 high-speed host.
mod ehci;
mod hid;
use alloc::vec;

use ehci::{Device, Host, Speed};
use hid::{Keys, keyboard_report_id};
use runtime::arch::paging::MemoryType;
use runtime::mm::mmio::Mapper;

use crate::hw::{Registers, Result, delay_us};

struct Endpoint {
    dev: Device,
    parent: Option<(ehci::Hub, u16)>,
    number: u8,
    packet: u8,
    interval: u8,
    id: Option<u8>,
}

pub struct Keyboard {
    host: Host,
    ep: Endpoint,
    keys: Keys,
}

impl Keyboard {
    pub fn init(mapper: &mut Mapper, ccu: Registers, dma_phys: u32) -> Result<Self> {
        let usb = Registers::map(mapper, 0x0420_0000, 0x1000)?;
        let dma =
            Registers::map_with_type(mapper, dma_phys as usize, 8192, MemoryType::NonCacheable)?;
        // 12 MHz OHCI clock from HOSC/2, PHY1 reset deasserted.
        ccu.update(0xa74, 0xc300_0000, 0xc100_0000);
        ccu.update(0xa8c, 0x22_0000, 0); // hold both host controllers in reset
        delay_us(10);
        ccu.update(0xa8c, 0x22_0022, 0x22_0022);
        delay_us(1000);
        usb.update(0x810, 1 << 3, 0); // PMU1 PHY SIDDQ
        usb.update(0x800, 0xf01, 0xf01); // AHB burst and UTMI bypass
        Self::attach(usb, dma, dma_phys)
    }

    pub(crate) fn attach(regs: Registers, dma: Registers, dma_phys: u32) -> Result<Self> {
        let mut host = Host::new(regs, dma, dma_phys)?;
        let root = host.root_reset()?;
        let mut next = 1;
        let ep = enumerate(&mut host, root, &mut next, 0)?.ok_or("USB keyboard not found")?;
        host.start_input(ep.dev, ep.number, ep.packet, ep.interval, ep.parent)?;
        Ok(Self {
            host,
            ep,
            keys: Keys::default(),
        })
    }

    // Called by a kernel thread with the controller exclusively owned.
    pub fn reconnect(&mut self) -> Result<()> {
        self.host.stop_input()?;
        let link = self.reset_link()?;
        // Reset returns only this device to address zero. Reuse its former
        // address and leave sibling devices on the hub alone.
        let mut addr = self.ep.dev.addr;
        let device = address_device(&mut self.host, link, &mut addr)?;
        let mut ep = find_endpoint(&mut self.host, device, &mut addr, 0, false)?
            .ok_or("no keyboard on recovered port")?;
        ep.parent = self.ep.parent;
        self.host
            .start_input(ep.dev, ep.number, ep.packet, ep.interval, ep.parent)?;
        self.ep = ep;
        self.keys = Keys::default();
        Ok(())
    }

    fn reset_link(&mut self) -> Result<Device> {
        match self.ep.parent {
            Some((parent, port)) => {
                let hub = parent.dev;
                let mut status = [0; 4];
                self.host.control(hub, 0xa3, 0, 0, port, &mut status)?;
                let bits = u32::from_le_bytes(status);
                for bit in 1..=4 {
                    if status[2] & (1 << bit) != 0 {
                        self.host.control(hub, 0x23, 1, 16 + bit, port, &mut [])?;
                    }
                }
                if bits & 1 == 0 {
                    return Err("keyboard disconnected");
                }
                if bits & (1 << 16) != 0 {
                    self.host.control(hub, 0x23, 1, 16, port, &mut [])?;
                    delay_us(100_000); // connection debounce
                }
                reset_hub_port(&mut self.host, hub, port)?.ok_or("keyboard port reset")
            }
            None => self.host.root_reset(),
        }
    }

    pub fn poll(&mut self, receive: &mut dyn FnMut(u8)) -> Result<()> {
        let mut data = [0; 64];
        let got = self.host.input(&mut data)?;
        if got != 0 {
            self.keys.report(&data[..got], self.ep.id, receive);
        }
        Ok(())
    }
}

fn address_device(host: &mut Host, mut dev: Device, next: &mut u8) -> Result<Device> {
    let mut device = [0u8; 18];
    if host.control(dev, 0x80, 6, 0x100, 0, &mut device[..8])? != 8
        || !matches!(device[7], 8 | 16 | 32 | 64)
    {
        return Err("USB EP0 descriptor");
    }
    dev.packet = device[7];
    host.control(dev, 0, 5, u16::from(*next), 0, &mut [])?;
    dev.addr = *next;
    *next += 1;
    delay_us(10_000);
    if host.control(dev, 0x80, 6, 0x100, 0, &mut device)? != 18 {
        return Err("USB device descriptor");
    }
    Ok(dev)
}

fn configuration(host: &mut Host, dev: Device) -> Result<alloc::vec::Vec<u8>> {
    let mut header = [0u8; 9];
    if host.control(dev, 0x80, 6, 0x200, 0, &mut header)? != 9 {
        return Err("USB configuration header");
    }
    let size = usize::from(u16::from_le_bytes([header[2], header[3]]));
    if !(9..=1024).contains(&size) {
        return Err("USB configuration size");
    }
    let mut config = vec![0; size];
    if host.control(dev, 0x80, 6, 0x200, 0, &mut config)? != size {
        return Err("short USB configuration");
    }
    host.control(dev, 0, 9, u16::from(header[5]), 0, &mut [])?;
    Ok(config)
}

fn enumerate(host: &mut Host, link: Device, next: &mut u8, depth: u8) -> Result<Option<Endpoint>> {
    if depth > 5 || *next > 127 {
        return Err("USB topology limit");
    }
    let dev = address_device(host, link, next)?;
    find_endpoint(host, dev, next, depth, true)
}

fn find_endpoint(
    host: &mut Host,
    dev: Device,
    next: &mut u8,
    depth: u8,
    scan_hubs: bool,
) -> Result<Option<Endpoint>> {
    let config = configuration(host, dev)?;
    let mut tail = &config[..];
    let mut interface = None;
    let mut report_len = 0;
    let mut hub_interface = false;
    while tail.len() >= 2 {
        let len = usize::from(tail[0]);
        if len < 2 || len > tail.len() {
            return Err("bad USB descriptor");
        }
        let (d, rest) = tail.split_at(len);
        tail = rest;
        match d[1] {
            4 if len >= 9 => {
                hub_interface = scan_hubs && d[3] == 0 && d[5] == 9;
                interface = (d[3] == 0 && d[5] == 3).then_some((d[2], d[6] == 1 && d[7] == 1));
                report_len = 0;
            }
            0x21 if len >= 9 && d[6] == 0x22 => {
                report_len = usize::from(u16::from_le_bytes([d[7], d[8]]))
            }
            5 if len >= 7 && d[2] & 0x80 != 0 && d[3] & 3 == 3 => {
                if hub_interface {
                    let size = u16::from_le_bytes([d[4], d[5]]);
                    if !(1..=64).contains(&size) {
                        return Err("hub endpoint size");
                    }
                    let hub_ep = ehci::Hub {
                        dev,
                        number: d[2] & 15,
                        packet: size as u8,
                        interval: d[6],
                    };
                    return hub(host, hub_ep, next, depth);
                }
                let Some((number, boot)) = interface else {
                    continue;
                };
                let packet = u16::from_le_bytes([d[4], d[5]]);
                if !(8..=64).contains(&packet) || d[2] & 15 == 0 {
                    continue;
                }
                let id = if boot {
                    host.control(dev, 0x21, 11, 0, u16::from(number), &mut [])?;
                    None
                } else {
                    if !(1..=1024).contains(&report_len) {
                        continue;
                    }
                    let mut report = vec![0; report_len];
                    let got = host.control(dev, 0x81, 6, 0x2200, u16::from(number), &mut report)?;
                    let Some(id) = keyboard_report_id(&report[..got]) else {
                        continue;
                    };
                    Some(id)
                };
                return Ok(Some(Endpoint {
                    dev,
                    parent: None,
                    number: d[2] & 15,
                    packet: packet as u8,
                    interval: d[6],
                    id,
                }));
            }
            _ => {}
        }
    }
    Ok(None)
}

fn hub(host: &mut Host, endpoint: ehci::Hub, next: &mut u8, depth: u8) -> Result<Option<Endpoint>> {
    let dev = endpoint.dev;
    let mut desc = [0; 7];
    if host.control(dev, 0xa0, 6, 0x2900, 0, &mut desc)? != 7 || desc[1] != 0x29 {
        return Err("USB hub descriptor");
    }
    let ports = u16::from(desc[2]);
    for port in 1..=ports {
        host.control(dev, 0x23, 3, 8, port, &mut [])?;
    }
    delay_us(u64::from(desc[5]) * 2000 + 150_000);
    scan_hub(host, endpoint, next, depth, ports)
}

fn scan_hub(
    host: &mut Host,
    endpoint: ehci::Hub,
    next: &mut u8,
    depth: u8,
    ports: u16,
) -> Result<Option<Endpoint>> {
    // The STM32 DFU bootloader can disconnect and reappear as HID after
    // the first hub scan. Reset only newly connected ports, leaving already
    // addressed devices and the hub powered during the transition.
    let start = runtime::arch::asm::time::ticks();
    let dev = endpoint.dev;
    let mut attempts = [0u8; 128];
    if ports as usize >= attempts.len() {
        return Err("invalid hub port count");
    }
    let mut last_error = None;
    loop {
        let mut status = [0; 4];
        for port in 1..=ports {
            host.control(dev, 0xa3, 0, 0, port, &mut status)?;
            let changed = status[2] & 1 != 0;
            if changed {
                host.control(dev, 0x23, 1, 16, port, &mut [])?;
            }
            if status[0] & 1 == 0 {
                attempts[port as usize] = 0;
                continue;
            }
            let attempt = &mut attempts[port as usize];
            if changed {
                *attempt = 0;
            }
            if *attempt >= 3 {
                continue;
            }
            *attempt += 1;
            let Some(link) = reset_hub_port(host, dev, port)? else {
                last_error = Some("USB hub port reset");
                continue;
            };
            match enumerate(host, link, next, depth + 1) {
                Ok(Some(mut ep)) => {
                    ep.parent.get_or_insert((endpoint, port));
                    return Ok(Some(ep));
                }
                Ok(None) => *attempt = 3,
                Err(error) => {
                    last_error = Some(error);
                    host.control(dev, 0x23, 1, 1, port, &mut [])?;
                } // isolate failed address-zero device
            }
        }
        if depth != 0 || runtime::arch::asm::time::ticks().wrapping_sub(start) >= 6 * 24_000_000 {
            break;
        }
        delay_us(100_000);
    }
    match last_error {
        Some(error) => Err(error),
        None => Ok(None),
    }
}

fn reset_hub_port(host: &mut Host, dev: Device, port: u16) -> Result<Option<Device>> {
    let mut status = [0; 4];
    host.control(dev, 0x23, 3, 4, port, &mut [])?;
    for _ in 0..50 {
        delay_us(10_000);
        host.control(dev, 0xa3, 0, 0, port, &mut status)?;
        if status[0] & 0x13 == 3 && status[2] & 0x10 != 0 {
            // Enabling a port as part of reset can also latch C_PORT_ENABLE.
            // Acknowledge the observed changes before arming the hub interrupt.
            for bit in 0..=4 {
                if status[2] & (1 << bit) != 0 {
                    host.control(dev, 0x23, 1, 16 + bit, port, &mut [])?;
                }
            }
            delay_us(10_000);
            let speed = match status[1] & 6 {
                0 => Speed::Full,
                2 => Speed::Low,
                4 => Speed::High,
                _ => return Err("invalid hub port speed"),
            };
            return Ok(Some(dev.child(port as u8, speed)));
        }
    }
    Ok(None)
}
