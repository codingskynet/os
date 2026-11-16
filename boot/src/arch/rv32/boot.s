/* ============================================================================
 *  RISC-V 32-bit boot entry (assembly cheat sheet)
 *
 *  PLATFORM BOOT FLOW
 *  The current single-hart LiteX/VexRiscv platform enters this image from
 *  LiteX BIOS in M-mode at physical address 0x40000000:
 *    a0 = boot hart ID
 *    a1 = physical address of the Flattened Device Tree (FDT)
 *
 *  kernel.ld places .text.init first in the raw image. The kernel is linked
 *  at a high virtual address, but this entry runs at its physical load address.
 *  Early symbol accesses rely on PC-relative code and disabled relaxation;
 *  gp has not been initialized. Rust later installs Sv32 mappings and jumps
 *  to the high kernel address.
 *
 *  ASSEMBLY CHEAT SHEET (RISC-V / GNU as)
 *  ─────────────────────────────────────
 *  This list covers the syntax used in this file. Real instructions are marked
 *  "(instr)"; assembler conveniences are marked "(pseudo)" or "(directive)".
 *
 *  Assembler directives
 *    .option arch, rv32ima  (directive) select RV32 integer, multiply/divide,
 *                            and atomic instruction extensions for assembly.
 *    .option norvc          (directive) disable compressed instruction emission.
 *    .option push / pop     (directive) save / restore assembler option state.
 *    .option norelax        (directive) suppress linker-relaxation opportunities
 *                            for the enclosed instruction sequence.
 *    .section name, "flags" (directive) select a named input section.
 *                            "ax" means allocatable + executable.
 *    .global symbol         (directive) export a symbol to the linker.
 *
 *  Labels and label references
 *    label:                 define an address in code or data.
 *    1: / 1b / 2f           local numeric labels. 1b means nearest previous
 *                            "1:"; 2f means nearest next "2:".
 *
 *  General-purpose instructions and pseudo-instructions
 *    mv   rd, rs            (pseudo) copy rs to rd: addi rd, rs, 0.
 *    li   rd, imm           (pseudo) load an immediate constant into rd.
 *    la   rd, symbol        (pseudo) load the address of symbol into rd.
 *    addi rd, rs, imm       (instr)  rd = rs + sign-extended imm.
 *    sw   rs, offset(base)  (instr)  store 32-bit rs at base + offset.
 *
 *  Branches and jumps
 *    bgeu rs1, rs2, label   (instr)  if rs1 >= rs2 as unsigned, jump.
 *    j    label             (pseudo) unconditional jump: jal x0, label.
 *
 *  CSR (Control and Status Register) access
 *    csrw csr, rs           (pseudo) write rs into csr, discarding the old value.
 *                            Expands to csrrw x0, csr, rs.
 *    csrs csr, rs           (pseudo) set csr bits selected by rs.
 *                            Expands to csrrs x0, csr, rs.
 *    csrc csr, rs           (pseudo) clear csr bits selected by rs.
 *                            Expands to csrrc x0, csr, rs.
 *    csrci csr, imm         (pseudo) clear csr bits selected by a 5-bit mask.
 *                            Expands to csrrci x0, csr, imm.
 *
 *  CSR names and fields used here
 *    mie                    Machine interrupt-enable register; zero disables
 *                            the implemented interrupt-enable bits.
 *    mstatus                Machine status. MIE (bit 3) is the M-mode global
 *                            interrupt enable; MPP (bits 12:11) selects the
 *                            privilege mode entered by mret.
 *    medeleg / mideleg      Exception / interrupt delegation to S-mode for
 *                            supported causes. These are WARL registers.
 *    mepc                   Machine exception PC; mret resumes execution here.
 *    satp                   Supervisor address translation and protection;
 *                            zero selects Bare mode (no address translation).
 *    0xbc0 / 0x9c0          VexRiscv-specific M-mode / S-mode external IRQ masks.
 *                            These are custom CSRs, not standard RISC-V devices.
 *
 *  Privileged instructions
 *    mret                   (instr)  enter the mode selected by mstatus.MPP
 *                            and resume at mepc. It does not set a return address.
 *
 *  REGISTER NAMING QUICK REFERENCE (RISC-V calling convention)
 *  ──────────────────────────────────────────────────────────
 *    zero (x0)      — always 0 (writes ignored)
 *    sp   (x2)      — stack pointer
 *    gp   (x3)      — global pointer; not initialized in this entry
 *    tp   (x4)      — thread pointer; runtime later uses it for per-core state
 *    t0 / t1 (x5/6) — scratch registers for BSS bounds and CSR values
 *    a0 / a1 (x10/11) — hart ID and FDT pointer passed to Rust
 *    s0 / s1 (x8/9) — saved copies of the firmware handoff arguments
 * ============================================================================ */

