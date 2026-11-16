use core::mem::MaybeUninit;

use runtime::arch::consts::*;
use runtime::arch::page_table::{PageTable as RawPageTable, PteFlags, vpn1};
use runtime::arch::paging::{PageTable, Permission as Perm};
use runtime::arch::{asm, region};
use runtime::asm;
use runtime::dev::dt::Fdt;
use runtime::dev::dt::memory::MemoryIter;
use runtime::mm::addr::{Pa, Va};
use runtime::mm::region::Region;

/// Install bootstrap Sv32 superpages and enter the linked high kernel.
///
/// # Safety
/// Called once after BSS and stack initialization, in S-mode with IRQs off.
/// `dtb` must point to an immutable firmware FDT in RAM and remain readable
/// while constructing the bootstrap mappings.
#[unsafe(link_section = ".init.text")]
pub unsafe fn enable_mmu_and_jump(entry: usize, hart_id: usize, dtb: *const u8) -> ! {
    #[unsafe(link_section = ".init.bss")]
    static mut ROOT: MaybeUninit<RawPageTable> = MaybeUninit::uninit();
    unsafe {
        let root = &mut *RawPageTable::init_raw_mut(&raw mut ROOT);
        let flags =
            PteFlags::V | PteFlags::R | PteFlags::W | PteFlags::X | PteFlags::A | PteFlags::D;
        let fdt = Fdt::new(dtb).expect("invalid firmware DTB");
        // Discover RAM from firmware before enabling paging.
        let mut dtb_in_ram = false;
        for memory in MemoryIter::new(&fdt) {
            let (base, size) = memory.expect("unsupported firmware RAM region");
            let end = base.checked_add(size.get()).expect("RAM range overflow");
            dtb_in_ram |= dtb as usize >= base
                && (dtb as usize)
                    .checked_add(fdt.total_size())
                    .is_some_and(|v| v <= end);
            let mut pa = Pa::new(base).align_down(HUGE_PAGE_SIZE);
            while pa.as_raw() < end {
                for va in [Va::new(pa.as_raw()), pa.into_va()] {
                    root.entry(vpn1(va)).mut_address(pa).mut_flags(flags);
                }
                pa = pa.offset(HUGE_PAGE_SIZE);
            }
        }
        assert!(dtb_in_ram, "firmware DTB is outside declared RAM");
        // The image's linked load address remains a kernel linker property.
        let mut pa = Pa::new(KERNEL_LMA_BASE);
        let end = &raw const _kernel_end as usize + KERNEL_VMA_OFFSET;
        let mut va = KERNEL_VMA_BASE;
        while va < end {
            root.entry(vpn1(Va::new(va)))
                .mut_address(pa)
                .mut_flags(flags);
            pa = pa.offset(HUGE_PAGE_SIZE);
            va += HUGE_PAGE_SIZE.get();
        }
        asm::page_table::activate_from_pa(Pa::new(root as *mut _ as usize));
        asm!("add sp, sp, t1", "jr t0",
            in("t0") entry + KERNEL_VMA_OFFSET, in("t1") KERNEL_VMA_OFFSET,
            in("a0") hart_id, in("a1") dtb,
            clobber_abi("C"), options(noreturn));
    }
}

/// Build and activate the final kernel page table.
///
/// # Safety
///
/// The buddy allocator and physical-page metadata must already be initialized.
/// The resulting root is leaked and remains active for the kernel lifetime.
#[unsafe(link_section = ".init.text")]
pub unsafe fn init_page_table(fdt: &Fdt) {
    #[unsafe(link_section = ".init.text")]
    fn map_region(root: &mut PageTable, region: Region, to_va: impl Fn(Pa) -> Va, perms: Perm) {
        if region.is_empty() {
            return;
        }

        let mut pa = region.start.align_down(PAGE_SIZE);
        let end = region.end.align_up(PAGE_SIZE);
        while pa < end {
            root.map(to_va(pa), pa, PAGE_SIZE, perms);
            pa = pa.offset(PAGE_SIZE);
        }
    }

    #[unsafe(link_section = ".init.text")]
    fn map_physical_memories(fdt: &Fdt, root: &mut PageTable) {
        let regs = MemoryIter::new(fdt);
        let live_kernel = region::live();
        assert_eq!(live_kernel.start.align_down(PAGE_SIZE), live_kernel.start);
        assert_eq!(live_kernel.end.align_down(PAGE_SIZE), live_kernel.end);

        let perms = Perm::R | Perm::W;
        for reg in regs {
            let (addr, size) = reg.expect("memory reg is incompatible with this target");
            let region = Region::from_size(Pa::new(addr), size);

            let mut pa = region.start.align_down(PAGE_SIZE);
            let end = region.end.align_up(PAGE_SIZE);
            while pa < end {
                if !live_kernel.contains(pa) {
                    root.map(pa.into_va(), pa, PAGE_SIZE, perms);
                }
                pa = pa.offset(PAGE_SIZE);
            }
        }
    }

    // Map the high kernel image with page-granular permissions. The init
    // island is temporary and reclaimable, so keep it broadly accessible until
    // boot code has fully handed off to runtime-owned stacks and text.
    #[unsafe(link_section = ".init.text")]
    fn map_kernel(root: &mut PageTable) {
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

        map_region(root, init, Pa::into_kernel_va, Perm::R | Perm::W | Perm::X);
        map_region(root, rx, Pa::into_kernel_va, Perm::R | Perm::X);
        map_region(root, r, Pa::into_kernel_va, Perm::R);
        map_region(root, rw, Pa::into_kernel_va, Perm::R | Perm::W);
    }

    let mut root = PageTable::new();
    map_physical_memories(fdt, &mut root);
    map_kernel(&mut root);

    let root = PageTable::leak(root);
    // SAFETY: leaking the root keeps the complete page-table tree alive for the
    // remainder of boot. It maps the current init code and stack as well as the
    // runtime kernel and direct-map regions needed after this transition.
    unsafe { asm::page_table::activate(root) };
}
