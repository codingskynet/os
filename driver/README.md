# Device discovery and drivers

Cargo features include implementations; they never select a board or remove
another backend. `board/*` crates bundle features for a build. `runtime`
provides console, timer, and interrupt-dispatch interfaces without depending
on these implementations.

Implementations and their selection/installation code are grouped by device role:

```text
src/
├── console/      # LiteUART, NS16550
├── interrupt/    # PLIC, VexRiscv interrupt array, IRQ routes
├── timer/        # LiteX timer, RISC-V Sstc
├── discovery.rs  # Shared FDT queries
└── lib.rs        # Individual device installation entry points
```

The additive `litex` feature bundles `liteuart` and `litex-timer`. Individual
features remain available. CPU-specific interrupt controllers are selected
separately: `board/litex-vexriscv` enables `litex` plus `vexriscv-intc`, so the
LiteX peripheral bundle does not assume a particular CPU or interrupt layout.

| Feature | Discovery |
| --- | --- |
| `ns16550` | `ns16550` / `ns16550a` on the chosen console |
| `liteuart` | `litex,liteuart` on the chosen console |
| `litex-timer` | enabled `litex,timer` node with uptime registers |
| `riscv-sstc` | Sstc advertised by every enabled CPU (RV64 implementation) |
| `plic` | `sifive,plic-1.0.0` / `riscv,plic0` on the devices' interrupt parent |
| `vexriscv-intc` | `vexriscv,intc0` on the devices' interrupt parent |

Boot builds the initial and final page tables from RAM and the kernel image,
without probing drivers or mapping device registers. Early log messages are
buffered without timestamps until the clock is installed.

After initializing the page allocator and activating the final kernel root,
boot calls `Device::install_timer`, `Device::install_uart`, and
`Device::install_intc` separately. Each call discovers its device from FDT before
installation; no aggregate device state is retained. Discovery follows
`/chosen/stdout-path`, checks enabled nodes and their ancestors, and follows
interrupt-parent phandles without accessing hardware. Each device registers its
own IRQ and handler during installation. `install_intc` then closes registration
and installs the controllers referenced by those independent registrations; it
does not probe the UART or timer again. Device IRQ-enable callbacks run during
per-hart interrupt initialization, after the backends have been installed.

Each controller implements runtime's `InterruptController` trait. After all
controllers have been constructed, boot retains their allocations for the
kernel's lifetime and publishes `&'static dyn InterruptController` references.
Runtime exposes the controller set through `interrupt::system()`, whose methods
initialize controllers per hart and dispatch pending IRQs through the trait.
The kernel's external interrupt entry acts on the returned scheduling request
after every controller finishes its IRQs. Timer consumers use `timer::device()`
to call methods on the installed timer directly.
Controller selection enums are used only during discovery and construction.

Each device's installation requests its MMIO mapping through
`runtime::mm::mmio::Mapper`, then constructs the driver with the returned virtual
base. The LiteX timer is mapped before clock initialization; the console and
PLIC are mapped before use. Sstc and VexRiscv's CSR interrupt array need no MMIO.
Drivers do not derive register pointers with `Pa::into_va()`.

The mapper allocates permanent supervisor RW, non-executable pages in the MMIO
virtual window, preserving resource offsets within a page. Device memory uses
the platform PMAs. It is a boot-time mapper: all mappings must finish before
`PerCore::init` clones roots for idle threads and before secondary harts start.
Runtime initialization then starts the timer and enables interrupts. Installed
objects and callbacks live outside reclaimable boot sections. Hotplug and
unmapping are not implemented.

The current kernel consumes one console and one timer. Multiple matching timer
backends are reported as ambiguous instead of depending on feature or probe
order. IRQs are identified by interrupt-parent phandle and source number, so
devices may use different supported controllers. Shared IRQ lines and cascaded
controllers are not implemented. Standard CPU
Sstc interrupts do not occupy an external controller route. Firmware must
permit supervisor access to Sstc CSRs; advertising the ISA extension alone
does not grant that access.

Features can be enabled directly, for example:

```sh
cargo build -p board-qemu-virt --release \
  --features driver/liteuart
```

Use the board build scripts for the corresponding target, linker flags and
userland archive. Enabling additional drivers does not make an image portable
across incompatible ISA widths, entry protocols, or kernel load addresses.
