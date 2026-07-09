/* ============================================================================
 *  RISC-V 64-bit boot entry (assembly cheat sheet)
 *
 *  ASSEMBLY CHEAT SHEET (RISC-V / GNU as)
 *  ─────────────────────────────────────
 *
 *  This is intentionally limited to the syntax used in this file plus the
 *  nearby RV64 inline assembly. Real instructions are marked "(instr)";
 *  assembler conveniences are marked "(pseudo)" or "(directive)".
 *
 *  Assembler directives
 *    .equ NAME, value        (directive) define a compile-time constant, like
 *                            #define in C. Example: .equ STACK_SIZE, 8192
 *    .equiv NAME, value      (directive) define a constant, but error if NAME
 *                            was already defined. Useful for override guards.
 *    .section name, "flags"  (directive) place following code/data in a named
 *                            section. "ax" means allocatable + executable.
 *    .global symbol          (directive) export symbol to the linker so other
 *                            files can reference it.
 *    .skip N                 (directive) reserve N zero-initialized bytes.
 *
 *  Labels and label references
 *    label:                  define an address in code or data.
 *    1: / 1b / 2f            local numeric labels. 1b means nearest previous
 *                            "1:"; 2f means nearest next "2:".
 *
 *  General-purpose instructions and pseudo-instructions
 *    li   rd, imm            (pseudo) load an immediate constant into rd.
 *    la   rd, symbol         (pseudo) load the address of symbol into rd.
 *    addi rd, rs, imm        (instr)  rd = rs + sign-extended imm.
 *    sd   rs, offset(base)   (instr)  store 64-bit rs at base + offset.
 *
 *  Branches and jumps
 *    bnez rs, label          (pseudo) if rs != 0, jump to label.
 *    bgeu rs1, rs2, label    (instr)  if rs1 >= rs2 as unsigned, jump.
 *    j    label              (pseudo) unconditional jump: jal x0, label.
 *
 *  CSR (Control and Status Register) access
 *    csrrw rd, csr, rs       (instr)  read old csr into rd, then write rs
 *                            into csr. Use rd = x0 to discard the old value.
 *    csrrs rd, csr, rs       (instr)  read old csr into rd, then set csr bits
 *                            that are 1 in rs. Use rs = x0 for read-only.
 *    csrrc rd, csr, rs       (instr)  read old csr into rd, then clear csr
 *                            bits that are 1 in rs. rs = x0 changes nothing.
 *    csrr rd, csr            (pseudo) read csr into rd.
 *                            Expands to csrrs rd, csr, x0.
 *    csrw csr, rs            (pseudo) write rs into csr.
 *                            Expands to csrrw x0, csr, rs.
 *    csrs csr, rs            (pseudo) set csr bits that are 1 in rs, discard
 *                            old value. Expands to csrrs x0, csr, rs.
 *    csrc csr, rs            (pseudo) clear csr bits that are 1 in rs, discard
 *                            old value. Expands to csrrc x0, csr, rs.
 *
 *  CSR names used around this code
 *    CSR                     Special CPU control/status register, separate
 *                            from general registers x0-x31. Access depends on
 *                            privilege mode; illegal access traps.
 *    mhartid                 Machine hart id. Used to pick the boot hart and
 *                            per-hart stack.
 *    mstatus                 Machine status bits. The MPP field selects the
 *                            privilege mode entered by mret.
 *    mepc                    Machine exception PC. mret resumes execution here.
 *    medeleg / mideleg       Machine exception/interrupt delegation registers.
 *                            Set bits delegate supported traps to S-mode.
 *    pmpaddr0 / pmpcfg0      Physical Memory Protection entry 0 address/config.
 *                            Used here to let S-mode access physical memory.
 *    sstatus                 Supervisor status bits. SIE controls whether
 *                            supervisor interrupts are enabled.
 *    satp                    Supervisor address translation and protection.
 *                            Holds paging mode and root page-table PPN.
 *    time                    Timer counter CSR, read by the scheduler/time code.
 *
 *  Privileged / memory-management instructions
 *    mret                    (instr)  return from M-mode trap/entry. The next
 *                            privilege mode comes from mstatus.MPP, and the
 *                            target PC comes from mepc.
 *    sfence.vma rs1, rs2     (instr)  flush address-translation cache entries.
 *                            Common full flush form: sfence.vma zero, zero.
 *    wfi                     (instr)  wait for interrupt; used here to park
 *                            secondary harts.
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

# pmpcfg0 entry bits for PMP entry 0.
#   bit 0      R = read permission
#   bit 1      W = write permission
#   bit 2      X = execute permission
#   bits 4:3   A = address-matching mode; 01 means TOR (top of range)
#   bit 7      L = lock bit; left clear so the entry is not locked
.equ PMP_R,                 (1 << 0)
.equ PMP_W,                 (1 << 1)
.equ PMP_X,                 (1 << 2)
.equ PMP_A_TOR,             (1 << 3)
.equ PMP_CFG_TOR_RWX,       (PMP_A_TOR | PMP_R | PMP_W | PMP_X)

# mcounteren controls which counters are visible below M-mode.
#   bit 0 CY = cycle
#   bit 1 TM = time
#   bit 2 IR = instret
.equ MCOUNTEREN_TM,         (1 << 1)

# menvcfg controls optional supervisor-mode facilities.
#   bit 63 STCE = S-mode may use stimecmp for supervisor timer interrupts.
.equ MENVCFG_STCE,          (1 << 63)

# mstatus.MPP is bits 12:11. mret enters the privilege mode stored there:
#   00 = U-mode, 01 = S-mode, 11 = M-mode.
.equ MSTATUS_MPP_MASK,      (3 << 11)
.equ MSTATUS_MPP_S,         (1 << 11)

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

    # setup the boot stack used by hart 0 until Rust installs the runtime stack
    la   sp, boot_stack_top

    # zero the init bss range
    la   t0, _init_bss_start
    la   t1, _init_bss_end
1:  bgeu t0, t1, 2f                 # if t0 >= t1, done
    sd   zero, 0(t0)                # store 8 zero bytes at t0
    addi t0, t0, 8                  # advance by 8 bytes
    j    1b                         # loop
2:
    # zero the regular bss section
    la   t0, _bss_start
    la   t1, _bss_end
3:  bgeu t0, t1, 4f                 # if t0 >= t1, done
    sd   zero, 0(t0)                # store 8 zero bytes at t0
    addi t0, t0, 8                  # advance by 8 bytes
    j    3b                         # loop
4:
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
    # In TOR mode, pmpaddr0 is the top of the range encoded as address >> 2.
    # All ones therefore makes entry 0 cover the largest representable range.
    li   t0, -1
    csrw pmpaddr0, t0
    # pmpcfg0 = A:TOR | R | W | X, so S-mode can read/write/execute in it.
    li   t0, PMP_CFG_TOR_RWX
    csrw pmpcfg0, t0

    # Delegate traps and interrupts that S-mode can handle.  These CSRs are
    # WARL, so unsupported delegation bits are ignored by the implementation.
    # Writing all ones requests delegation of every delegatable cause.
    li   t0, -1
    csrw medeleg, t0
    csrw mideleg, t0

    # Allow S-mode to read the time CSR. Without mcounteren.TM, `rdtime`
    # from supervisor mode traps as an illegal instruction on compliant harts.
    li   t0, MCOUNTEREN_TM
    csrs mcounteren, t0

    # Allow S-mode to program stimecmp when Sstc is available.
    li   t0, MENVCFG_STCE
    csrs 0x30a, t0

    # mret returns to the privilege encoded in mstatus.MPP.  Clear MPP, then
    # set it to 01 (supervisor mode).
    li   t0, MSTATUS_MPP_MASK
    csrc mstatus, t0
    li   t0, MSTATUS_MPP_S
    csrs mstatus, t0

    # mepc is the PC that mret jumps to after switching into S-mode.
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
