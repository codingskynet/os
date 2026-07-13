//! RISC-V 64-bit architecture support.
//!
//! This module contains the supervisor-mode pieces required by the runtime:
//! CSR wrappers, Sv39 paging, trap dispatch, timer interrupts, and kernel
//! thread context switching.

macro_rules! riscv_privileged_isa_version {
    () => {
        "v20260120"
    };
}

macro_rules! riscv_supervisor_doc_url {
    ($fragment:literal) => {
        concat!(
            "https://docs.riscv.org/reference/isa/",
            riscv_privileged_isa_version!(),
            "/priv/supervisor.html",
            $fragment,
        )
    };
}

pub mod asm;
pub mod consts;
pub mod page_table;
pub mod paging;
pub mod regs;
pub mod switch;
pub mod timer;
pub mod trap;
