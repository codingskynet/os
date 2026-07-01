/* ============================================================================
 *  RISC-V 64-bit kernel linker script
 *
 *  TARGET:  riscv64gc-unknown-none-elf
 *  FORMAT:  ELF64 RISC-V, little-endian
 *  LOAD:    QEMU virt machine loads the raw binary at 0x80000000
 *
 *  RISC-V BACKGROUND
 *  On the QEMU virt machine with -bios none -kernel:
 *    - All harts (CPU cores) start in MACHINE mode at address 0x80000000
 *    - The first instruction at 0x80000000 MUST be the entry point (_start)
 *    - hart 0 runs; other harts are parked in boot.s
 *    - No bootloader — the kernel IS the bootloader
 *  ============================================================================
 */

OUTPUT_ARCH( "riscv" )

/*
 * This kernel is linked at a high virtual address but starts executing from
 * its physical load address before paging is enabled. Like Linux/RISC-V, the
 * early image therefore relies on medany-style PC-relative code generation:
 *
 *   Rust/LLVM: -C code-model=medium -C relocation-model=static
 *   Linker:    --no-relax
 *
 * Do not use absolute addresses or GP-relative relaxation in early boot code
 * unless gp and the matching virtual mapping are already established.
 *
 * KERNEL_LMA_BASE, KERNEL_VMA_BASE, and KERNEL_VMA_OFFSET are injected by
 * build.rs from src/arch/rv64/consts.rs. Keep Rust as the single source for
 * these addresses.
 */

/* ---------------------------------------------------------------------------
 *  ENTRY
 *  Declares the runtime entry point symbol (does NOT set the binary offset).
 *  The linker records this in the ELF header's e_entry field.
 *  QEMU ignores ELF headers (it loads the raw binary) — so the real
 *  guarantee comes from PLACING this symbol FIRST via .text.init below.
 * ------------------------------------------------------------------------- */
ENTRY( _start )

/* ---------------------------------------------------------------------------
 *  MEMORY
 *  RISCV: QEMU virt machine's main RAM starts at 0x80000000.
 *  128 MiB is more than enough for a toy kernel; the virt machine typically
 *  has 128-1024 MiB depending on configuration.
 * ------------------------------------------------------------------------- */
/* ---------------------------------------------------------------------------
 *  PHDRS (Program Headers)
 *  Defines ELF program headers (= segments, the parts the loader maps).
 *  Each output section below maps to one of these via ":text" / ":data" etc.
 *
 *  PT_LOAD = a segment that gets loaded into memory.
 *  On bare metal these are mostly informational; QEMU's -kernel loads the
 *  entire flat binary anyway. But they matter for:
 *    - Debuggers (GDB)
 *    - objcopy --strip-all (creates the .img from the ELF)
 *    - Linker relaxation / GP-relative addressing
 * ------------------------------------------------------------------------- */
PHDRS
{
  text PT_LOAD;
  data PT_LOAD;
  bss  PT_LOAD;
}

