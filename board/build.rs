#![allow(clippy::large_stack_frames)]
#![allow(non_upper_case_globals)]
#![allow(unused)]

mod util {
    pub mod consts {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../runtime/src/util/consts.rs"
        ));
    }
}

mod arch {
    pub mod rv32 {
        pub mod consts {
            include!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../runtime/src/arch/rv32/consts.rs"
            ));
        }
    }
    pub mod rv64 {
        pub mod consts {
            include!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../runtime/src/arch/rv64/consts.rs"
            ));
        }
    }
}

fn main() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    // Build scripts run on the host; select constants using Cargo's target,
    // not cfg(target_arch), which would describe the host architecture here.
    let target_arch =
        std::env::var("CARGO_CFG_TARGET_ARCH").expect("Cargo must provide CARGO_CFG_TARGET_ARCH");
    let (arch, symbols) = match target_arch.as_str() {
        "riscv32" => {
            use crate::arch::rv32::consts::*;
            (
                "rv32",
                [
                    ("KERNEL_LMA_BASE", KERNEL_LMA_BASE),
                    ("KERNEL_VMA_BASE", KERNEL_VMA_BASE),
                    ("KERNEL_VMA_OFFSET", KERNEL_VMA_OFFSET),
                    ("PAGE_SIZE", PAGE_SIZE.get()),
                    ("HUGE_PAGE_SIZE", HUGE_PAGE_SIZE.get()),
                    ("STACK_SIZE", STACK_SIZE.get()),
                ],
            )
        }
        "riscv64" => {
            use crate::arch::rv64::consts::*;
            (
                "rv64",
                [
                    ("KERNEL_LMA_BASE", KERNEL_LMA_BASE),
                    ("KERNEL_VMA_BASE", KERNEL_VMA_BASE),
                    ("KERNEL_VMA_OFFSET", KERNEL_VMA_OFFSET),
                    ("PAGE_SIZE", PAGE_SIZE.get()),
                    ("HUGE_PAGE_SIZE", HUGE_PAGE_SIZE.get()),
                    ("STACK_SIZE", STACK_SIZE.get()),
                ],
            )
        }
        _ => panic!("unsupported boot target architecture: {target_arch}"),
    };

    println!("cargo:rerun-if-changed={manifest_dir}/../../runtime/src/util/consts.rs");
    println!("cargo:rerun-if-changed={manifest_dir}/../../runtime/src/arch/{arch}/consts.rs");
    println!("cargo:rerun-if-changed={manifest_dir}/../../boot/src/arch/{arch}");

    // Linker arguments belong to the board executable, not the boot library.
    println!("cargo:rustc-link-arg=--script={manifest_dir}/../../boot/src/arch/{arch}/kernel.ld");
    println!("cargo:rustc-link-arg=--no-relax");
    println!("cargo:rustc-link-arg=--orphan-handling=error");
    for (name, value) in symbols {
        println!("cargo:rustc-link-arg=--defsym={name}={value:#x}");
    }
}
