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

.equ STACK_SIZE, 8192

.global _start

_start:
    # setup stacks per hart
    csrr t0, mhartid                # read current hart id
    slli t0, t0, 13                 # shift left the hart id by STACK_SIZE (8192 = 2^13)
    la   sp, stacks + STACK_SIZE    # set the initial stack pointer
                                    # to the end of the stack space
    add  sp, sp, t0                 # move the current hart stack pointer
                                    # to its place in the stack space

    # park harts with id != 0
    csrr a0, mhartid                # read current hart id
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

    j    _start_rust                # hart 0 jump to rust

park:
    wfi
    j park

stacks:
    .skip STACK_SIZE * 8            # allocate space for the harts stacks
