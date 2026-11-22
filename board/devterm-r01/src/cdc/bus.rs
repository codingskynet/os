//! Polled USB0 peripheral. Each non-control endpoint owns a single 64-byte FIFO.
use runtime::kernel::sync::SpinLock;
use runtime::mm::mmio::Mapper;
use usb_device::bus::{PollResult, UsbBus};
use usb_device::endpoint::{EndpointAddress, EndpointType};
use usb_device::{Result, UsbDirection, UsbError};

use crate::hw::{Registers, delay_us};

const EP_COUNT: usize = 6;
const POWER: usize = 0x40;
const INDEX: usize = 0x42;
const TX_IRQ: usize = 0x44;
const RX_IRQ: usize = 0x46;
const BUS_IRQ: usize = 0x4c;
const TXCSR: usize = 0x82;
const RXCSR: usize = 0x86;
const COUNT: usize = 0x88;
const FUNC_ADDR: usize = 0x98;

// PERI_CSR0 commands. Read flags and write commands share this address.
const SERVICE_RX: u16 = 1 << 6;
const SERVICE_SETUP_END: u16 = 1 << 7;
const DATA_END: u16 = 1 << 3;
const EP0_SEND: u16 = 1 << 1;

#[derive(Default)]
enum Phase {
    #[default]
    Request,
    ToHost(usize),
    FromHost(usize),
    Accept, // The class driver may now acknowledge or stall the status stage.
}

#[derive(Default)]
struct Controller {
    phase: Phase,
    transmitting: u16,
}

#[derive(Default, Clone, Copy)]
struct Endpoint {
    input: u16,
    output: u16,
}

impl Endpoint {
    fn max_packet(&mut self, dir: UsbDirection) -> &mut u16 {
        match dir {
            UsbDirection::In => &mut self.input,
            UsbDirection::Out => &mut self.output,
        }
    }
}

pub(super) struct Bus {
    io: Registers,
    endpoints: [Endpoint; EP_COUNT],
    // INDEX is shared by all endpoints, even for status reads.
    controller: SpinLock<Controller>,
}

impl Bus {
    pub(super) fn new(mapper: &mut Mapper, ccu: Registers) -> crate::hw::Result<Self> {
        let io = Registers::map(mapper, 0x0410_0000, 4096)?;
        let clock = 1 << 8;
        let reset = 1 << 24;
        ccu.update(0xa8c, clock | reset, clock | reset);
        io.write8(POWER, 0);
        delay_us(30_000); // allow a host to observe the FEL disconnect
        ccu.update(0xa8c, reset, 0);
        ccu.update(0xa70, 1 << 30, 0);
        delay_us(20);
        ccu.update(0xa70, 1 << 30, 1 << 30);
        ccu.update(0xa8c, reset, reset);
        delay_us(1000);
        io.update(0x420, 1, 1); // select peripheral controller
        io.update(0x410, (1 << 5) | (1 << 3), 1 << 5); // VBUS valid, PHY powered
        io.update(0x400, 0x3f << 12, 0x3f << 12); // force peripheral role
        io.write8(0x43, 0);
        Ok(Self {
            io,
            endpoints: [Endpoint::default(); EP_COUNT],
            controller: SpinLock::new(Controller::default()),
        })
    }

    pub(super) fn tx_idle(&self) -> bool {
        self.controller.lock().transmitting == 0
    }

    fn select(&self, ep: usize) {
        self.io.write8(INDEX, ep as u8);
    }

    fn capacity(&self, addr: EndpointAddress, dir: UsbDirection) -> Result<usize> {
        let mut config = *self
            .endpoints
            .get(addr.index())
            .ok_or(UsbError::InvalidEndpoint)?;
        let bytes = *config.max_packet(dir);
        if bytes == 0 || dir != addr.direction() {
            return Err(UsbError::InvalidEndpoint);
        }
        Ok(bytes.into())
    }

    fn configure_fifo(&self, ep: usize, dir: UsbDirection, packet: u16) {
        let inbound = dir == UsbDirection::In;
        let (maxp, csr, size, base, flush) = if inbound {
            (0x80, TXCSR, 0x90, 0x92, (1 << 13) | (1 << 6) | (1 << 3))
        } else {
            (0x84, RXCSR, 0x94, 0x96, (1 << 7) | (1 << 4))
        };
        // Unit = 8 bytes. EP0 uses slot 0, then reserve OUT/IN slots in pairs.
        let slot = 2 * ep - 1 + usize::from(inbound);
        self.io.write8(size, 3);
        self.io.write16(base, (slot * 8) as u16);
        self.io.write16(maxp, packet);
        self.io.write16(csr, flush);
    }

