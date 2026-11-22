#![allow(clippy::large_stack_frames)]
#![allow(non_upper_case_globals)]
#![allow(unused)]

mod driver_gen;

mod util {
    pub mod consts {
        include!("../runtime/src/util/consts.rs");
    }
}

mod arch {
    pub mod rv32 {
        pub mod consts {
            include!("../runtime/src/arch/rv/rv32/consts.rs");
        }
    }
    pub mod rv64 {
        pub mod consts {
            include!("../runtime/src/arch/rv/rv64/consts.rs");
        }
    }
}

fn main() {
    let package = env!("CARGO_PKG_NAME");
    if package == "driver" {
        driver_gen::generate_for_cargo("src", driver_gen::Output::Library);
        return;
    }

    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").expect("missing Cargo target arch");
    link(&target_arch);

    if package != "board-qemu-virt" {
        driver_gen::generate_for_cargo("../../driver/src", driver_gen::Output::Selection);
    }
}

fn link(target_arch: &str) {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    println!("cargo:rerun-if-changed={manifest_dir}/../build.rs");
    let (arch, symbols) = match target_arch {
        "riscv32" => {
            use self::arch::rv32::consts::*;
            (
                "rv32",
                [
                    ("KERNEL_VMA_BASE", KERNEL_VMA_BASE),
                    ("KERNEL_VMA_SIZE", KERNEL_VMA_SIZE),
                    ("DIRECT_VMA_SIZE", DIRECT_VMA_SIZE),
                    ("PAGE_SIZE", PAGE_SIZE.get()),
                    ("HUGE_PAGE_SIZE", HUGE_PAGE_SIZE.get()),
                    ("STACK_SIZE", STACK_SIZE.get()),
                ],
            )
        }
        "riscv64" => {
            use self::arch::rv64::consts::*;
            (
                "rv64",
                [
                    ("KERNEL_VMA_BASE", KERNEL_VMA_BASE),
                    ("KERNEL_VMA_SIZE", KERNEL_VMA_SIZE),
                    ("DIRECT_VMA_SIZE", DIRECT_VMA_SIZE),
                    ("PAGE_SIZE", PAGE_SIZE.get()),
                    ("HUGE_PAGE_SIZE", HUGE_PAGE_SIZE.get()),
                    ("STACK_SIZE", STACK_SIZE.get()),
                ],
            )
        }
        _ => panic!("unsupported boot target architecture: {target_arch}"),
    };

    println!("cargo:rerun-if-changed={manifest_dir}/../../runtime/src/util/consts.rs");
    println!("cargo:rerun-if-changed={manifest_dir}/../../runtime/src/arch/rv/{arch}/consts.rs");
    let boot_arch_dir = format!("{manifest_dir}/../../boot/src/arch/rv/{arch}");
    println!("cargo:rerun-if-changed={boot_arch_dir}");

    // Linker arguments belong to the board executable, not the boot library.
    println!("cargo:rerun-if-changed={manifest_dir}/board.ld");
    println!("cargo:rustc-link-arg=--script={manifest_dir}/board.ld");
    println!("cargo:rustc-link-arg=--script={boot_arch_dir}/kernel.ld");
    println!("cargo:rustc-link-arg=--no-relax");
    println!("cargo:rustc-link-arg=--orphan-handling=error");
    for (name, value) in symbols {
        println!("cargo:rustc-link-arg=--defsym={name}={value:#x}");
    }
}
