mod page_table;

use core::arch::asm;
use core::mem::MaybeUninit;
use core::num::NonZeroUsize;
use core::ptr;

use page_table::{PageTable, PteFlags, SATP_MODE_SV39, ppn, vpn0, vpn1, vpn2};

use super::consts::*;
use crate::console::{CONSOLE, Console};
use crate::dev::uart::ns16550::NS16550;
use crate::mm::addr::{Pa, Va};
use crate::util::consts::{G, M};

pub unsafe fn enable_mmu_and_jump(
    entry: usize,
    hart_id: usize,
    dtb_pa: usize,
    memory_start: Pa,
    memory_end: Pa,
) -> ! {
    const L2_PAGE_SIZE: NonZeroUsize = NonZeroUsize::new(1 * G).unwrap();
    const L1_PAGE_SIZE: NonZeroUsize = NonZeroUsize::new(2 * M).unwrap();

    // Temporary Sv39 root page table using 1GiB leaf mapping for whole memory
    static mut TEMP_ROOT: PageTable = PageTable::new();
    // Temporary Sv39 L1 page table usign 2MiB leaf mapping for kernel
    static mut TEMP_KERNEL_L1: PageTable = PageTable::new();

    unsafe {
        // Build a temporary bootstrap address space:
        // - identity RAM map for the instructions immediately after satp
        // - linear direct map for early physical access after the jump
        // - kernel image map at its linked high virtual address
        let root = (&raw mut TEMP_ROOT) as *mut PageTable;
        let kernel_l1 = (&raw mut TEMP_KERNEL_L1) as *mut PageTable;

        let flag =
            PteFlags::V | PteFlags::R | PteFlags::W | PteFlags::X | PteFlags::A | PteFlags::D;

        let early_mmio = Pa::new(0);
        (*root)
            .entry(vpn2(early_mmio.to_va()))
            .mut_address(early_mmio)
            .mut_flags(flag);

        // create direct map for physical RAM section(QEMU: 0x8000_0000 ~)
        {
            let mut pa = memory_start.align_down(L2_PAGE_SIZE);
            while pa < memory_end {
                // identical mapping
                (*root)
                    .entry(vpn2(Va::new(pa.as_raw())))
                    .mut_address(pa)
                    .mut_flags(flag);
                // direct mapping
                (*root)
                    .entry(vpn2(pa.to_va()))
                    .mut_address(pa)
                    .mut_flags(flag);
                pa = pa.checked_offset(L2_PAGE_SIZE.get()).unwrap();
            }
        }

        // create kernel map starting from KERNEL_VMA_BASE
        {
            (*root)
                .entry(vpn2(Va::new(KERNEL_VMA_BASE)))
                .mut_address(Pa::new(kernel_l1 as usize))
                .mut_flags(PteFlags::V);

            let mut va = Va::new(&raw const _kernel_start as usize + KERNEL_VMA_OFFSET);
            let end = Va::new((&raw const _kernel_end) as usize + KERNEL_VMA_OFFSET);
            while va < end {
                (*kernel_l1)
                    .entry(vpn1(va))
                    .mut_address(va.to_pa())
                    .mut_flags(flag);
                va = va.checked_offset(L1_PAGE_SIZE.get()).unwrap();
            }
        }

        // TODO: MMIO must be mapped to temp page table?
        ptr::write(
            CONSOLE.as_mut(),
            Console::Ns16550(NS16550::new(Pa::new(0x1000_0000).to_va().as_raw())),
        );

        let satp = SATP_MODE_SV39 | ppn(Pa::new(root as usize));
        asm!("sfence.vma zero, zero", options(nostack, preserves_flags));
        asm!("csrw satp, {}", in(reg) satp, options(nostack, preserves_flags));
        asm!("sfence.vma zero, zero", options(nostack, preserves_flags));

        let entry = entry.checked_add(KERNEL_VMA_OFFSET).expect("Invalid entry");

        // Enter the high-kernel world without returning through the low-address
        // call stack. The temporary identity map exists only to execute the
        // instructions between `csrw satp` and this jump.
        asm!(
            "add sp, sp, t1",
            "jr  t0",
            in("t0") entry,
            in("t1") KERNEL_VMA_OFFSET,
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
    fn map(
        l2: &mut PageTable,
        va: Va,
        mut alloc: impl FnMut() -> &'static mut MaybeUninit<PageTable>,
        flags: PteFlags,
    ) {
        let l1 = l2.entry(vpn2(va)).or_insert_with(|| alloc().as_mut_ptr());
        let l0 = l1.entry(vpn1(va)).or_insert_with(|| alloc().as_mut_ptr());
        l0.entry(vpn0(va)).mut_address(va.to_pa()).mut_flags(flags);
    }

    let root = alloc().write(PageTable::default());
    let flags = PteFlags::V | PteFlags::R | PteFlags::W | PteFlags::X | PteFlags::A | PteFlags::D;

    // create direct map for physical RAM section(QEMU: 0x8000_0000 ~)
    let mut pa = start;
    while pa < end {
        map(root, pa.to_va(), &mut alloc, flags);
        pa = pa.checked_offset(PAGE_SIZE.get()).unwrap();
    }

    // create kernel map starting from KERNEL_VMA_BASE
    let mut va = Va::new(&raw const _kernel_start as usize);
    assert_eq!(
        va, KERNEL_VMA_BASE,
        "kernel binary does not start from {KERNEL_VMA_BASE:#x}"
    );
    let kernel_end = Va::new((&raw const _kernel_end) as usize);
    while va < kernel_end {
        map(root, va, &mut alloc, flags);
        va = va.checked_offset(PAGE_SIZE.get()).unwrap();
    }

    // TODO: generalize from reading FDT with MMIO_MAP_ADDR
    let uart = Pa::new(0x1000_0000).to_va();
    map(root, uart, &mut alloc, flags);
    unsafe {
        ptr::write(
            CONSOLE.as_mut(),
            Console::Ns16550(NS16550::new(uart.as_raw())),
        );
    }

    unsafe {
        let satp = SATP_MODE_SV39 | ppn(Va::from(root).to_pa());
        asm!("sfence.vma zero, zero", options(nostack, preserves_flags));
        asm!("csrw satp, {}", in(reg) satp, options(nostack, preserves_flags));
        asm!("sfence.vma zero, zero", options(nostack, preserves_flags));
    }
}
