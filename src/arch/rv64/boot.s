/* ============================================================================
 *  RISC-V 64-bit boot entry (assembly cheat sheet)
 *
 *  ASSEMBLY CHEAT SHEET (RISC-V)
 *  ─────────────────────────────
 *
 *  ┌─────────────────────┬──────────────────────────────────────────────────┐
 *  │ Instruction /       │ What it does                                     │
 *  │ Directive           │                                                  │
 *  ├─────────────────────┼──────────────────────────────────────────────────┤
 *  │ .equ NAME, value    │ Compile-time constant — like #define in C or a   │
 *  │                     │ const in Rust. Example: .equ STACK_SIZE, 8192    │
 *  ├─────────────────────┼──────────────────────────────────────────────────┤
 *  │ .global symbol      │ Make 'symbol' visible to the linker (like        │
 *  │                     │ pub in Rust). Other files can reference it.      │
 *  ├─────────────────────┼──────────────────────────────────────────────────┤
 *  │ label:              │ Defines a position (address) in code. Labels     │
 *  │                     │ are targets for j/bnez/etc. Can also declare     │
 *  │                     │ data locations (e.g. stacks: .skip ...).         │
 *  ├─────────────────────┼──────────────────────────────────────────────────┤
 *  │ 1: / 2f / 1b        │ Local numeric labels. 1: defines "label 1",     │
 *  │                     │ 1b = "the most recent label 1 backward",        │
 *  │                     │ 2f = "label 2 forward". No name collision —     │
 *  │                     │ you can reuse 1: many times in the same file.   │
 *  ├─────────────────────┼──────────────────────────────────────────────────┤
 *  │ csrr rd, csr        │ CSR (Control Status Register) Read. Reads a     │
 *  │                     │ special CPU register (like mhartid) into        │
 *  │                     │ general-purpose register rd.                     │
 *  ├─────────────────────┼──────────────────────────────────────────────────┤
 *  │ slli rd, rs, shamt  │ Shift Left Logical Immediate. rd = rs << shamt. │
 *  │                     │ RISC-V immediate shift: shamt is a 5/6-bit      │
 *  │                     │ constant encoded in the instruction itself.      │
 *  ├─────────────────────┼──────────────────────────────────────────────────┤
 *  │ la   rd, symbol     │ Load Address. Puts the link-time address of     │
 *  │                     │ 'symbol' into rd. Pseudo-instruction — the      │
 *  │                     │ assembler expands it to lui + addi.             │
 *  ├─────────────────────┼──────────────────────────────────────────────────┤
 *  │ add  rd, rs1, rs2   │ rd = rs1 + rs2.                                 │
 *  ├─────────────────────┼──────────────────────────────────────────────────┤
 *  │ addi rd, rs, imm    │ Add Immediate. rd = rs + sign-extended imm.     │
 *  ├─────────────────────┼──────────────────────────────────────────────────┤
 *  │ bnez rs, label      │ Branch if Not Equal to Zero. If rs != 0,        │
 *  │                     │ jump to label.                                  │
 *  ├─────────────────────┼──────────────────────────────────────────────────┤
 *  │ bgeu rs1, rs2, lbl  │ Branch if Greater or Equal, Unsigned.           │
 *  │                     │ If rs1 >= rs2 (unsigned), jump to label.        │
 *  ├─────────────────────┼──────────────────────────────────────────────────┤
 *  │ j    label          │ Unconditional jump (jal x0, label).             │
 *  ├─────────────────────┼──────────────────────────────────────────────────┤
 *  │ sd   rs, offset(rt) │ Store Doubleword (64-bit). Writes rs into       │
 *  │                     │ memory at address rt + offset.                  │
 *  ├─────────────────────┼──────────────────────────────────────────────────┤
 *  │ wfi                 │ Wait For Interrupt. Puts the hart to sleep      │
 *  │                     │ until an interrupt arrives. Used to park        │
 *  │                     │ unused CPU cores.                               │
 *  ├─────────────────────┼──────────────────────────────────────────────────┤
 *  │ .skip N             │ Reserve N bytes of zero-initialized space.      │
 *  │                     │ Like .zero N or .space N in other assemblers.  │
 *  └─────────────────────┴──────────────────────────────────────────────────┘
 *
 *  REGISTER NAMING QUICK REFERENCE (RISC-V calling convention)
 *  ──────────────────────────────────────────────────────────
 *    zero  (x0)   — always 0 (writes ignored)
 *    ra    (x1)   — return address
 *    sp    (x2)   — stack pointer
 *    gp    (x3)   — global pointer
 *    tp    (x4)   — thread pointer
 *    t0–t6 (x5-7, x28-31) — temporaries (caller-saved)
 *    a0–a7 (x10-17)       — function arguments / return values
 *    s0–s11 (x8-9, x18-27) — saved registers (callee-saved)
 *
 * ============================================================================ */

