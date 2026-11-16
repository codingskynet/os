# OS

The toy project for implementing OS

## How to Setup & Build & Run

```bash
make setup          # install dependencies and prepare pinned userland ports
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
workspace default member. The FPGA repository composes RV32 firmware images; see below. Smoke tests remain in `boot`: use `make run FEATURES=smoke` or
`cargo build -p board-qemu-virt --release --features boot/smoke`.
See [driver discovery](driver/README.md) for matching rules and boot ordering.

## LiteX / VexRiscv (RV32IMA + Sv32)

`board-litex-vexriscv` selects LiteUART, LiteX timer and VexRiscv interrupts.
The OS owns RV32 entry/trap/context code, Sv32 page tables, allocators,
scheduler, VFS/TarFS, ELF loading, syscall ABI and freestanding userland ports.
Firmware passes a0=boot hart and a1=physical FDT pointer. The OS discovers
RAM, console, timer, clocks and IRQ routing from FDT and reserves the DTB.
It does not generate a DTB or encode board pinouts, RAM capacity or clocks.

The current RV32 load address is `0x40000000`, kernel virtual base
`0xff000000`, and userspace is below `0x80000000`. The physical direct map
uses `VA = PA + 0x90000000`; MMIO starts at virtual `0x80000000` and guarded
kernel stacks at `0xfec00000`. These are OS/loader ABI constraints, not board
capacity discovery. The linker checks the 16MiB kernel virtual-address window;
the external image builder checks physical RAM capacity and firmware reservations.

The sibling FPGA repository owns image composition and deployment:
[`linux-spi-lcd`](../TangNano-9K-example/linux-spi-lcd/README.md).
Run `make -C ../TangNano-9K-example/linux-spi-lcd rust-image BOARD=primer20k`
(or `BOARD=nano9k`) there. Cross-toolchain lookup, board description, staging
rootfs, LiteX FBI length/CRC, Flash offsets, programming and UART probes belong
there. This OS repository must not depend on that repository to build the
kernel or implement drivers.

For a prepared RV32 `artifacts/initarfs`, the kernel-only invocation is:

```sh
RUSTFLAGS='-C target-feature=-c,-zca -C relocation-model=static -C code-model=medium' \
  cargo build -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem \
  --release --target riscv32imac-unknown-none-elf -p board-litex-vexriscv
```

Use the pinned nightly with `rust-src` and `llvm-tools`; core/alloc must be
rebuilt without C/Zca for RV32IMA. Userland `ARCH` and `CROSS_COMPILE` are
architecture/toolchain inputs and remain in the MicroPython port. The image
currently starts `/bin/micropython` in U-mode, with software double precision,
read-only module imports, `math`, and `smp.hart_id()`. Rust-std userland remains
RV64-only. The unused diagnostic `userland/tangnano` is legacy OS test code;
renaming/removing it is independent of image deployment.

Both architectures currently embed `artifacts/initarfs`; rebuild it when
switching architecture and do not build different images concurrently in one
checkout. The root Makefile's full userland build remains RV64-oriented;
selecting `BOARD=litex-vexriscv` alone does not compose an RV32 rootfs.
