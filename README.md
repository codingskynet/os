# OS

The toy project for implementing OS

## How to Setup & Build & Run

```bash
make setup          # install dependencies including QEMU
make run            # build artifacts/kernel.{elf,debug,bin} + boot on QEMU
make run DEBUG=1    # build artifacts/kernel-debug.* + boot on QEMU
make build          # build the boot ELF in Cargo's target directory
make image          # copy the ELF and create debug + raw binary artifacts
make clean          # remove artifacts
```

Run the same formatting, section, lint, host-test, and Rustdoc checks used by
the project before submitting a change:

```bash
make check
```

## Managing userland ports

External userland sources are pinned in `userland/ports.toml`. Each port records
an immutable upstream commit, a generated checkout below `userland/ports`, and
a reviewable patch series below `userland/patches`. Generated checkouts are
intentionally excluded from this repository. The top-level Makefile delegates
the `ports-*` targets to `userland/Makefile`.

Prepare every checkout by shallow-fetching its pinned commit and applying its
patches. Only the pinned upstream commit is downloaded; upstream history is not
required for the local patch stack:

```bash
make ports-prepare
```

Pass `PORTS` to operate on one or more selected ports:

```bash
make ports-prepare PORTS=micropython
make ports-status PORTS=micropython
make ports-prepare PORTS=rust
```

To modify a port, edit its generated checkout and commit the changes there.
Then export every commit after the pinned revision as a patch series:

```bash
cd userland/ports/micropython
git add <files>
git commit -m "Describe the OS port change"
cd ../../..
make ports-sync PORTS=micropython
make ports-verify PORTS=micropython
```

`ports-sync` refuses dirty checkouts, replaces the stored patch series, and
therefore requires all intended changes to be committed first. `ports-verify`
applies the stored patches to the pinned revision in a temporary checkout and
checks that the resulting Git tree matches the working checkout. Commit both
`userland/ports.toml` and `userland/patches/` when adding or updating a port.

Available commands are:

```bash
make ports-prepare  # shallow-fetch the pinned revision and apply patches
make ports-sync     # export local commits to userland/patches/<port>
make ports-status   # show base, commit, patch, and dirty state
make ports-verify   # verify that the patches reproduce the checkout
make ports-clean    # remove only clean, fully synchronized checkouts
```

`ports-clean` refuses to remove a checkout when it contains uncommitted or
untracked files, or when the stored patch series does not reproduce its current
Git tree. Use `PORTS=micropython` to clean only the selected checkout.

Ports may declare a small set of pinned upstream submodules. `ports-prepare`
initializes only those listed paths rather than recursively downloading an
upstream project's unrelated toolchain and documentation repositories.

## Rust standard library userland

The `rust` port implements the repository's syscall ABI in Rust's platform
abstraction layer. It reuses the host compiler from the pinned rustup toolchain
and source-builds only the target libraries, avoiding a full compiler build.
`make build` installs an ordinary Rust `std` example as
`/bin/rust-std-demo`; it demonstrates collections, console output, read-only
file access, and sleeping. See `userland/ports/rust/os/README.md` after running
`make ports-prepare PORTS=rust` for the supported surface and limitations.

## Device drivers and board presets

`driver` discovers the console, timer, and interrupt controller independently
from firmware FDT. Its Cargo features only include device implementations.
`board/qemu-virt` (the default build target) enables NS16550, PLIC, and RV64 Sstc;
`board/litex-vexriscv` enables LiteUART, LiteX timer, and VexRiscv interrupts.
Each board owns its executable and driver features, and depends on the shared
`boot` library. `boot` has no board dependencies or board-selection features;
runtime depends on neither board nor driver crates.

Select a board with `cargo build -p board-qemu-virt --release` or
`make build BOARD=qemu-virt`. Plain `cargo build` selects QEMU virt via the
workspace default member. Use `make tangnano-image` for the RV32 target and
userland. Smoke tests remain in `boot`: use `make run FEATURES=smoke` or
`cargo build -p board-qemu-virt --release --features boot/smoke`.
See [driver discovery](driver/README.md) for matching rules and boot ordering.

## Tang Nano 9K (RV32IMA + Sv32)

The existing `linux-spi-lcd` gateware in the sibling `TangNano-9K-example`
repository contains a VexRiscv CPU with Sv32. Its BIOS must be built with the
firmware DTB handoff below; old gateware that passes a null a1 is unsupported.
The board target reuses this OS's buddy/slab allocators, guarded kernel stacks,
scheduler, VFS/TarFS, ELF loader, and syscall ABI. It adds RV32 integer trap and
context-switch code, two-level Sv32 page tables, a LiteX UART driver, and LiteX
external interrupt/timer support. C, F, and D instructions are disabled; A is
used for the existing kernel atomics. The CPU runs at 27MHz with 8MiB PSRAM.

Install the pinned toolchain with `rustup toolchain install` if needed. It needs
`rust-src` and `llvm-tools`. The build uses `-Zbuild-std` to rebuild core/alloc
without C/Zca, since the standard RV32IMAC libraries contain compressed code.