.equ STACK_SIZE,     8192
.section .text.init, "ax"

/* ---------------------------------------------------------------------------
 *  BOOT ENTRY — the Rust entry-point symbol.
 *
 *  Override by defining `BOOT_ENTRY` before including this file:
 *      .equiv BOOT_ENTRY, my_other_entry
 * ------------------------------------------------------------------------- */
.equ BOOT_ENTRY, _start_rust

.global _start

_start:
    # setup stacks per hart
    csrr t0, mhartid                # read current hart id
    slli t0, t0, 13                 # shift left the hart id by STACK_SIZE (8192 = 2^13)
    la   sp, stacks + STACK_SIZE    # set the initial stack pointer
                                    # to the end of the stack space
    add  sp, sp, t0                 # move the current hart stack pointer
                                    # to its place in the stack space

    # pass hart_id and DTB pointer to Rust according to the QEMU boot convention.
    #
    # QEMU's -bios none -kernel mode jumps to _start with:
    #   a0 = mhartid  (boot hart id)
    #   a1 = DTB physical address (Flattened Device Tree blob)
    #
    # Re-read a0 from the CSR (idempotent), leave a1 untouched so the
    # device-tree pointer survives until _start_rust.
    csrr a0, mhartid                # a0 = hart id (from CSR, same as what QEMU set)

    # park harts with id != 0
    bnez a0, park                   # if we're not on the hart 0
                                    # we park the hart

    # zero the bss section
    la   t0, _bss_start
    la   t1, _bss_end
1:  bgeu t0, t1, 2f                 # if t0 >= t1, done
    sd   zero, 0(t0)                # store 8 zero bytes at t0
    addi t0, t0, 8                  # advance by 8 bytes
    j    1b                         # loop
2:

    j    enter_supervisor_mode      # hart 0 enters S-mode before Rust

/* ---------------------------------------------------------------------------
 *  Temporary M-mode -> S-mode transition
 *
 *  QEMU's -bios none -kernel path starts the kernel in machine mode.  Sv39
 *  paging is controlled by satp in supervisor mode, so drop to S-mode before
 *  entering Rust.
 *
 *  TODO: When booting through firmware such as OpenSBI, the kernel may already
 *  enter here in S-mode.  Split this into a firmware/SBI entry path before
 *  supporting that boot mode; executing M-mode CSR instructions from S-mode
 *  would trap.
 * ------------------------------------------------------------------------- */
enter_supervisor_mode:
    # Give S-mode access to all physical memory for this early bootstrap path.
    # pmpaddr0 = all ones, pmpcfg0 = TOR | R | W | X.
    li   t0, -1
    csrw pmpaddr0, t0
    li   t0, 0x0f
    csrw pmpcfg0, t0

    # Delegate traps and interrupts that S-mode can handle.  These CSRs are
    # WARL, so unsupported delegation bits are ignored by the implementation.
    li   t0, -1
    csrw medeleg, t0
    csrw mideleg, t0

    # mret returns to the privilege encoded in mstatus.MPP.  Clear MPP, then
    # set it to 01 (supervisor mode).
    li   t0, (3 << 11)
    csrc mstatus, t0
    li   t0, (1 << 11)
    csrs mstatus, t0

    la   t0, supervisor_entry
    csrw mepc, t0
    mret

supervisor_entry:
    j    BOOT_ENTRY                 # jump to Rust in S-mode

/* ---------------------------------------------------------------------------
 *  Secondary hart parking
 *
 *  Harts with id != 0 are parked here in WFI.  Later, when SMP bring-up is
 *  implemented, the primary hart can send an IPI or store a spin-table entry
 *  that releases each parked hart.  The spin-table protocol (used by Linux)
 *  polls a memory location; WFI is a gentler default.
 * ------------------------------------------------------------------------- */
.secondary_entry:
    /* a0 already holds mhartid from the common path above.
     * SMP bring-up will redirect harts here instead of park. */
    j    BOOT_ENTRY

park:
    wfi
    j park

stacks:
    .skip STACK_SIZE * 8            # allocate space for the harts stacks
