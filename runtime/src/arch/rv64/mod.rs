//! RISC-V 64-bit architecture support.
//!
//! *Entry* — boot-owned assembly entry and early MMU transition helpers.
//! *Trap* —   (TODO) trap vector, exception / interrupt handling.
//! *Paging* — (TODO) page table management (satp).
//! *Timer* —  (TODO) RISC-V timer (mtime/mtimecmp).
//! *Context* — (TODO) context-switch assembly (`__switch`).

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
pub mod regs;
pub mod switch;
pub mod timer;
pub mod trap;
