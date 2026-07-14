//! RISC-V boot-time page-table setup.
//!
//! Boot first installs a temporary address space so execution can jump from the
//! low physical entry point to the high kernel virtual address. It later builds
//! the final runtime page table from firmware memory information.

use core::alloc::Allocator;
use core::mem::MaybeUninit;
use core::num::NonZeroUsize;
use core::ops::DerefMut;
use core::ptr;

use runtime::arch::consts::*;
use runtime::arch::page_table::{PageTable, PteFlags, vpn0, vpn1, vpn2};
use runtime::arch::paging::PageTableRoot;
use runtime::arch::{asm, region};
use runtime::asm;
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
                let region = Region::from_size(Pa::new(addr as usize), size);
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
                    pa = pa.offset(L2_PAGE_SIZE);
                }
            }
        }

        // create kernel map starting from KERNEL_VMA_BASE
        {
            (*root)
                .entry(vpn2(Va::new(KERNEL_VMA_BASE)))
                .mut_address(Pa::new(kernel_l1 as *mut _ as usize))
                .mut_flags(PteFlags::V);

            let mut va = Va::new(&raw const _kernel_start as usize).offset(KERNEL_VMA_OFFSET);
            assert!(va == KERNEL_VMA_BASE);
            let end = Va::new((&raw const _kernel_end) as usize).offset(KERNEL_VMA_OFFSET);
            while va < end {
                (*kernel_l1)
                    .entry(vpn1(va))
                    .mut_address(va.into_pa())
                    .mut_flags(flag);
                va = va.offset(L1_PAGE_SIZE);
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

        // SAFETY: the temporary tables are static, fully initialized, and map
        // both the current low execution state and the high kernel entry used
        // immediately below. Interrupts are not enabled during early boot.
        asm::page_table::activate_from_pa(Pa::new(root as *mut _ as usize));

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
/// Build and activate the final kernel page table.
///
/// # Safety
///
/// The allocator must keep every returned allocation valid while this page
/// table is active. It must also return uniquely owned allocations.
pub unsafe fn init_page_table(fdt: &Fdt, alloc: &impl Allocator) {
    #[unsafe(link_section = ".init.text")]
    fn map<A: Allocator + Clone>(root: &mut PageTableRoot<A>, va: Va, flags: PteFlags) {
        let flags = flags | PteFlags::V | PteFlags::A | PteFlags::D;

        root.cursor()
            .entry(vpn2(va))
            .or_insert()
            .entry(vpn1(va))
            .or_insert()
            .entry(vpn0(va))
            .mut_address(va.into_pa())
            .mut_flags(flags);
    }

    #[unsafe(link_section = ".init.text")]
    fn map_region<A: Allocator + Clone>(
        root: &mut PageTableRoot<A>,
        region: Region,
        to_va: impl Fn(Pa) -> Va,
        flags: PteFlags,
    ) {
        if region.is_empty() {
            return;
        }

        let mut pa = region.start.align_down(PAGE_SIZE);
        let end = region.end.align_up(PAGE_SIZE);
        while pa < end {
            map(root, to_va(pa), flags);
            pa = pa.offset(PAGE_SIZE);
        }
    }

    #[unsafe(link_section = ".init.text")]
    fn map_physical_memories<A: Allocator + Clone>(fdt: &Fdt, root: &mut PageTableRoot<A>) {
        let regs = MemoryIter::new(fdt);
        let live_kernel = region::live();
        assert_eq!(live_kernel.start.align_down(PAGE_SIZE), live_kernel.start);
        assert_eq!(live_kernel.end.align_down(PAGE_SIZE), live_kernel.end);

        let flags = PteFlags::R | PteFlags::W;
        for (addr, size) in regs {
            let region = Region::from_size(Pa::new(addr as usize), size);

            let mut pa = region.start.align_down(PAGE_SIZE);
            let end = region.end.align_up(PAGE_SIZE);
            while pa < end {
                if !live_kernel.contains(pa) {
                    map(root, pa.into_va(), flags);
                }
                pa = pa.offset(PAGE_SIZE);
            }
        }
    }

    // Map the high kernel image with page-granular permissions. The init
    // island is temporary and reclaimable, so keep it broadly accessible until
    // boot code has fully handed off to runtime-owned stacks and text.
    #[unsafe(link_section = ".init.text")]
    fn map_kernel<A: Allocator + Clone>(root: &mut PageTableRoot<A>) {
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
            Pa::into_kernel_va,
            PteFlags::R | PteFlags::W | PteFlags::X,
        );
        map_region(root, rx, Pa::into_kernel_va, PteFlags::R | PteFlags::X);
        map_region(root, r, Pa::into_kernel_va, PteFlags::R);
        map_region(root, rw, Pa::into_kernel_va, PteFlags::R | PteFlags::W);
    }

    let mut root = PageTableRoot::new(alloc);
    // Child page tables are owned by the root and use the allocator retained
    // by the root Box, rather than an independently threaded allocator value.
    map_physical_memories(fdt, &mut root);
    map_kernel(&mut root);

    // TODO: generalize from reading FDT with MMIO_MAP_ADDR
    {
        let flags = PteFlags::R | PteFlags::W;
        let uart = Pa::new(0x1000_0000).into_va();
        map(&mut root, uart, flags);
        unsafe {
            ptr::write(
                CONSOLE.lock().deref_mut(),
                Console::Ns16550(NS16550::new(uart.as_raw())),
            );
        }
    }

    let root = PageTableRoot::leak(root);
    // SAFETY: leaking the root keeps the complete page-table tree alive for the
    // remainder of boot. It maps the current init code and stack as well as the
    // runtime kernel and direct-map regions needed after this transition.
    unsafe { asm::page_table::activate(root) };
}
