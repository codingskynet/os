# Drivers

This crate contains the shared RISC-V [UART](src/uart), [timer](src/timer)
and [interrupt controller](src/interrupt) drivers.

To add a driver, put its Rust source in the matching directory and export a
`DRIVER` constant of type `ConsoleDriver`, `TimerDriver` or `InterruptDriver`.
The build collects these descriptors into `driver::AllDevices`, which the
QEMU board uses. The descriptor types are defined in [boot/src/drivers.rs](../boot/src/drivers.rs).

A board can also select individual drivers with `use_driver!` and list them
in its `DriverSet`. See the [LiteX board definition](../board/litex-vexriscv/src/drivers.rs)
for a complete example.

After building the root filesystem with `make initarfs`, run
`cargo check -p driver` from the repository root to check the driver library.