```sh
make tangnano-image
```

The normal build includes the existing MicroPython REPL as `/bin/micropython`
and boots it directly in U-mode. It uses the sibling FPGA repository's xPack
GCC by default; override `RV32_CROSS_COMPILE` to use another cross prefix.

Outputs are `artifacts/tangnano/kernel.{elf,bin,map}` and `kernel-flash.bin`
(LiteX FBI length/CRC header), plus an RV32 staging rootfs. Both architectures
embed the common `artifacts/initarfs` path; rebuild the archive for the target
architecture when switching builds. The image loads at
`0x40000000` and executes at virtual `0xff000000`. Like RV64, direct-map
addresses use `VA = PA + DIRECT_VMA_BASE`: RAM at `0x40000000` maps to
`0xd0000000`. Userspace is below `0x80000000`. Device installation maps LiteX
CSR resources into the MMIO virtual window starting at `0x80000000`; virtual
register addresses are independent of their physical addresses. Kernel stacks
share one Sv32 subtree at `0xfec00000`.

The OS does not generate, compile in, or upload a board DTB. The FPGA project
owns it: `board_dtb.py` generates the FDT from the finalized LiteX memory/CSR,
clock, CPU and interrupt metadata. `firmware.py` embeds it in the existing
BIOS ROM. On boot, BIOS copies it to the last RAM page (currently `0x407ff000`)
and passes a0=boot hart, a1=physical DTB pointer. The OS discovers RAM, UART,
timer, clock frequency and IRQ routing from that pointer, and reserves the DTB
memory before enabling the runtime allocator. DTB placement is a firmware
choice, not an address compiled into this OS.

Build and verify from the FPGA repository (close any serial terminal first):

```sh
cd ../TangNano-9K-example/linux-spi-lcd
./build-fpga.sh
# Test the new FPGA design and OS in RAM without changing Flash:
.venv/bin/python boot-rust.py --bitstream build/soc/gateware/impl/pnr/project.fs
# Install both matching images, then verify Flash autoboot:
.venv/bin/python boot-rust.py --persistent --fpga-flash build/soc/gateware/impl/pnr/project.fs
# Later kernel-only changes do not need to rewrite FPGA configuration:
.venv/bin/python boot-rust.py --persistent
.venv/bin/python boot-rust.py --autoboot
```

The persistent command replaces the external SPI Flash OS image. Supplying
`--fpga-flash` also replaces FPGA configuration Flash, including BIOS and its
DTB. The SD card is untouched. To restore the previously
built Linux image, run `program-kernel-uart.py --image build/linux-flash.bin`
from this directory, then reload FPGA configuration with Gowin Programmer's
operation 1. Keep the Linux image backup when switching OS images.

The default prompt is MicroPython's `>>>`, as on the RV64 OS. The RV32 port
keeps the existing core modules, read-only imports, `math`, and `smp.hart_id()`.
It uses software double-precision arithmetic (no FPU instructions) and 64-bit
integer objects for values that exceed RV32's small-integer range. Static files
such as `eratosthenes.py` and `rk4_harmonic_oscillator.py` are included in the
board rootfs. The port changes are preserved as patch 0009 in the pinned
MicroPython patch series, and `make ports-verify PORTS=micropython` checks them.

Connect a serial terminal at 115200 baud and press Enter:

```sh
screen /dev/cu.usbserial-1101 115200
```

The diagnostic Rust shell and its MMU isolation probe remain in
`userland/tangnano` for future use. The current image build only packages and
starts MicroPython; Rust-shell and MMU-fault boot selection are not exposed.
UART TX is mirrored to the existing LCD hardware. Rust-std userland and hardware
floating-point smoke tests remain RV64-only.

Hardware validation on 2026-11-09: the kernel reached the RV32 U-mode shell
with Sv32 active; UART input/output, file reads and timer sleep/wakeup passed.
The final 520192-byte image boots the original MicroPython REPL directly from
external Flash after FPGA configuration reload, with no UART kernel upload.
Arithmetic, software-double math, the existing prime/RK4 scripts and GC passed
on the board. The boot log is
`../TangNano-9K-example/linux-spi-lcd/build/firmware-dtb-persistent.log`.
A separate smoke image's U-mode read of `0xc0000000` generated a load-page
fault. The board kernel passed Clippy with warnings denied; all 38 existing
host tests passed, and the original RV64 boot target passed `cargo check`
using a static-files-only rootfs for compile validation. Full RV64 userland
ports and QEMU execution were not part of that check.

Firmware DTB handoff was verified in both temporary RAM boot and Flash autoboot:
BIOS copied its 1286-byte ROM DTB to `0x407ff000`; the OS reported exactly the
same a1 pointer and hart 0, with no DTB upload or DTB inside the kernel image.
The matching FPGA configuration and OS images are installed persistently.
MicroPython, software math, prime/RK4 imports and GC passed after Flash boot.
