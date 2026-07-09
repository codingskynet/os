//! RISC-V 64-bit architecture support.

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
pub mod page_fault;
pub mod page_table;
pub mod paging;
pub mod regs;
pub mod switch;
pub mod timer;
pub mod trap;
