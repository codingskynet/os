# LiteX / VexRiscv

This single-hart RV32IMA board uses Sv32 paging and a LiteUART console. The
LiteX BIOS loads the kernel at `0x40000000` and passes the hart ID in `a0`
and the physical device-tree address in `a1`.

## Build

From the repository root, prepare the MicroPython port and build an RV32
root filesystem. This needs a `riscv-none-elf-` GCC toolchain with RV32IMA/ILP32
support. Set `CROSS_COMPILE` if your toolchain uses a different prefix.

```sh
make ports-prepare PORTS=micropython
make -C userland initarfs ARCH=riscv32imac-unknown-none-elf \
  SUBDIRS=ports/micropython/ports/os BUILD=build-rv32
```

Build the kernel and its Rust libraries without compressed instructions.

```sh
RUSTFLAGS='-C target-feature=-c,-zca -C relocation-model=static -C code-model=medium' \
  cargo build -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem \
  --release --target riscv32imac-unknown-none-elf -p board-litex-vexriscv
```

The ELF is `target/riscv32imac-unknown-none-elf/release/board-litex-vexriscv`.
RV32 and RV64 builds share `artifacts/initarfs`. Build one architecture at a
time and regenerate the archive when switching.
