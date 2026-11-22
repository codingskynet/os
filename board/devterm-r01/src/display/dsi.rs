//! Four-lane RGB888 sync-pulse video. Commands use virtual channel zero.
use super::{HEIGHT, Mode, mipi};
use crate::hw::{Registers, Result, delay_us};

fn video_packets(io: Registers, mode: Mode) {
    const SHORT: u16 = 4;
    const LONG: u16 = 6;
    let sync = Mode::HSYNC;
    let line = mode.htotal();
    let after_sync = (line - sync) * 3 - SHORT - LONG;
    let front = line - mode.hbp - mode.width;
    let frame = u32::from(line) * 3 * u32::from(Mode::LINES);
    let tail = (frame - u32::from(after_sync + LONG)) % 4;
    io.set(0x90, mipi::header(0x3e, 3 * mode.width));
    io.set(0x98, 0xffff);
    io.set(0x9c, u32::MAX);
    for (n, event) in [0x21, 0x31, 0x01, 0x11].into_iter().enumerate() {
        io.set(0xb0 + n * 4, mipi::header(event, 0));
    }
    for (offset, count) in [
        (0xc0, sync * 3 - SHORT - LONG),
        (0xc8, (mode.hbp - sync) * 3 - LONG),
        (0xd0, front * 3 - SHORT - 2 * LONG),
        (0xe0, after_sync),
        (0xe8, 4 - tail as u16),
    ] {
        io.set(offset, mipi::header(0x19, count));
        let crc = mipi::payload_crc(core::iter::repeat_n(0, usize::from(count)));
        io.set(offset + 4, u32::from(crc) << 16);
    }
}

const STOP_STATE: u32 = 0;
const HS_CLOCK: u32 = 2;
const HS_VIDEO: u32 = 3;
const LP_COMMAND: u32 = 4;
const CLOCK_EXIT: u32 = 5;
const DATA_STOP: u32 = 6;
const DELAY: u32 = 7;
const END: u32 = 15;
const CLOCK: u32 = 1 << 4;
const LANES: u32 = 15;

// Each slot has a four-bit successor in the sequencer's jump register.
fn execute(io: Registers, edges: &[(u32, u32)]) {
    let mut jumps = 0;
    for &(from, to) in edges {
        jumps |= to << (4 * from);
    }
    io.update(0x10, 1, 0);
    io.set(0x48, jumps);
    io.update(0x10, 1, 1);
}

fn phy(io: Registers) {
    // D1 combo PHY, 150 MHz timing reference and continuous clock.
    let cycles = |ns: u32| (ns * 150).div_ceil(1000);
    io.set(0x00, 3 << 4); // four data lanes, digital sequencer stopped
    io.set(0x04, 1 << 28);
    io.set(0x10, cycles(80) | (cycles(65) << 16) | (cycles(85) << 24));
    io.set(
        0x14,
        cycles(55) | (cycles(300) << 8) | (cycles(32) << 16) | (cycles(250) << 24),
    );
    io.set(0x18, cycles(100));
    io.set(0x20, 3 | (3 << 8));

    // Electrical trim codes for the R-01 link, in the D1 ANA4 fields.
    let pull_strength = 2 | (3 << 2) | (2 << 4) | (3 << 6);
    let termination = (1 << 8) | (1 << 10) | (3 << 17);
    let bias = (3 << 12) | (4 << 20) | (2 << 25);
    io.set(0x5c, pull_strength | termination | bias);
    io.set(0x4c, (1 << 2) | (4 << 4)); // feedback and line resistance trim
    io.set(0x54, (1 << 1) | (1 << 4)); // bias and clock
    let regulators = (1 << 18) | (3 << 24);
    io.set(0x58, regulators);
    io.set(0x110, 1); // charge pump
    // 24 MHz * 110 / 8 = 330 Mbit/s/lane; 24 bpp * 55 MHz / 4 lanes.
    io.set(0x10c, 0); // integer PLL
    io.set(0x104, (15 << 20) | (7 << 16) | (110 << 8) | (1 << 7) | 2);
    io.set(0x118, 20);
    io.update(0x5c, 1 << 31, 1 << 31);
    io.set(0x110, (1 << 3) | (1 << 1) | 1); // MIPI output, LDO, charge pump
    delay_us(10);
    io.set(0x58, regulators | (1 << 26) | (1 << 27) | (LANES << 28));
    delay_us(10);
    io.set(0x50, 1 << 31); // VTT mode
    io.update(0x54, LANES << 24, LANES << 24); // parallel-to-serial drivers
    io.update(0, 1, 1);
}

