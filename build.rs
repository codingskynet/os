include!("src/arch/rv64/consts.rs");

fn main() {
    println!("cargo:rerun-if-changed=src/arch/rv64/consts.rs");

    println!("cargo:rustc-link-arg=--defsym=KERNEL_LMA_BASE={KERNEL_LMA_BASE:#x}");
    println!("cargo:rustc-link-arg=--defsym=KERNEL_VMA_BASE={KERNEL_VMA_BASE:#x}");
    println!("cargo:rustc-link-arg=--defsym=KERNEL_VMA_OFFSET={KERNEL_VMA_OFFSET:#x}");
}
