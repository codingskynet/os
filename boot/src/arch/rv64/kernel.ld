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
 * build.rs injects KERNEL_LMA_BASE, KERNEL_VMA_BASE, KERNEL_VMA_OFFSET,
 * PAGE_SIZE, and HUGE_PAGE_SIZE from src/arch/rv64/consts.rs. Keep Rust as
 * the single source for memory-layout constants.
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
 *  PHDRS (Program Headers)
 *  Defines ELF program headers (= segments, the parts the loader maps).
 *  Each output section below maps to one of these via ":text" / ":data" etc.
 *
 *  PT_LOAD = a segment that gets loaded into memory.
 *  On bare metal these are mostly informational; QEMU's -kernel loads the
 *  flat kernel image. They still matter for:
 *    - Debuggers (GDB)
 *    - objcopy --strip-all (creates the .img from the ELF)
 *    - Linker relaxation / GP-relative addressing
 *    - Keeping ELF segment flags aligned with runtime page-table permissions
 *
 *  Runtime permissions:
 *    text   -> readable + executable
 *    rodata -> readable only
 *    data   -> readable + writable
 *    bss    -> readable + writable
 * ------------------------------------------------------------------------- */
PHDRS
{
  text PT_LOAD;
  rodata PT_LOAD;
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
  ASSERT(DEFINED(PAGE_SIZE),
         "PAGE_SIZE must be provided by build.rs")
  ASSERT(DEFINED(HUGE_PAGE_SIZE),
         "HUGE_PAGE_SIZE must be provided by build.rs")
  ASSERT(PAGE_SIZE > 0 && ((PAGE_SIZE & (PAGE_SIZE - 1)) == 0),
         "PAGE_SIZE must be a non-zero power of two")
  ASSERT(HUGE_PAGE_SIZE > 0 && ((HUGE_PAGE_SIZE & (HUGE_PAGE_SIZE - 1)) == 0),
         "HUGE_PAGE_SIZE must be a non-zero power of two")
  ASSERT((HUGE_PAGE_SIZE % PAGE_SIZE) == 0,
         "HUGE_PAGE_SIZE must be a multiple of PAGE_SIZE")

  . = KERNEL_VMA_BASE;
  _kernel_start = .;

  ASSERT((KERNEL_LMA_BASE % HUGE_PAGE_SIZE) == 0,
         "KERNEL_LMA_BASE must be HUGE_PAGE_SIZE-aligned for early Sv39 leaf mapping")
  ASSERT((KERNEL_VMA_BASE % HUGE_PAGE_SIZE) == 0,
         "KERNEL_VMA_BASE must be HUGE_PAGE_SIZE-aligned for early Sv39 leaf mapping")
  ASSERT((KERNEL_VMA_OFFSET % HUGE_PAGE_SIZE) == 0,
         "KERNEL_VMA_OFFSET must preserve HUGE_PAGE_SIZE alignment")

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
    _text_start = .;                    /* "." = current location counter */
    KEEP(*(.text.init))            /* Collect ALL .text.init input sections */
    . = ALIGN(4);
  } :text

  ASSERT(_text_start == KERNEL_VMA_BASE,
         "_start must be the first byte in the linked kernel image")
  ASSERT(LOADADDR(.text.init) == KERNEL_LMA_BASE,
         ".text.init must load at the QEMU virt reset address")
  ASSERT((_text_start % PAGE_SIZE) == 0,
         "_text_start must be PAGE_SIZE-aligned")

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
   *  _text_end is rounded up to PAGE_SIZE after the input section. This creates
   *  the runtime page-table boundary between executable text and NX rodata.
   *  Useful for:
   *    - Calculating the RX mapping range
   *    - Setting up MMU permissions (RX vs RO/RW)
   *    - Self-checks / integrity hashing
   * ======================================================================== */
  . = ALIGN(4);
  .text : AT(ADDR(.text) - KERNEL_VMA_OFFSET)
  {
    *(.text .text.*)
    . = ALIGN(4);
  } :text
  . = ALIGN(PAGE_SIZE);
  _text_end = .;
  ASSERT((_text_end % PAGE_SIZE) == 0,
         "_text_end must be PAGE_SIZE-aligned")

  /* ========================================================================
   *  .rodata — Read-only data
   *
   *  Stores string literals, const data, jump tables, vtables, and other data
   *  that must be readable but not writable or executable after the final page
   *  table is installed.
   *
   *  The section has its own PT_LOAD segment and PAGE_SIZE-aligned boundaries so
   *  runtime mappings and ELF segment flags both model rodata as read-only/NX.
   *
   *  _rodata_start / _rodata_end: boundaries for permission setting.
   * ======================================================================== */
  . = ALIGN(PAGE_SIZE);
  .rodata : AT(ADDR(.rodata) - KERNEL_VMA_OFFSET)
  {
    _rodata_start = .;
    *(.rodata .rodata.*)
    . = ALIGN(4);
  } :rodata
  . = ALIGN(PAGE_SIZE);
  _rodata_end = .;
  ASSERT((_rodata_start % PAGE_SIZE) == 0,
         "_rodata_start must be PAGE_SIZE-aligned")
  ASSERT((_rodata_end % PAGE_SIZE) == 0,
         "_rodata_end must be PAGE_SIZE-aligned")

  /* ========================================================================
   *  .data — Writable data (initialized globals / statics)
   *
   *  _data_start follows the PAGE_SIZE-aligned end of rodata. That boundary is
   *  required by the final page table so RO and RW pages never share one PTE.
   *
   *  *(.sdata .sdata.*)
   *  RISC-V has a "small data" section for GP-relative addressing —
   *  short globals accessed via a dedicated global-pointer register (gp).
   *  Must be collected alongside regular .data so the linker can compute
   *  GP-relative offsets correctly.
   *
   *  _data_start / _data_end: boundaries for cache maintenance, hashing.
   * ======================================================================== */
  .data : AT(ADDR(.data) - KERNEL_VMA_OFFSET)
  {
    _data_start = .;
    *(.sdata .sdata.*)
    *(.data .data.*)
    . = ALIGN(4);
    _data_end = .;
  } :data
  ASSERT((_data_start % PAGE_SIZE) == 0,
         "_data_start must be PAGE_SIZE-aligned")

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
  } :bss
  . = ALIGN(PAGE_SIZE);
  _bss_end = .;
  ASSERT((_bss_end % PAGE_SIZE) == 0,
         "_bss_end must be PAGE_SIZE-aligned")

  _kernel_end = .;
  ASSERT((_kernel_end % PAGE_SIZE) == 0,
         "_kernel_end must be PAGE_SIZE-aligned")

  /* ========================================================================
   *  Non-runtime metadata
   *
   *  These sections are intentionally placed at VMA 0 and are not assigned to
   *  a PT_LOAD segment. They remain in the ELF for debuggers, tooling, and
   *  target metadata, but they are not part of the loaded kernel image and are
   *  ignored when objcopy creates the flat binary.
   *
   *  Listing them here also keeps --orphan-handling=error useful: any new
   *  allocatable runtime section must be handled explicitly instead of being
   *  silently placed by the linker.
   * ======================================================================== */

  /* DWARF debug sections kept for kernel.elf/kernel.debug symbolication. */
  .debug_abbrev 0 : { *(.debug_abbrev) *(.debug_abbrev.*) }
  .debug_addr 0 : { *(.debug_addr) *(.debug_addr.*) }
  .debug_aranges 0 : { *(.debug_aranges) *(.debug_aranges.*) }
  .debug_frame 0 : { *(.debug_frame) *(.debug_frame.*) }
  .debug_info 0 : { *(.debug_info .debug_info.*) }
  .debug_line 0 : { *(.debug_line) *(.debug_line.*) }
  .debug_line_str 0 : { *(.debug_line_str) *(.debug_line_str.*) }
  .debug_loc 0 : { *(.debug_loc) *(.debug_loc.*) }
  .debug_loclists 0 : { *(.debug_loclists) *(.debug_loclists.*) }
  .debug_macro 0 : { *(.debug_macro) *(.debug_macro.*) }
  .debug_names 0 : { *(.debug_names) *(.debug_names.*) }
  .debug_pubnames 0 : { *(.debug_pubnames) *(.debug_pubnames.*) }
  .debug_pubtypes 0 : { *(.debug_pubtypes) *(.debug_pubtypes.*) }
  .debug_ranges 0 : { *(.debug_ranges) *(.debug_ranges.*) }
  .debug_rnglists 0 : { *(.debug_rnglists) *(.debug_rnglists.*) }
  .debug_str 0 : { *(.debug_str) *(.debug_str.*) }
  .debug_str_offsets 0 : { *(.debug_str_offsets) *(.debug_str_offsets.*) }

  /* RISC-V ABI/ISA attributes consumed by ELF-aware tools. */
  .riscv.attributes 0 : { *(.riscv.attributes) }

  /* ELF symbol and string tables used by debuggers and inspection tools. */
  .symtab 0 : { *(.symtab) }
  .strtab 0 : { *(.strtab) }
  .shstrtab 0 : { *(.shstrtab) }

  /* Compiler/linker provenance; useful for inspection, never loaded. */
  .comment 0 : { *(.comment) }

  /* This kernel has no stack unwinder yet, so discard unwind metadata. */
  /DISCARD/ : {
    *(.eh_frame)
    *(.eh_frame.*)
    *(.eh_frame_hdr)
  }
}
