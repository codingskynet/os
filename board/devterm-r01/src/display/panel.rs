//! CWD686 control pins on the ClockworkPi v3.14 mainboard.
use crate::hw::{Registers, delay_us};

const PD: usize = 0x90;
const RESET: u32 = 1 << 19;
const LIGHT: u32 = 1 << 20;

pub(super) fn hold_reset(gpio: Registers) {
    // PD19 asserts panel reset high in the R-01 board configuration.
    gpio.update(PD + 0x10, RESET | LIGHT, RESET);
    for pin in [19, 20] {
        gpio.pin_function(PD, pin, 1);
    }
    for lane_pin in 0..10 {
        gpio.pin_function(PD, lane_pin, 4);
    }
}

pub(super) fn release_reset(gpio: Registers) {
    delay_us(10_000); // ICNL9707 table 7-1, supply rise to reset release
    gpio.update(PD + 0x10, RESET, 0);
    delay_us(120_000); // table 4-2, earliest Sleep Out
}

pub(super) fn illuminate(gpio: Registers, ccu: Registers, pwm: Registers) {
    // OCP8178 CTRL accepts continuous PWM. Feed PWM4 from HOSC at 20 kHz,
    // with half the period active for approximately half the full-scale current.
    const PERIOD: u32 = 24_000_000 / 20_000;
    ccu.update(0x7ac, 0x10001, 0x10001);
    pwm.update(0x80, 1 << 4, 0);
    pwm.update(0x28, 0x18f, 0); // channels 4/5 clock = HOSC / 1
    pwm.update(0x40, (1 << 20) | (1 << 4), 1 << 4);
    pwm.set(0x180, 1 << 8); // continuous, active high, prescaler = 1
    pwm.set(0x184, ((PERIOD - 1) << 16) | (PERIOD / 2));
    pwm.update(0x80, 1 << 4, 1 << 4);
    gpio.pin_function(PD, 20, 5);
}
