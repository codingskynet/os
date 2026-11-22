# OS

A small RISC-V operating system written in Rust. It boots into a MicroPython
REPL from an embedded root filesystem. QEMU virt (RV64) is the default target.

## Run in QEMU

With Git, Make, Python 3 and rustup installed, run these commands from the
repository root. Setup installs build tools and fetches the pinned userland ports.

```sh
make setup
make run
```

At the `>>>` prompt, enter `1 + 2`. To power off the guest, enter
`import sys; sys.exit(0)`.

## Development

- `make image` writes the kernel ELF, debug symbols and raw binary to `artifacts/`.
- `make test` runs the host tests. `make check` runs the full project checks.
- Hardware drivers live in [driver/](driver/README.md). Userland ports and their revisions are listed in [ports.toml](userland/ports.toml).

## Boards

- [QEMU virt](board/qemu-virt/README.md) — RV64 emulator, serial console and smoke tests
- [ClockworkPi DevTerm R-01](board/devterm-r01/README.md) — RV64 handheld, microSD or USB boot
- [LiteX / VexRiscv](board/litex-vexriscv/README.md) — RV32 FPGA target, LiteUART console
