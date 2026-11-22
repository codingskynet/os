use alloc::boxed::Box;
use alloc::vec;

/// A fixed number of bits, initially all clear.
#[derive(Default)]
pub struct Bitmap {
    bytes: Box<[u8]>,
    bit_count: usize,
}

impl Bitmap {
    pub fn new(bit_count: usize) -> Self {
        Self {
            bytes: vec![0; bit_count.div_ceil(8)].into_boxed_slice(),
            bit_count,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.bit_count == 0
    }

    pub fn is_set(&self, index: usize) -> bool {
        let (byte, mask) = self.position(index);
        self.bytes[byte] & mask != 0
    }

    pub fn set(&mut self, index: usize) {
        let (byte, mask) = self.position(index);
        self.bytes[byte] |= mask;
    }

    fn position(&self, index: usize) -> (usize, u8) {
        debug_assert!(index < self.bit_count, "bitmap index out of bounds");
        (index / 8, 1 << (index % 8))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_are_independent_across_bytes() {
        assert!(Bitmap::default().is_empty());
        let mut bits = Bitmap::new(10);
        assert!(!bits.is_empty());
        for index in [7, 8, 8] {
            bits.set(index);
        }
        for index in 0..10 {
            assert_eq!(bits.is_set(index), index == 7 || index == 8);
        }
    }

    #[test]
    #[should_panic(expected = "bitmap index out of bounds")]
    fn rejects_padding_in_the_last_byte() {
        Bitmap::new(10).set(10);
    }
}
