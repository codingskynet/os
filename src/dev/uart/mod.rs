//! Universal Asynchronous Receiver/Transmitter (UART) abstraction.
//!
//! Any concrete serial driver (NS16550, PL011, etc.) implements [`Uart`] and
//! can be used interchangeably by the rest of the kernel.

pub mod ns16550;
