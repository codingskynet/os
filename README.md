# OS

The toy project for implementing OS

## How to Setup & Build & Run

```bash
make setup          # install dependencies including QEMU
make run            # build + create kernel.img/kernel.elf/kernel.debug + boot on QEMU
make run DEBUG=1    # build debug image + boot on QEMU
make build          # build only (ELF)
make image          # build + create boot image + profiler/debug artifacts
make clean          # remove artifacts
```

# Reference
- https://github.com/rust-embedded/rust-raspberrypi-OS-tutorials
- https://github.com/cccriscv/mini-riscv-os
