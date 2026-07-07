include!("src/arch/rv64/consts.rs");

fn main() {
    println!("cargo:rerun-if-changed=src/arch/rv64/consts.rs");
    println!("cargo:rerun-if-changed=src/arch/rv64/kernel.ld");

    // Use rustc-link-arg-bin to restrict these linker symbols to the "kernel"
    // binary only. This prevents them from leaking into lib.rs builds (e.g.
    // `cargo test` for arch-independent unit tests).
    println!("cargo:rustc-link-arg-bin=kernel=--defsym=KERNEL_LMA_BASE={KERNEL_LMA_BASE:#x}");
    println!("cargo:rustc-link-arg-bin=kernel=--defsym=KERNEL_VMA_BASE={KERNEL_VMA_BASE:#x}");
    println!("cargo:rustc-link-arg-bin=kernel=--defsym=KERNEL_VMA_OFFSET={KERNEL_VMA_OFFSET:#x}");
    println!("cargo:rustc-link-arg-bin=kernel=--defsym=PAGE_SIZE={PAGE_SIZE:#x}");
    println!("cargo:rustc-link-arg-bin=kernel=--defsym=HUGE_PAGE_SIZE={HUGE_PAGE_SIZE:#x}");
}