pub(super) fn prepare(io: Registers, mode: Mode) {
    io.set(0, 1);
    io.set(0x10, 3 << 16); // ECC and CRC engines
    let start_line = Mode::LINES - Mode::VSYNC - Mode::VBACK - 3;
    io.set(0x14, u32::from(start_line) << 4 | 7);
    io.set(
        0x18,
        (u32::from(Mode::VBACK) << 16) | u32::from(Mode::VSYNC),
    );
    io.set(0x1c, (u32::from(Mode::LINES) << 16) | HEIGHT as u32);
    io.set(0x80, (1 << 16) | 8); // RGB888 from TCON, MSB first
    video_packets(io, mode);
    let slot =
        |n, instruction: u32, signals| io.set(0x20 + n as usize * 4, instruction << 28 | signals);
    slot(STOP_STATE, 0, CLOCK | LANES);
    slot(HS_CLOCK, 2, CLOCK);
    slot(HS_VIDEO, 2, LANES);
    slot(LP_COMMAND, 3, (1 << 20) | 1);
    slot(CLOCK_EXIT, 4, CLOCK);
    slot(DATA_STOP, 0, LANES);
    slot(DELAY, 5, CLOCK | LANES);
    io.set(0x40, 2 | (3 << 28)); // loop counters for stop and delay slots
    for bank in [0x44, 0x54] {
        io.set(bank, (49 << 16) | 49);
    }
    io.set(0x4c, (CLOCK_EXIT << 20) | (DATA_STOP << 16) | 1);
    io.set(0x60, u32::from(Mode::VSYNC));
    io.set(0x7c, (1 << 28) | 33);
    phy(io.offset(0x1000));
}

fn dcs(io: Registers, opcode: u8, parameter: Option<u8>) -> Result<()> {
    io.set(0x300, mipi::dcs_short_write(opcode, parameter));
    io.set(0x200, 4 - 1); // one complete short packet
    execute(io, &[(STOP_STATE, LP_COMMAND), (LP_COMMAND, END)]);
    io.wait(0x10, 1, 0, 5000)
        .then_some(())
        .ok_or("DSI command timed out")
}

pub(super) fn wake(io: Registers) -> Result<()> {
    // Reset loads the panel's OTP settings. ICNL9707 8.2 level-one commands.
    dcs(io, 0x36, Some((1 << 4) | (1 << 2)))?; // reverse both refresh directions
    dcs(io, 0x3a, Some(0x70))?; // 24-bit video
    dcs(io, 0x11, None)?; // Exit Sleep Mode
    delay_us(120_000);
    dcs(io, 0x29, None)?; // Set Display On
    delay_us(20_000);
    execute(io, &[(STOP_STATE, HS_CLOCK), (HS_CLOCK, END)]);
    if !io.wait(0x10, 1, 0, 5_000) {
        return Err("DSI clock start timed out");
    }
    io.update(0x20, CLOCK, 0); // leave clock running between video packets
    Ok(())
}

pub(super) fn stream(io: Registers) {
    execute(
        io,
        &[
            (STOP_STATE, DATA_STOP),
            (DATA_STOP, HS_VIDEO),
            (HS_VIDEO, DELAY),
            (DELAY, DATA_STOP),
            (CLOCK_EXIT, END),
        ],
    );
}