SECTIONS
{
  ASSERT(DEFINED(KERNEL_LMA_BASE),
         "KERNEL_LMA_BASE must be provided by build.rs")
  ASSERT(DEFINED(KERNEL_VMA_BASE),
         "KERNEL_VMA_BASE must be provided by build.rs")
  ASSERT(DEFINED(KERNEL_VMA_OFFSET),
         "KERNEL_VMA_OFFSET must be provided by build.rs")

  . = KERNEL_VMA_BASE;
  _kernel_start = .;

  ASSERT((KERNEL_LMA_BASE % 0x200000) == 0,
         "KERNEL_LMA_BASE must be 2MiB-aligned for early Sv39 superpage mapping")
  ASSERT((KERNEL_VMA_BASE % 0x200000) == 0,
         "KERNEL_VMA_BASE must be 2MiB-aligned for early Sv39 superpage mapping")
  ASSERT((KERNEL_VMA_OFFSET % 0x200000) == 0,
         "KERNEL_VMA_OFFSET must preserve 2MiB alignment")

  /* ========================================================================
   *  .text.init — Entry point section
   *
   *  PURPOSE: Guarantee _start is at binary offset 0 (= 0x80000000).
   *
   *  KEEP()
   *  Prevents --gc-sections (dead-stripping) from removing this section.
   *  Without KEEP(), the linker might think .text.init has no references
   *  since _start is called implicitly by hardware reset, not by any
   *  call instruction in the code.
   *
   *  ALIGN(4)
   *  RISC-V instructions are exactly 4 bytes (one word) and must be
   *  4-byte aligned. ALIGN(4) ensures the section starts at a multiple
   *  of 4 — both for alignment and for consistent disassembly.
   *
   *  WHY DOES THIS WORK ON QEMU?
   *  QEMU's -kernel loads the binary into RAM at 0x80000000 and jumps
   *  to that exact address. If the very first 4 bytes are NOT _start,
   *  execution begins in the middle of Rust code or garbage — instant fault.
   * ======================================================================== */
  . = ALIGN(4);
  .text.init KERNEL_VMA_BASE : AT(KERNEL_LMA_BASE)
  {
    _stext = .;                    /* "." = current location counter */
    KEEP(*(.text.init))            /* Collect ALL .text.init input sections */
    . = ALIGN(4);
  } :text

  ASSERT(_stext == KERNEL_VMA_BASE,
         "_start must be the first byte in the linked kernel image")
  ASSERT(LOADADDR(.text.init) == KERNEL_LMA_BASE,
         ".text.init must load at the QEMU virt reset address")

  /* ========================================================================
   *  .text — Main kernel code
   *
   *  *(.text .text.*) collects:
   *    - .text   : Rust/assembly functions (default code section)
   *    - .text.* : compiler-generated function-specific sections
   *                (e.g. .text._ZN4core… — mangled function names)
   *
   *  Without ".*", inline functions or #[inline(always)] items placed in
   *  separate subsections by the compiler would be MISSED.
   *
   *  _etext marks the end of code. Useful for:
   *    - Calculating code size
   *    - Setting up MMU permissions (RX vs RW)
   *    - Self-checks / integrity hashing
   * ======================================================================== */
  . = ALIGN(4);
  .text : AT(ADDR(.text) - KERNEL_VMA_OFFSET)
  {
    *(.text .text.*)
    . = ALIGN(4);
    _etext = .;
  } :text

  /* ========================================================================
   *  .rodata — Read-only data
   *
   *  Stores: string literals, const data, jump tables, vtables.
   *  In a kernel this can share the "text" PT_LOAD segment (RX permissions)
   *  since it never needs to be written.
   *
   *  _rodata_start / _rodata_end: boundaries for permission setting.
   * ======================================================================== */
  . = ALIGN(4);
  .rodata : AT(ADDR(.rodata) - KERNEL_VMA_OFFSET)
  {
    _rodata_start = .;
    *(.rodata .rodata.*)
    . = ALIGN(4);
    _rodata_end = .;
  } :text

  /* ========================================================================
   *  .data — Writable data (initialized globals / statics)
   *
   *  ALIGN(4096) creates a page-aligned boundary between RO and RW data.
   *  This is OPTIONAL for now but prepares for virtual memory — the MMU
   *  needs page-aligned regions to set different permissions (RX vs RW).
   *
   *  *(.sdata .sdata.*)
   *  RISC-V has a "small data" section for GP-relative addressing —
   *  short globals accessed via a dedicated global-pointer register (gp).
   *  Must be collected alongside regular .data so the linker can compute
   *  GP-relative offsets correctly.
   *
   *  _data_start / _data_end: boundaries for cache maintenance, hashing.
   * ======================================================================== */
  . = ALIGN(4096);
  .data : AT(ADDR(.data) - KERNEL_VMA_OFFSET)
  {
    _data_start = .;
    *(.sdata .sdata.*)
    *(.data .data.*)
    . = ALIGN(4);
    _data_end = .;
  } :data

  /* ========================================================================
   *  .bss — Zero-initialized data
   *
   *  BSS = "Block Started by Symbol" (historical IBM 7094 term).
   *  Stores global/static variables that start at zero.
   *  BSS takes NO space in the ELF file (only the .bss ELF section header
   *  records its size). The boot code zeros it at runtime in boot.s
   *  (la t0, _bss_start; la t1, _bss_end; loop: sd zero, 0(t0); ...).
   *
   *  RISC-V NOTE:
   *  *(.sbss .sbss.*) collects small-BSS for GP-relative access, same
   *  concept as .sdata for small globals.
   * ======================================================================== */
  . = ALIGN(4);
  .bss : AT(ADDR(.bss) - KERNEL_VMA_OFFSET)
  {
    _bss_start = .;
    *(.sbss .sbss.*)
    *(.bss .bss.*)
    . = ALIGN(4);
    _bss_end = .;
  } :bss

  _kernel_end = .;
}