.option arch, rv32ima
.option norvc

/* ---------------------------------------------------------------------------
 *  BOOT ENTRY — first instruction of the raw kernel image.
 *
 *  This path assumes a single boot hart and an M-mode firmware handoff.
 *  Secondary-hart startup is not implemented here.
 * ------------------------------------------------------------------------- */
.section .text.init, "ax"
.global _start
_start:
    # Preserve the firmware arguments while preparing the execution environment.
    mv s0, a0                       # s0 = boot hart ID
    mv s1, a1                       # s1 = FDT physical address

    # Disable interrupt sources and M-mode global interrupt delivery.
    csrw mie, zero
    csrci mstatus, 8                # clear MIE (bit 3)
    csrw 0xbc0, zero                # VexRiscv machine external IRQ mask
    csrw 0x9c0, zero                # VexRiscv supervisor external IRQ mask
    csrw satp, zero                 # enter S-mode with paging disabled

    # kernel.ld reserves a 16-byte-aligned boot stack inside the init BSS range.
    # Keep this address load PC-relative; gp is not available for relaxation.
    .option push
    .option norelax
    la sp, boot_stack_top
    .option pop
    li tp, 0                        # no runtime per-core state installed yet

    /* -----------------------------------------------------------------------
     *  Zero-initialized memory
     *
     *  The linker bounds are 4-byte aligned. Each loop clears a half-open
     *  [start, end) range in RV32 words. The init range includes the boot stack;
     *  clearing it is safe because no calls or stack stores have occurred yet.
     * --------------------------------------------------------------------- */
    la t0, _init_bss_start
    la t1, _init_bss_end
1:  bgeu t0, t1, 2f                 # if t0 >= t1, init BSS is complete
    sw zero, 0(t0)                  # store 4 zero bytes
    addi t0, t0, 4                  # advance one RV32 word
    j 1b
2:  la t0, _bss_start               # live kernel BSS, retained after boot
    la t1, _bss_end
3:  bgeu t0, t1, 4f                 # if t0 >= t1, live BSS is complete
    sw zero, 0(t0)
    addi t0, t0, 4
    j 3b
4:
    /* -----------------------------------------------------------------------
     *  Temporary M-mode -> S-mode transition
     *
     *  Sv32 translation will be enabled by Rust in S-mode. No page table is
     *  installed here, so mepc still receives a physical entry address.
     *
     *  TODO: Add a separate S-mode firmware/SBI entry path when supporting
     *  OpenSBI; these M-mode CSR accesses cannot run from S-mode.
     * --------------------------------------------------------------------- */

    # Request delegation of every exception/interrupt cause the CPU supports.
    # Unsupported delegation bits remain unset according to the CSR's WARL rules.
    li t0, -1
    csrw medeleg, t0
    csrw mideleg, t0

    # mstatus.MPP (bits 12:11): 00 = U-mode, 01 = S-mode, 11 = M-mode.
    # Clear both bits, then select S-mode as the destination of mret.
    li t0, 0x1800                   # MPP mask = 3 << 11
    csrc mstatus, t0
    li t0, 0x800                    # MPP.S = 1 << 11
    csrs mstatus, t0

    # Restore the handoff arguments and enter _start_rust(hart_id, dtb_ptr).
    # Rust enables Sv32 and transfers execution to the linked high address.
    la t0, _start_rust
    csrw mepc, t0
    mv a0, s0
    mv a1, s1
    mret
