//! Universal Asynchronous Receiver/Transmitter (UART) abstraction.
//!
//! Concrete serial drivers expose small read and `fmt::Write` implementations
//! that the console layer can install without knowing each device's register
//! layout.

pub mod ns16550;

/// Blocking byte-stream input with partial-read semantics.
pub trait Read {
    type Error;

    /// Read bytes received from the device into `buffer`.
    ///
    /// Implementations return immediately when `buffer` is empty. Otherwise,
    /// they block until at least one byte is available and may return before
    /// the entire buffer has been filled.
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error>;
}

/// Blocking byte-stream output with complete-write semantics.
pub trait Write {
    type Error;

    /// Write every byte in `buffer`, returning the number written.
    fn write(&mut self, buffer: &[u8]) -> Result<usize, Self::Error>;
}
