    .section .text
    .global _start
_start:
    # C906 User Manual 15.1.7. FEL has uploaded both images to DRAM.
    # Enable extended PTE attributes, supervisor CLINT and the C906 ISA.
    .equ MAEE, 1 << 21
    .equ CLINTEE, 1 << 17
    .equ THEADISAEE, 1 << 22
    csrci mstatus, 8
    csrw mie, zero
    li t0, MAEE | CLINTEE | THEADISAEE
    csrs 0x7c0, t0

    # Invalidate both L1 caches before using the newly uploaded instructions.
    fence iorw, iorw
    csrwi 0x7c2, 0x13
1:  csrr t0, 0x7c2
    andi t0, t0, 0x10
    bnez t0, 1b
    csrsi 0x7c1, 7              # instruction/data caches and write allocation
    csrr a0, mhartid
    li a1, 0x48000000
    lla a2, dynamic_info
    li t0, 0x40000000
    fence.i
    jr t0
    .balign 8
dynamic_info:
    # OpenSBI dynamic_info v2: magic, version, next PC, S-mode, options, boot hart.
    .dword 0x4942534f, 2, kernel_load, 1, 0, 0
