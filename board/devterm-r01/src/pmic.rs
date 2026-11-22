//! AXP228 register transactions using D1 TWI0 master mode (D1-H 9.1).
use runtime::kernel::power::POWER;
use runtime::mm::mmio::Mapper;

use crate::hw::{Registers, Result, delay_us};

const DATA: usize = 8;
const CONTROL: usize = 12;
const STATUS: usize = 16;
const BUS_ENABLE: u32 = 1 << 6;
const ADVANCE: u32 = 1 << 3; // write one to acknowledge a completed bus state
const START: u32 = 1 << 5;
const STOP: u32 = 1 << 4;
const ADDRESS: u8 = 0x34;

struct Axp228(Registers);

impl Axp228 {
    fn step(&self, action: u32, expected_status: u32) -> Result<()> {
        self.0.set(CONTROL, BUS_ENABLE | ADVANCE | action);
        if !self.0.wait(CONTROL, ADVANCE, ADVANCE, 5_000) {
            return Err("TWI0 transfer timed out");
        }
        if self.0.get(STATUS) != expected_status {
            return Err("unexpected TWI0 bus state");
        }
        Ok(())
    }

    fn transmit(&self, byte: u8, ack: u32) -> Result<()> {
        self.0.set(DATA, byte.into());
        self.step(0, ack)
    }

    // Each transaction selects a register, then writes or reads one byte.
    fn register(&self, reg: u8, write: Option<u8>) -> Result<u8> {
        let transaction = || {
            self.step(START, 0x08)?;
            self.transmit(ADDRESS << 1, 0x18)?;
            self.transmit(reg, 0x28)?;
            if let Some(value) = write {
                self.transmit(value, 0x28)?;
                return Ok(value);
            }
            self.step(START, 0x10)?;
            self.transmit((ADDRESS << 1) | 1, 0x40)?;
            self.step(0, 0x58)?; // NACK the last received byte.
            Ok(self.0.get(DATA) as u8)
        };
        let result = transaction();
        self.0.set(CONTROL, BUS_ENABLE | ADVANCE | STOP);
        if !self.0.wait(CONTROL, STOP, 0, 5_000) {
            return Err("TWI0 stop timed out");
        }
        result
    }

    fn change(&self, reg: u8, mask: u8, bits: u8) -> Result<()> {
        let old = self.register(reg, None)?;
        self.register(reg, Some((old & !mask) | bits))?;
        Ok(())
    }
}

pub(crate) fn init(mapper: &mut Mapper, gpio: Registers, ccu: Registers) -> Result<()> {
    let bus = Registers::map(mapper, 0x0250_2000, 0x400)?;
    ccu.update(0x91c, 0x10001, 0x10001);
    for pin in [10, 11] {
        gpio.pin_function(0x30, pin, 4); // PB10 SCL, PB11 SDA
    }
    bus.set(0x18, 1);
    if !bus.wait(0x18, 1, 0, 5_000) {
        return Err("TWI0 reset timed out");
    }
    // Fscl = APB1 / (10 * (M+1) * 2^N). Allow APB1 up to 200 MHz.
    bus.set(0x14, (12 << 3) | 4);
    bus.set(0x1c, 0);
    bus.set(CONTROL, BUS_ENABLE);
    let pmic = Axp228(bus);
    // 0.7 V + 26 * 0.1 V. Program all parallel external outputs before enabling them.
    for supply in [0x29, 0x16, 0x17, 0x18] {
        pmic.change(supply, 0x1f, 26)?;
    }
    pmic.change(0x10, 0, (1 << 7) | (1 << 3) | (1 << 1))?;
    pmic.change(0x12, 0, 0b111 << 4)?;
    POWER.install_power_off(move || {
        pmic.change(0x32, 0, 1 << 7)?;
        delay_us(500_000);
        Err("AXP228 power-off request did not complete")
    });
    Ok(())
}
