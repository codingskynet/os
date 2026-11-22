//! Landscape console on a portrait RGB scanout (same orientation as fbcon=rotate:1).
use alloc::boxed::Box;
use alloc::vec;

pub(super) const WIDTH: usize = 480;
pub(super) const HEIGHT: usize = 1280;
const COLUMNS: usize = HEIGHT / 8;
const ROWS: usize = WIDTH / 16;
// Public-domain IBM VGA glyphs, from dhepper/font8x8, printable ASCII only.
const FONT: &[u8; 95 * 8] = include_bytes!("ascii8x8.bin");

#[derive(Clone, Copy)]
enum Escape {
    Plain,
    Esc,
    Csi(usize),
}

pub struct TextConsole {
    pixels: &'static mut [u32],
    cells: Box<[u8]>,
    escape: Escape,
    column: usize,
    row: usize,
}

impl TextConsole {
    pub(super) fn new(pixels: &'static mut [u32]) -> Self {
        assert_eq!(pixels.len(), WIDTH * HEIGHT);
        assert!(pixels.as_ptr().cast::<u64>().is_aligned());
        for pixel in pixels.iter_mut() {
            unsafe { core::ptr::write_volatile(pixel, 0) };
        }
        Self {
            pixels,
            cells: vec![b' '; COLUMNS * ROWS].into_boxed_slice(),
            escape: Escape::Plain,
            column: 0,
            row: 0,
        }
    }

    fn draw(&mut self, col: usize, row: usize, ch: u8) {
        let cell = row * COLUMNS + col;
        if self.cells[cell] == ch {
            return;
        }
        let old = usize::from(self.cells[cell] - b' ') * 8;
        let new = usize::from(ch - b' ') * 8;
        self.cells[cell] = ch;
        for y in 0..8 {
            let bits = FONT[new + y];
            let mut changes = FONT[old + y] ^ bits;
            while changes != 0 {
                let x = changes.trailing_zeros() as usize;
                changes &= changes - 1;
                // A doubled font pixel occupies two adjacent XRGB words on
                // the rotated scanout. The framebuffer and each pair are aligned.
                let offset = (col * 8 + x) * WIDTH + WIDTH - (row * 16 + y * 2 + 2);
                let pair: u64 = if bits & (1 << x) != 0 {
                    0x00ff_ffff_00ff_ffff
                } else {
                    0
                };
                unsafe {
                    self.pixels
                        .as_mut_ptr()
                        .add(offset)
                        .cast::<u64>()
                        .write_volatile(pair)
                };
            }
        }
    }

    fn newline(&mut self) {
        self.column = 0;
        if self.row + 1 < ROWS {
            self.row += 1;
            return;
        }
        for row in 0..ROWS {
            for col in 0..COLUMNS {
                let ch = self
                    .cells
                    .get((row + 1) * COLUMNS + col)
                    .copied()
                    .unwrap_or(b' ');
                self.draw(col, row, ch);
            }
        }
    }

    pub fn write(&mut self, data: &[u8]) {
        for &byte in data {
            // MicroPython readline emits cursor-left and erase-line sequences.
            // One escape sequence may span several console writes.
            match (self.escape, byte) {
                (_, 0x1b) => self.escape = Escape::Esc,
                (Escape::Esc, b'[') => self.escape = Escape::Csi(0),
                (Escape::Csi(n), b'0'..=b'9') => {
                    self.escape = Escape::Csi((n * 10 + usize::from(byte - b'0')).min(COLUMNS));
                }
                (Escape::Csi(n), b'D') => {
                    self.column = self.column.saturating_sub(n.max(1));
                    self.escape = Escape::Plain;
                }
                (Escape::Csi(0), b'K') => {
                    for col in self.column..COLUMNS {
                        self.draw(col, self.row, b' ');
                    }
                    self.escape = Escape::Plain;
                }
                (Escape::Csi(_), 0x20..=0x3f) => {}
                (Escape::Esc | Escape::Csi(_), _) => self.escape = Escape::Plain,
                (Escape::Plain, byte) => match byte {
                    b'\n' => self.newline(),
                    b'\r' => self.column = 0,
                    8 => self.column = self.column.saturating_sub(1),
                    b'\t' => self.column = ((self.column + 8) & !7).min(COLUMNS),
                    32..=126 => {
                        if self.column == COLUMNS {
                            self.newline();
                        }
                        self.draw(self.column, self.row, byte);
                        self.column += 1;
                    }
                    _ => {}
                },
            }
        }
    }
}
