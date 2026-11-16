OUTPUT_ARCH(riscv)
ENTRY(_start)
SECTIONS {
  . = 0x10000;
  .text : { KEEP(*(.text.entry)) *(.text .text.*) }
  . = ALIGN(4096);
  .rodata : { *(.rodata .rodata.* .srodata .srodata.*) }
  . = ALIGN(4096);
  .data : { *(.data .data.* .sdata .sdata.*) }
  . = ALIGN(4096);
  .bss : { *(.bss .bss.* .sbss .sbss.*) }
  /DISCARD/ : { *(.eh_frame*) }
}
