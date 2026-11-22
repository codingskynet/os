//! MIPI DSI wire encoding for virtual channel zero.
// ICNL9707 5.3.10. Column n gives the six parity bits affected by D[n].
pub(crate) fn header(data_type: u8, field: u16) -> u32 {
    const COLUMNS: [u8; 24] = [
        7, 11, 13, 14, 19, 21, 22, 25, 26, 28, 35, 37, 38, 41, 42, 44, 49, 50, 52, 56, 31, 47, 55,
        59,
    ];
    let [lo, hi] = field.to_le_bytes();
    let mut data = u32::from_le_bytes([data_type, lo, hi, 0]);
    let mut parity = 0;
    while data != 0 {
        parity ^= COLUMNS[data.trailing_zeros() as usize];
        data &= data - 1;
    }
    u32::from_le_bytes([data_type, lo, hi, parity])
}

pub(crate) fn dcs_short_write(opcode: u8, parameter: Option<u8>) -> u32 {
    let (data_type, arg) = match parameter {
        None => (0x05, 0),
        Some(value) => (0x15, value),
    };
    header(data_type, u16::from_le_bytes([opcode, arg]))
}

// ICNL9707 5.3.11. Consume payload bits least significant first.
pub(crate) fn payload_crc(bytes: impl Iterator<Item = u8>) -> u16 {
    bytes.fold(u16::MAX, |crc, byte| {
        let mut state = crc ^ u16::from(byte);
        for _ in 0..8 {
            let feedback = 0u16.wrapping_sub(state & 1);
            state = (state >> 1) ^ (feedback & 0x8408);
        }
        state
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleep_in_matches_datasheet_example() {
        // ICNL9707 p.55. A command with an explicit zero parameter has
        // the same two data bytes but a different packet type and parity.
        assert_eq!(
            dcs_short_write(0x10, None).to_le_bytes(),
            [5, 0x10, 0, 0x2c]
        );
        assert_eq!(
            dcs_short_write(0x10, Some(0)).to_le_bytes(),
            [0x15, 0x10, 0, 0x3f]
        );
    }

    #[test]
    fn payload_checksums() {
        assert_eq!(payload_crc(core::iter::empty()), 0xffff);
        assert_eq!(payload_crc(b"123456789".iter().copied()), 0x6f91);
        assert_eq!(payload_crc(core::iter::repeat_n(0, 110)), 0x1296);
    }
}