    // Register, request-stall, sent-stall, reset-data-toggle.
    fn halt_bits(addr: EndpointAddress) -> (usize, u16, u16, u16) {
        match (addr.index(), addr.direction()) {
            (0, _) => (TXCSR, 1 << 5, 1 << 2, 0),
            (_, UsbDirection::In) => (TXCSR, 1 << 4, 1 << 5, 1 << 6),
            (_, UsbDirection::Out) => (RXCSR, 1 << 5, 1 << 6, 1 << 7),
        }
    }
}

impl UsbBus for Bus {
    fn alloc_ep(
        &mut self,
        dir: UsbDirection,
        requested: Option<EndpointAddress>,
        kind: EndpointType,
        max_packet_size: u16,
        _interval: u8,
    ) -> Result<EndpointAddress> {
        if !(1..=64).contains(&max_packet_size) || matches!(kind, EndpointType::Isochronous { .. })
        {
            return Err(UsbError::Unsupported);
        }
        let number = match (requested, kind) {
            (Some(addr), _) if addr.direction() != dir => return Err(UsbError::InvalidEndpoint),
            (Some(addr), _) => addr.index(),
            (None, EndpointType::Control) => 0,
            (None, _) => (1..EP_COUNT)
                .find(|&i| {
                    let endpoint = self.endpoints[i];
                    endpoint.input == 0 && endpoint.output == 0
                })
                .ok_or(UsbError::EndpointOverflow)?,
        };
        if (number == 0) != (kind == EndpointType::Control) {
            return Err(UsbError::InvalidEndpoint);
        }
        let endpoint = self
            .endpoints
            .get_mut(number)
            .ok_or(UsbError::InvalidEndpoint)?;
        let limit = endpoint.max_packet(dir);
        if *limit != 0 {
            return Err(UsbError::InvalidEndpoint);
        }
        *limit = max_packet_size;
        Ok(EndpointAddress::from_parts(number, dir))
    }

    fn enable(&mut self) {
        self.reset();
        self.io.write8(POWER, 1 << 6); // SOFTCONN, full speed
    }

    fn reset(&self) {
        let mut controller = self.controller.lock();
        *controller = Controller::default();
        self.select(0);
        self.io.write8(FUNC_ADDR, 0);
        let mut tx = 1;
        let mut rx = 0;
        for (ep, cfg) in self.endpoints.iter().enumerate().skip(1) {
            self.select(ep);
            if cfg.output > 0 {
                self.configure_fifo(ep, UsbDirection::Out, cfg.output);
                rx |= 1 << ep;
            }
            if cfg.input > 0 {
                self.configure_fifo(ep, UsbDirection::In, cfg.input);
                tx |= 1 << ep;
            }
        }
        self.io.write16(TX_IRQ, u16::MAX);
        self.io.write16(RX_IRQ, u16::MAX);
        self.io.write16(0x48, tx);
        self.io.write16(0x4a, rx);
        self.io.write8(0x50, 0x27);
    }

    fn set_device_address(&self, address: u8) {
        let _index = self.controller.lock();
        self.select(0);
        self.io.write8(FUNC_ADDR, address);
    }

    fn read(&self, addr: EndpointAddress, buffer: &mut [u8]) -> Result<usize> {
        let capacity = self.capacity(addr, UsbDirection::Out)?;
        let ep = addr.index();
        let mut ctl = self.controller.lock();
        self.select(ep);
        let csr = if ep == 0 { TXCSR } else { RXCSR };
        let status = self.io.read16(csr);
        if status & 1 == 0 || (ep == 0 && matches!(ctl.phase, Phase::Accept)) {
            return Err(UsbError::WouldBlock);
        }
        let count = usize::from(self.io.read16(COUNT));
        let packet = buffer.get_mut(..count).ok_or(UsbError::BufferOverflow)?;
        for byte in packet.iter_mut() {
            *byte = self.io.read8(4 * ep);
        }
        if ep != 0 {
            self.io.write16(csr, status & !1);
        } else {
            ctl.phase = match (&ctl.phase, &packet[..]) {
                (Phase::Request, [request, _, _, _, _, _, lo, hi]) => {
                    let length = usize::from(u16::from_le_bytes([*lo, *hi]));
                    match (length, request & 0x80) {
                        (0, _) => Phase::Accept,
                        (_, 0) => Phase::FromHost(length),
                        _ => Phase::ToHost(length),
                    }
                }
                (Phase::FromHost(left), _) if count < *left && count == capacity => {
                    Phase::FromHost(left - count)
                }
                (Phase::FromHost(_), _) => Phase::Accept,
                _ => Phase::Request,
            };
            if !matches!(ctl.phase, Phase::Accept) {
                self.io.write16(csr, SERVICE_RX);
            }
        }
        Ok(count)
    }

