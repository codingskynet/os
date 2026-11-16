# LiteX / VexRiscv build target

This executable depends on the shared `boot` library and enables the `driver` features `litex` and `vexriscv-intc`.
`litex` bundles the `liteuart` and `litex-timer` peripherals; the VexRiscv
interrupt controller is selected separately. Implementations live under
`driver/src/{console,interrupt,timer}`, and firmware FDT selects
each device independently. Supported LiteX machines use a single-hart RV32IMA
SoC with 32-bit LiteX CSRs and a timer with a 64-bit uptime counter.

The dependency direction is `board -> boot -> driver -> runtime`;
`boot` also uses the common driver discovery API directly. Runtime contains
interfaces and kernel policy, with no dependency on board or driver crates.

The FPGA repository owns RV32 image composition and upload; see the
[top-level contract](../../README.md#litex--vexriscv-rv32ima--sv32).
The kernel executable remains `board-litex-vexriscv`. Shared `board/build.rs`
supplies architecture-specific linker scripts and memory-layout symbols.
The common entry embeds `artifacts/initarfs` and starts `/bin/micropython`.

Driver features are additive. FDT selects an available implementation from
the drivers included by the board.
Addresses, timer frequency, and interrupt routing come from firmware.
See [driver discovery](../../driver/README.md) for the initialization sequence
and current supported device topologies.
