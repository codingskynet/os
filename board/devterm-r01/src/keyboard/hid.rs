//! US ASCII input from the six-key HID keyboard report used by DevTerm.
#[derive(Default)]
pub(super) struct Keys {
    previous: [u8; 6],
    caps: bool,
}

impl Keys {
    pub fn report(&mut self, mut bytes: &[u8], id: Option<u8>, emit: &mut dyn FnMut(u8)) {
        if let Some(id) = id {
            if bytes.first() != Some(&id) {
                return;
            }
            bytes = &bytes[1..];
        }
        let Some(report) = bytes.first_chunk::<8>() else {
            return;
        };
        let pressed = &report[2..8];
        if pressed.iter().any(|k| (1..=3).contains(k)) {
            return;
        } // rollover
        let shift = report[0] & 0x22 != 0;
        let ctrl = report[0] & 0x11 != 0;
        for &k in pressed {
            if k == 0 || self.previous.contains(&k) {
                continue;
            }
            match k {
                0x39 => self.caps = !self.caps,
                4..=29 => {
                    let letter = b'a' + (k - 4);
                    emit(if ctrl {
                        letter & 31
                    } else if shift ^ self.caps {
                        letter.to_ascii_uppercase()
                    } else {
                        letter
                    });
                }
                30..=39 => {
                    let digits = if shift { b"!@#$%^&*()" } else { b"1234567890" };
                    emit(digits[usize::from(k - 30)]);
                }
                40 => emit(b'\r'),
                41 => emit(27),
                42 => emit(8),
                43 => emit(b'\t'),
                44..=56 => {
                    let plain = b" -=[]\\#;'\x60,./";
                    let upper = b" _+{}|~:\"~<>?";
                    let c = if shift { upper } else { plain }[usize::from(k - 44)];
                    emit(if ctrl { c & 31 } else { c });
                }
                76 => b"\x1b[3~".iter().copied().for_each(&mut *emit),
                79..=82 => {
                    emit(27);
                    emit(b'[');
                    emit(b"CDBA"[usize::from(k - 79)]);
                }
                _ => {}
            }
        }
        self.previous.copy_from_slice(pressed);
    }
}

/// Find the report ID within the Generic Desktop / Keyboard application
/// collection. Mouse, consumer and joystick collections share the endpoint.
pub(super) fn keyboard_report_id(bytes: &[u8]) -> Option<u8> {
    let mut page = 0;
    let mut usage = 0;
    let mut depth = 0usize;
    let mut keyboard = None;
    let mut items = bytes;
    while let Some((&tag, rest)) = items.split_first() {
        if tag == 0xfe {
            return None;
        }
        let n = [0, 1, 2, 4][usize::from(tag & 3)];
        let data = rest.get(..n)?;
        items = &rest[n..];
        let value = data
            .iter()
            .enumerate()
            .fold(0u32, |v, (i, b)| v | (u32::from(*b) << (8 * i)));
        match tag & 0xfc {
            0x04 => page = value,
            0x08 => usage = value,
            0xa0 => {
                depth += 1;
                if page == 1 && usage == 6 && value == 1 {
                    keyboard = Some(depth);
                }
                usage = 0;
            }
            0xc0 => {
                if keyboard == Some(depth) {
                    keyboard = None;
                }
                depth = depth.checked_sub(1)?;
                usage = 0;
            }
            0x84 if keyboard.is_some() => return u8::try_from(value).ok().filter(|id| *id != 0),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn composite_input_and_release() {
        let desc = [
            5, 1, 9, 2, 0xa1, 1, 0x85, 1, 0xc0, 5, 1, 9, 6, 0xa1, 1, 0x85, 2, 0xc0,
        ];
        let id = keyboard_report_id(&desc);
        assert_eq!(id, Some(2));
        let mut keys = Keys::default();
        let mut text = Vec::new();
        for (rid, modifier, key) in [
            (1, 0, 8),
            (2, 2, 4),
            (2, 2, 4),
            (2, 0, 0),
            (2, 1, 6),
            (2, 0, 40),
        ] {
            keys.report(&[rid, modifier, 0, key, 0, 0, 0, 0, 0], id, &mut |c| {
                text.push(c)
            });
        }
        assert_eq!(text, b"A\x03\r");
    }
}
