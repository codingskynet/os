//! RISC-V boot-time page-table setup.
//!
//! Boot first installs a temporary address space so execution can jump from the
//! low physical entry point to the high kernel virtual address. It later builds
//! the final runtime page table from firmware memory information.

use runtime::asm;
use core::mem::MaybeUninit;
use core::num::NonZeroUsize;
use core::ops::DerefMut;
use core::ptr;

use runtime::arch::consts::*;
use runtime::arch::page_table::{PageTable, PteFlags, SATP_MODE_SV39, ppn, vpn0, vpn1, vpn2};
use runtime::arch::region;
use runtime::dev::dt::Fdt;
use runtime::dev::dt::memory::MemoryIter;
use runtime::dev::uart::ns16550::NS16550;
use runtime::kernel::console::{CONSOLE, Console};
use runtime::mm::addr::{Pa, Va};
use runtime::mm::region::Region;
use runtime::util::consts::*;

/// Enable the temporary bootstrap address space and jump to `entry`.
///
/// # Safety
///
/// `entry` must be a valid low-address function pointer that remains reachable
/// after adding `KERNEL_VMA_OFFSET`. `dtb_ptr` must point to a valid FDT while
/// the temporary page tables are being built, and this function must run only
/// on the boot hart before normal runtime initialization.
#[unsafe(link_section = ".init.text")]
pub unsafe fn enable_mmu_and_jump(entry: usize, hart_id: usize, dtb_ptr: *const u8) -> ! {
    const L2_PAGE_SIZE: NonZeroUsize = NonZeroUsize::new(1 * G).unwrap();
    const L1_PAGE_SIZE: NonZeroUsize = HUGE_PAGE_SIZE;

    // Temporary Sv39 root page table using 1GiB leaf mapping for whole memory
    #[unsafe(link_section = ".init.bss")]
    static mut TEMP_ROOT: MaybeUninit<PageTable> = MaybeUninit::uninit();
    // Temporary Sv39 L1 page table using huge-page leaf mappings for the kernel
    #[unsafe(link_section = ".init.bss")]
    static mut TEMP_KERNEL_L1: MaybeUninit<PageTable> = MaybeUninit::uninit();

    unsafe {
        let fdt = Fdt::new(dtb_ptr).unwrap();
        let regs = MemoryIter::new(&fdt);

        // Build a temporary bootstrap address space:
        // - identity RAM map for the instructions immediately after satp
        // - linear direct map for early physical access after the jump
        // - kernel image map at its linked high virtual address
        let root = &mut *PageTable::init_raw_mut(&raw mut TEMP_ROOT);
        let kernel_l1 = &mut *PageTable::init_raw_mut(&raw mut TEMP_KERNEL_L1);

        let flag =
            PteFlags::V | PteFlags::R | PteFlags::W | PteFlags::X | PteFlags::A | PteFlags::D;

        // create direct map for physical RAM section(QEMU: 0x8000_0000 ~)
        {
            for (addr, size) in regs {
                let region = Region::from_size(Pa::new(addr as usize), size).unwrap();
                let mut pa = region.start.align_down(L2_PAGE_SIZE);
                while pa < region.end {
                    // identical mapping
                    (*root)
                        .entry(vpn2(Va::new(pa.as_raw())))
                        .mut_address(pa)
                        .mut_flags(flag);
                    // direct mapping
                    (*root)
                        .entry(vpn2(pa.into_va()))
                        .mut_address(pa)
                        .mut_flags(flag);
                    pa = pa.checked_offset(L2_PAGE_SIZE.get()).unwrap();
                }
            }
        }

        // create kernel map starting from KERNEL_VMA_BASE
        {
            (*root)
                .entry(vpn2(Va::new(KERNEL_VMA_BASE)))
                .mut_address(Pa::new(kernel_l1 as *mut _ as usize))
                .mut_flags(PteFlags::V);

            let mut va = Va::new(&raw const _kernel_start as usize)
                .checked_offset(KERNEL_VMA_OFFSET)
                .unwrap();
            assert!(va == KERNEL_VMA_BASE);
            let end = Va::new((&raw const _kernel_end) as usize)
                .checked_offset(KERNEL_VMA_OFFSET)
                .unwrap();
            while va < end {
                (*kernel_l1)
                    .entry(vpn1(va))
                    .mut_address(va.into_pa())
                    .mut_flags(flag);
                va = va.checked_offset(L1_PAGE_SIZE.get()).unwrap();
            }
        }

        // create MMIO mapping, especially for console
        {
            let console = Pa::new(0x1000_0000);
            let page = console.align_down(L2_PAGE_SIZE);
            (*root)
                .entry(vpn2(page.into_va()))
                .mut_address(page)
                .mut_flags(flag);
            ptr::write(
                CONSOLE.lock().deref_mut(),
                Console::Ns16550(NS16550::new(console.into_va().as_raw())),
            );
        }

        let satp = SATP_MODE_SV39 | ppn(Pa::new(root as *mut _ as usize));
        asm!("sfence.vma zero, zero", options(nostack, preserves_flags));
        asm!("csrw satp, {}", in(reg) satp, options(nostack, preserves_flags));
        asm!("sfence.vma zero, zero", options(nostack, preserves_flags));

        let entry = entry.checked_add(KERNEL_VMA_OFFSET).expect("invalid entry");

        // Enter the high-kernel world without returning through the low-address
        // call stack. The temporary identity map exists only to execute the
        // instructions between `csrw satp` and this jump.
        asm!(
            "add sp, sp, t1",
            "jr  t0",
            in("t0") entry,
            in("t1") KERNEL_VMA_OFFSET,
            in("a0") hart_id,
            in("a1") dtb_ptr,
            clobber_abi("C"),
            options(noreturn),
        );
    }
}

