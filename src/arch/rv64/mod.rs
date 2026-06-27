//! RISC-V 64-bit architecture support.
//!
//! *Boot* — low-level assembly entry (`_start`), BSS zeroing, stack setup.
//! *Trap* —   (TODO) trap vector, exception / interrupt handling.
//! *Paging* — (TODO) page table management (satp).
//! *Timer* —  (TODO) RISC-V timer (mtime/mtimecmp).
//! *Context* — (TODO) context-switch assembly (`__switch`).

pub mod boot;
