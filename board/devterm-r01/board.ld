/* OpenSBI occupies the first 2 MiB of DRAM. */
KERNEL_LMA_BASE = 0x40200000;
_reset_target = _sbi_entry;
