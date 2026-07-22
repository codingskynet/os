//! Universal Asynchronous Receiver/Transmitter (UART) abstraction.
//!
//! Concrete serial drivers expose small read and `fmt::Write` implementations
//! that the console layer can install without knowing each device's register
//! layout.

pub mod ns16550;

/// Non-blocking byte-stream input with partial-read semantics.
pub trait Read {
    type Error;

    /// Read bytes received from the device into `buffer`.
    ///
    /// `Some(0)` denotes an empty input buffer, while `None` means that the
    /// device would block waiting for input. Once input is available, the
    /// implementation may return before the entire buffer has been filled.
    fn read(&mut self, buffer: &mut [u8]) -> Result<Option<usize>, Self::Error>;
}

/// Blocking byte-stream output with complete-write semantics.
pub trait Write {
    type Error;

    /// Write every byte in `buffer`, returning the number written.
    fn write(&mut self, buffer: &[u8]) -> Result<usize, Self::Error>;
}
