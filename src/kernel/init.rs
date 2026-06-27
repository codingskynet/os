use crate::dev::dt::Fdt;
use crate::{console, println};

pub struct BootInfo {
    /// Hardware identifier of the CPU that entered the common kernel path.
    pub boot_cpu_id: usize,
    /// Platform description handed over by firmware or the bootloader.
    pub boot_data: BootData,
}

pub enum BootData {
    /// Flattened Device Tree pointer, commonly used by RISC-V and ARM systems.
    DeviceTree(Fdt),
    /// No platform description was provided, or it is not known yet.
    None,
}

/// Kernel entry point
///
/// `boot_info` describes the boot CPU and any platform data supplied by the
/// architecture-specific entry code.
///
/// # Safety
/// It must be called with a valid stack pointer and BSS already zeroed.
pub unsafe fn kernel_init(boot_info: BootInfo) -> ! {
    if let BootData::DeviceTree(dt) = &boot_info.boot_data {
        let model = unsafe { dt.query().prop("model") }
            .and_then(|v| {
                let end = v.iter().position(|&b| b == 0).unwrap_or(v.len());
                core::str::from_utf8(&v[..end]).ok()
            })
            .unwrap_or("(unknown)");
        println!("dtb: FDT detected, model = \"{}\"", model);
        unsafe {
            if let Err(e) = console::install_from_fdt(dt) {
                println!("dtb: Failed to install console: {:?}", e);
            }
        }
    }
    println!("hello, world");
    panic!();
}
