//! Universal Asynchronous Receiver/Transmitter (UART) abstraction.
//!
//! Concrete serial drivers expose small `fmt::Write` implementations that the
//! console layer can install without knowing each device's register layout.

pub mod ns16550;
