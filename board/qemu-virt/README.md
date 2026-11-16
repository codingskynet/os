# QEMU virt build target

This executable depends on the shared `boot` library and enables the `driver`
features `ns16550`, `plic`, and `riscv-sstc`. The current Sstc implementation
supports RV64 and requires firmware to permit supervisor access to time/stimecmp.

Build with `cargo build -p board-qemu-virt --release` from the workspace root,
or use `make run` to build userland and boot QEMU. This board is the workspace
default member. Optional boot features use `--features boot/smoke` (or
`make run FEATURES=smoke`). The shared `board/build.rs` supplies the
architecture-specific linker script and memory-layout symbols.

The board supplies no device initialization code. FDT selects the console,
interrupt controller, and timer independently from the included drivers.
