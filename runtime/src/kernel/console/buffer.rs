//! Fixed-capacity buffers for console input and pre-installation output.

const TX_CAPACITY: usize = 4096;
const RX_CAPACITY: usize = 256;

struct RingBuffer<const CAPACITY: usize> {
    bytes: [u8; CAPACITY],
    read: usize,
    len: usize,
}

impl<const CAPACITY: usize> RingBuffer<CAPACITY> {
    const EMPTY: Self = Self {
        bytes: [0; CAPACITY],
        read: 0,
        len: 0,
    };

    fn push(&mut self, byte: u8) -> bool {
        if self.len == self.bytes.len() {
            return false;
        }
        let write = (self.read + self.len) % self.bytes.len();
        self.bytes[write] = byte;
        self.len += 1;
        true
    }

    fn push_overwrite(&mut self, byte: u8) -> bool {
        let overwritten = if self.len == self.bytes.len() {
            self.read = (self.read + 1) % self.bytes.len();
            self.len -= 1;
            true
        } else {
            false
        };
        assert!(self.push(byte));
        overwritten
    }

    fn read(&mut self, buffer: &mut [u8]) -> usize {
        let read = buffer.len().min(self.len);
        for byte in &mut buffer[..read] {
            *byte = self.bytes[self.read];
            self.read = (self.read + 1) % self.bytes.len();
        }
        self.len -= read;
        read
    }
}

pub struct TxBuffer {
    ring: RingBuffer<TX_CAPACITY>,
    truncated: bool,
}

impl TxBuffer {
    pub const EMPTY: Self = Self {
        ring: RingBuffer::EMPTY,
        truncated: false,
    };

    pub fn write(&mut self, buffer: &[u8]) {
        for &byte in buffer {
            self.truncated |= self.ring.push_overwrite(byte);
        }
    }

    pub fn read(&mut self, buffer: &mut [u8]) -> usize {
        let read = self.ring.read(buffer);
        if read != 0 {
            self.truncated = false;
        }
        read
    }

    pub fn take_truncated(&mut self) -> bool {
        core::mem::take(&mut self.truncated)
    }
}

pub struct RxBuffer {
    ring: RingBuffer<RX_CAPACITY>,
}

impl RxBuffer {
    pub const EMPTY: Self = Self {
        ring: RingBuffer::EMPTY,
    };

    pub fn push(&mut self, byte: u8) -> bool {
        self.ring.push(byte)
    }

    pub fn read(&mut self, buffer: &mut [u8]) -> usize {
        self.ring.read(buffer)
    }
}

#[cfg(test)]
mod tests {
    use std::vec;
    use std::vec::Vec;

    use super::*;

    fn drain_tx(buffer: &mut TxBuffer) -> Vec<u8> {
        let mut output = vec![0; TX_CAPACITY];
        let read = buffer.read(&mut output);
        output.truncate(read);
        output
    }

    #[test]
    fn tx_preserves_write_order_across_wraparound() {
        let mut buffer = TxBuffer::EMPTY;
        buffer.write(&vec![b'a'; TX_CAPACITY - 2]);

        let mut prefix = vec![0; TX_CAPACITY - 4];
        assert_eq!(buffer.read(&mut prefix), prefix.len());
        buffer.write(b"bcde");

        assert_eq!(drain_tx(&mut buffer), b"aabcde");
        assert!(!buffer.take_truncated());
    }

    #[test]
    fn tx_overflow_keeps_the_newest_bytes() {
        let mut buffer = TxBuffer::EMPTY;
        let input: Vec<_> = (0..TX_CAPACITY + 3).map(|value| value as u8).collect();

        buffer.write(&input);

        assert!(buffer.take_truncated());
        assert!(!buffer.take_truncated());
        assert_eq!(drain_tx(&mut buffer), input[3..]);
        assert!(!buffer.take_truncated());
    }

    #[test]
    fn reading_past_truncation_clears_it() {
        let mut buffer = TxBuffer::EMPTY;
        buffer.write(&vec![b'a'; TX_CAPACITY + 1]);

        let mut first = [0];
        assert_eq!(buffer.read(&mut first), 1);

        assert!(!buffer.take_truncated());
    }

    #[test]
    fn rx_overflow_keeps_already_buffered_input() {
        let mut buffer = RxBuffer::EMPTY;
        let input: Vec<_> = (0..RX_CAPACITY).map(|value| value as u8).collect();
        for &byte in &input {
            assert!(buffer.push(byte));
        }

        assert!(!buffer.push(0xff));

        let mut output = vec![0; RX_CAPACITY];
        assert_eq!(buffer.read(&mut output), RX_CAPACITY);
        assert_eq!(output, input);
    }
}
