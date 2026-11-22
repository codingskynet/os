//! CDC ACM on the main USB-C connector. The LCD never waits for a host.
mod bus;

use alloc::boxed::Box;
use alloc::collections::VecDeque;

use bus::Bus;
use runtime::arch::asm::time::ticks;
use runtime::mm::mmio::Mapper;
use usb_device::bus::UsbBusAllocator;
use usb_device::device::UsbRev;
use usb_device::prelude::*;
use usbd_serial::CdcAcmClass;

use crate::hw::{Registers, Result};

const PACKET: usize = 64;
const HISTORY: usize = 4096;

pub(crate) struct Cdc {
    device: Box<UsbDevice<'static, Bus>>,
    serial: CdcAcmClass<'static, Bus>,
    tx: VecDeque<u8>,
}

impl Cdc {
    pub(crate) fn new(mapper: &mut Mapper, ccu: Registers) -> Result<Self> {
        let bus = Box::leak(Box::new(UsbBusAllocator::new(Bus::new(mapper, ccu)?)));
        let serial = CdcAcmClass::new(bus, PACKET as u16);
        let device = Self::device(bus);
        Ok(Self {
            device,
            serial,
            tx: VecDeque::with_capacity(HISTORY),
        })
    }

    // usb-device's builder takes its inline descriptor table by value. Keep
    // construction on the boot stack and the finished device on the heap.
    #[inline(never)]
    #[expect(clippy::large_stack_frames)]
    fn device(bus: &'static UsbBusAllocator<Bus>) -> Box<UsbDevice<'static, Bus>> {
        // pid.codes test PID, for this development image.
        Box::new(
            UsbDeviceBuilder::new(bus, UsbVidPid(0x1209, 0x0001))
                .device_class(usbd_serial::USB_CLASS_CDC)
                .usb_rev(UsbRev::Usb200)
                .max_packet_size_0(64)
                .expect("valid EP0 size")
                .strings(&[StringDescriptors::default().product("DevTerm R-01 console")])
                .expect("one USB language")
                .build(),
        )
    }

    pub(crate) fn write(&mut self, data: &[u8]) {
        for &byte in data {
            if self.tx.len() == HISTORY {
                self.tx.pop_front();
            }
            self.tx.push_back(byte);
        }
        self.send();
    }

    fn send(&mut self) {
        if self.tx.is_empty()
            || !self.serial.dtr()
            || self.device.state() != UsbDeviceState::Configured
        {
            return;
        }
        // Short packets terminate bulk reads even if output stops here.
        let bytes = self.tx.as_slices().0;
        let bytes = &bytes[..bytes.len().min(PACKET - 1)];
        if let Ok(n) = self.serial.write_packet(bytes) {
            self.tx.drain(..n);
        }
    }

    pub(crate) fn poll(&mut self, space: usize, receive: &mut dyn FnMut(u8)) {
        self.device.poll(&mut [&mut self.serial]);
        if self.device.state() == UsbDeviceState::Configured && space >= PACKET {
            let mut packet = [0; PACKET];
            if let Ok(n) = self.serial.read_packet(&mut packet) {
                for &byte in &packet[..n] {
                    receive(byte);
                }
            }
        }
        self.send();
    }

    pub(crate) fn flush(&mut self) {
        let start = ticks();
        while self.serial.dtr()
            && self.device.state() == UsbDeviceState::Configured
            && ticks().wrapping_sub(start) < 24_000 * 100
        {
            self.poll(0, &mut |_| {});
            if self.tx.is_empty() && self.device.bus().tx_idle() {
                break;
            }
        }
    }
}
