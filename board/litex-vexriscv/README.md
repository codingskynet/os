# LiteX / VexRiscv build target

This executable depends on the shared `boot` library and enables the `driver` features `litex` and `vexriscv-intc`.
`litex` bundles the `liteuart` and `litex-timer` peripherals; the VexRiscv
interrupt controller is selected separately. Implementations live under
`driver/src/{console,interrupt,timer}`, and firmware FDT selects
each device independently. Tang Nano 9K hosts the supported single-hart RV32IMA
SoC with 32-bit LiteX CSRs and a timer with a 64-bit uptime counter.

The dependency direction is `board -> boot -> driver -> runtime`;
`boot` also uses the common driver discovery API directly. Runtime contains
interfaces and kernel policy, with no dependency on board or driver crates.

`make tangnano-image` builds `-p board-litex-vexriscv` with the RV32 target
and CPU flags. The shared `board/build.rs` supplies the architecture-specific
linker script and memory-layout symbols. The common entry embeds `artifacts/initarfs` and runs
`/bin/micropython`. The diagnostic Rust init remains in `userland/tangnano`.
Assembly entry and the existing physical/virtual kernel layout are unchanged.

Driver features are additive. FDT selects an available implementation from
the drivers included by the board.
Addresses, timer frequency, and interrupt routing come from firmware.
See [driver discovery](../../driver/README.md) for the initialization sequence
and current supported device topologies.
