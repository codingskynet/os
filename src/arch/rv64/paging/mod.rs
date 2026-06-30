mod page_table;

use core::arch::asm;
use core::mem::MaybeUninit;
use core::ptr;

use page_table::{PageTable, PteFlags, SATP_MODE_SV39, ppn, vpn0, vpn1, vpn2};

use super::consts::*;
use crate::console::{CONSOLE, Console};
use crate::dev::uart::ns16550::NS16550;
use crate::mm::addr::{Pa, Va};
use crate::util::consts::G;

const DIRECT_MAP_SIZE: usize = 128 * G;

// Temporary Sv39 root page table using 1GiB leaf mappings.
static mut TEMP: PageTable = PageTable::new();

#[cfg(not(target_arch = "riscv64"))]
pub unsafe fn enable_mmu_and_jump(_entry: usize, _hart_id: usize, _dtb_pa: usize) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(target_arch = "riscv64")]
pub unsafe fn enable_mmu_and_jump(entry: usize, hart_id: usize, dtb_pa: usize) -> ! {
    unsafe {
        // Build a temporary bootstrap address space with two 1GiB-leaf windows:
        //
        // Identity mapping for the MMU-enable trampoline:
        // VA 0x0000_0000_0000_0000 .. 0x0000_003f_ffff_ffff
        // PA 0x0000_0000_0000_0000 .. 0x0000_003f_ffff_ffff

        // Higher-half direct mapping:
        // VA 0xffff_ffc0_0000_0000 .. 0xffff_ffff_ffff_ffff
        // PA 0x0000_0000_0000_0000 .. 0x0000_003f_ffff_ffff
        //
        // The identity half is needed for the instructions immediately after
        // writing satp. The CPU keeps fetching at the current low virtual PC
        // until we explicitly jump to the higher-half alias below.
        // Use a raw pointer so this early boot code does not create Rust
        // references to a `static mut`.
        let temp = &raw mut TEMP;

        let mut address = Pa::new(0);
        let flag =
            PteFlags::V | PteFlags::R | PteFlags::W | PteFlags::X | PteFlags::A | PteFlags::D;
        while address < Pa::new(DIRECT_MAP_SIZE) {
            (*temp)
                .entry(vpn2(Va::new(address.as_raw()))) // identical mapping
                .mut_address(address)
                .mut_flags(flag);
            (*temp)
                .entry(vpn2(address.to_va()))
                .mut_address(address)
                .mut_flags(flag);
            address = address.checked_offset(G).unwrap();
        }

        let satp = SATP_MODE_SV39 | ppn(Pa::new(temp as usize));
        asm!("sfence.vma zero, zero", options(nostack, preserves_flags));
        asm!("csrw satp, {}", in(reg) satp, options(nostack, preserves_flags));
        asm!("sfence.vma zero, zero", options(nostack, preserves_flags));

        let entry = entry.checked_add(DIRECT_MAP_ADDR).expect("Invalid entry");

        // Enter the direct-map world without returning through the low-address
        // call stack. The temporary identity map exists only to execute the
        // instructions between `csrw satp` and this jump.
        asm!(
            "add sp, sp, t1",
            "jr  t0",
            in("t0") entry,
            in("t1") DIRECT_MAP_ADDR,
            in("a0") hart_id,
            in("a1") dtb_pa,
            clobber_abi("C"),
            options(noreturn),
        );
    }
}

pub unsafe fn init_page_table(
    start: Pa,
    end: Pa,
    mut alloc: impl FnMut() -> &'static mut MaybeUninit<PageTable>,
) {
    unsafe {
        let l2 = alloc().write(PageTable::default());

        let flags =
            PteFlags::V | PteFlags::R | PteFlags::W | PteFlags::X | PteFlags::A | PteFlags::D;

        let mut addr = start.to_va();
        while addr < end.to_va() {
            let pa = addr.to_pa();
            let l1 = l2.entry(vpn2(addr)).or_insert_with(|| alloc().as_mut_ptr());
            let l0 = l1.entry(vpn1(addr)).or_insert_with(|| alloc().as_mut_ptr());
            l0.entry(vpn0(addr))
                .mut_address(pa)
                .mut_flags(flags);

            let identity = Va::new(pa.as_raw());
            let l1 = l2.entry(vpn2(identity)).or_insert_with(|| alloc().as_mut_ptr());
            let l0 = l1.entry(vpn1(identity)).or_insert_with(|| alloc().as_mut_ptr());
            l0.entry(vpn0(identity)).mut_address(pa).mut_flags(flags);

            addr = addr.checked_offset(PAGE_SIZE.get()).unwrap();
        }

        // TODO: generalize from reading FDT with MMIO_MAP_ADDR
        let pa = Pa::new(0x1000_0000);
        let va = pa.to_va();
        let l1 = l2.entry(vpn2(va)).or_insert_with(|| alloc().as_mut_ptr());
        let l0 = l1.entry(vpn1(va)).or_insert_with(|| alloc().as_mut_ptr());
        l0.entry(vpn0(va)).mut_address(pa).mut_flags(flags);

        ptr::write(
            CONSOLE.as_mut(),
            Console::Ns16550(NS16550::new(va.as_raw())),
        );

        let satp = SATP_MODE_SV39 | ppn(Va::from(l2).to_pa());
        asm!("sfence.vma zero, zero", options(nostack, preserves_flags));
        asm!("csrw satp, {}", in(reg) satp, options(nostack, preserves_flags));
        asm!("sfence.vma zero, zero", options(nostack, preserves_flags));
    }
}