#[unsafe(link_section = ".init.text")]
pub fn init_page_table(fdt: &Fdt, mut alloc: impl FnMut() -> &'static mut MaybeUninit<PageTable>) {
    #[unsafe(link_section = ".init.text")]
    fn map(
        l2: &mut PageTable,
        va: Va,
        mut alloc: impl FnMut() -> &'static mut MaybeUninit<PageTable>,
        flags: PteFlags,
    ) {
        let flags = flags | PteFlags::V | PteFlags::A | PteFlags::D;
        let l1 = l2.entry(vpn2(va)).or_insert_with(|| alloc());
        let l0 = l1.entry(vpn1(va)).or_insert_with(|| alloc());
        l0.entry(vpn0(va))
            .mut_address(va.into_pa())
            .mut_flags(flags);
    }

    #[unsafe(link_section = ".init.text")]
    fn map_region(
        l2: &mut PageTable,
        region: Region,
        mut alloc: impl FnMut() -> &'static mut MaybeUninit<PageTable>,
        to_va: impl Fn(Pa) -> Va,
        flags: PteFlags,
    ) {
        if region.is_empty() {
            return;
        }

        let mut pa = region.start.align_down(PAGE_SIZE);
        let end = region.end.align_up(PAGE_SIZE);
        while pa < end {
            map(l2, to_va(pa), &mut alloc, flags);
            pa = pa.checked_offset(PAGE_SIZE.get()).unwrap();
        }
    }

    #[unsafe(link_section = ".init.text")]
    fn map_physical_memories(
        fdt: &Fdt,
        mut alloc: impl FnMut() -> &'static mut MaybeUninit<PageTable>,
        root: &mut PageTable,
    ) {
        let regs = MemoryIter::new(fdt);
        let live_kernel = region::live();
        assert_eq!(live_kernel.start.align_down(PAGE_SIZE), live_kernel.start);
        assert_eq!(live_kernel.end.align_down(PAGE_SIZE), live_kernel.end);

        let flags = PteFlags::R | PteFlags::W;
        for (addr, size) in regs {
            let region = Region::from_size(Pa::new(addr as usize), size).unwrap();

            let mut pa = region.start.align_down(PAGE_SIZE);
            let end = region.end.align_up(PAGE_SIZE);
            while pa < end {
                if !live_kernel.contains(pa) {
                    map(root, pa.into_va(), &mut alloc, flags);
                }
                pa = pa.checked_offset(PAGE_SIZE.get()).unwrap();
            }
        }
    }

    // Map the high kernel image with page-granular permissions. The init
    // island is temporary and reclaimable, so keep it broadly accessible until
    // boot code has fully handed off to runtime-owned stacks and text.
    #[unsafe(link_section = ".init.text")]
    fn map_kernel(
        mut alloc: impl FnMut() -> &'static mut MaybeUninit<PageTable>,
        root: &mut PageTable,
    ) {
        let kernel = region::kernel();
        assert_eq!(kernel.start.into_kernel_va(), KERNEL_VMA_BASE);

        let init = region::init();
        let rx = region::rx();
        let r = region::r();
        let rw = region::rw();

        assert_eq!(init.start.align_down(PAGE_SIZE), init.start);
        assert_eq!(init.end.align_down(PAGE_SIZE), init.end);
        assert_eq!(rx.start.align_down(PAGE_SIZE), rx.start);
        assert_eq!(rx.end.align_down(PAGE_SIZE), rx.end);
        assert_eq!(r.start.align_down(PAGE_SIZE), r.start);
        assert_eq!(r.end.align_down(PAGE_SIZE), r.end);
        assert_eq!(rw.start.align_down(PAGE_SIZE), rw.start);
        assert_eq!(rw.end.align_down(PAGE_SIZE), rw.end);

        map_region(
            root,
            init,
            &mut alloc,
            Pa::into_kernel_va,
            PteFlags::R | PteFlags::W | PteFlags::X,
        );
        map_region(
            root,
            rx,
            &mut alloc,
            Pa::into_kernel_va,
            PteFlags::R | PteFlags::X,
        );
        map_region(root, r, &mut alloc, Pa::into_kernel_va, PteFlags::R);
        map_region(
            root,
            rw,
            &mut alloc,
            Pa::into_kernel_va,
            PteFlags::R | PteFlags::W,
        );
    }

    let root = PageTable::init_mut(alloc());
    map_physical_memories(fdt, &mut alloc, root);
    map_kernel(&mut alloc, root);

    // TODO: generalize from reading FDT with MMIO_MAP_ADDR
    {
        let flags = PteFlags::R | PteFlags::W;
        let uart = Pa::new(0x1000_0000).into_va();
        map(root, uart, &mut alloc, flags);
        unsafe {
            ptr::write(
                CONSOLE.lock().deref_mut(),
                Console::Ns16550(NS16550::new(uart.as_raw())),
            );
        }
    }

    unsafe {
        let satp = SATP_MODE_SV39 | ppn(Va::from(root).into_pa());
        asm!("sfence.vma zero, zero", options(nostack, preserves_flags));
        asm!("csrw satp, {}", in(reg) satp, options(nostack, preserves_flags));
        asm!("sfence.vma zero, zero", options(nostack, preserves_flags));
    }
}