    fn write(&self, addr: EndpointAddress, packet: &[u8]) -> Result<usize> {
        let capacity = self.capacity(addr, UsbDirection::In)?;
        if packet.len() > capacity {
            return Err(UsbError::BufferOverflow);
        }
        let ep = addr.index();
        let mut ctl = self.controller.lock();
        self.select(ep);
        if ctl.transmitting & (1 << ep) != 0 {
            return Err(UsbError::WouldBlock);
        }
        let command = match (ep, &ctl.phase) {
            (0, Phase::Accept) if packet.is_empty() => {
                ctl.phase = Phase::Request;
                SERVICE_RX | DATA_END
            }
            (0, Phase::ToHost(left)) if packet.len() < capacity || packet.len() >= *left => {
                ctl.phase = Phase::Request;
                EP0_SEND | DATA_END
            }
            (0, Phase::ToHost(left)) => {
                ctl.phase = Phase::ToHost(left - packet.len());
                EP0_SEND
            }
            (0, _) => return Err(UsbError::InvalidState),
            _ => self.io.read16(TXCSR) | 1,
        };
        for &value in packet {
            self.io.write8(4 * ep, value);
        }
        self.io.write16(TXCSR, command);
        ctl.transmitting |= 1 << ep;
        Ok(packet.len())
    }

    fn set_stalled(&self, addr: EndpointAddress, stalled: bool) {
        if self.capacity(addr, addr.direction()).is_err() {
            return;
        }
        let mut ctl = self.controller.lock();
        self.select(addr.index());
        let (csr, request, sent, toggle) = Self::halt_bits(addr);
        if addr.index() == 0 {
            if stalled {
                self.io.write16(csr, request | SERVICE_RX);
                ctl.phase = Phase::Request;
            }
        } else {
            let value = self.io.read16(csr);
            self.io.write16(
                csr,
                if stalled {
                    value | request
                } else {
                    (value & !(request | sent)) | toggle
                },
            );
        }
    }

    fn is_stalled(&self, addr: EndpointAddress) -> bool {
        if self.capacity(addr, addr.direction()).is_err() {
            return false;
        }
        let _index = self.controller.lock();
        self.select(addr.index());
        let (csr, request, _, _) = Self::halt_bits(addr);
        self.io.read16(csr) & request != 0
    }

    fn suspend(&self) {}
    fn resume(&self) {}

    fn poll(&self) -> PollResult {
        let mut ctl = self.controller.lock();
        let events = self.io.read8(BUS_IRQ);
        self.io.write8(BUS_IRQ, events);
        if events & 0x24 != 0 {
            return PollResult::Reset;
        }

        let tx = self.io.read16(TX_IRQ);
        let rx = self.io.read16(RX_IRQ);
        self.io.write16(TX_IRQ, tx);
        self.io.write16(RX_IRQ, rx);
        let mut out = 0;
        // Report readable FIFOs until the class consumes them, even if the
        // class deferred a read on the original interrupt.
        for ep in 1..EP_COUNT {
            if self.endpoints[ep].output != 0 {
                self.select(ep);
                out |= (self.io.read16(RXCSR) & 1) << ep;
            }
        }
        self.select(0);
        let mut status = self.io.read16(TXCSR);
        if status & 0x14 != 0 {
            // setup end or sent stall
            self.io.write16(
                TXCSR,
                if status & 0x10 != 0 {
                    SERVICE_SETUP_END
                } else {
                    0
                },
            );
            ctl.phase = Phase::Request;
            ctl.transmitting &= !1;
            status = self.io.read16(TXCSR);
        }
        let mut completed = tx & ctl.transmitting;
        if status & EP0_SEND != 0 {
            completed &= !1;
        }
        ctl.transmitting &= !completed;
        let mut setup = 0;
        if status & 1 != 0 {
            match ctl.phase {
                Phase::Request if self.io.read16(COUNT) == 8 => setup = 1,
                Phase::Accept => {}
                _ => out |= 1,
            }
        }
        if (setup | completed | out) != 0 {
            PollResult::Data {
                ep_out: out,
                ep_in_complete: completed,
                ep_setup: setup,
            }
        } else {
            match events & 3 {
                0 => PollResult::None,
                1 => PollResult::Suspend,
                _ => PollResult::Resume,
            }
        }
    }
}
