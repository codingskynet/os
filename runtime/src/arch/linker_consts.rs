//! Linker-defined constants shared by the RV32 and RV64 kernel images.

unsafe extern "C" {
    // Section boundaries are addresses in the linked image.
    pub static _kernel_start: u8;
    pub static _init_start: u8;
    pub static _init_end: u8;
    pub static _text_start: u8;
    pub static _text_end: u8;
    pub static _rodata_start: u8;
    pub static _rodata_end: u8;
    pub static _data_start: u8;
    pub static _data_end: u8;
    pub static _bss_start: u8;
    pub static _bss_end: u8;
    pub static _kernel_end: u8;

    pub static _kernel_vma_offset: usize;
}

/// Distance from the board's physical kernel load address to its linked VA.
#[inline]
pub fn kernel_vma_offset() -> usize {
    // Both linker scripts store one aligned, pointer-sized word in .rodata.
    unsafe { _kernel_vma_offset }
}
