use runtime::arch::consts::*;
use runtime::arch::page_table::PteFlags;
use runtime::arch::paging::{PageTable, Permission as Perm};
use runtime::arch::{asm, region};
use runtime::asm;
use runtime::dev::dt::Fdt;
use runtime::mm::addr::{Pa, Va};
use runtime::mm::region::Region;

use crate::arch::rv::paging::BootstrapPageTable;
use crate::boot::build_bootstrap_mappings;
use crate::bump::MemoryMap;

/// Highest page-table level in Sv32.
const ROOT_LEVEL: u32 = 1;

/// Install bootstrap Sv32 mappings and enter the linked high kernel.
///
/// # Safety
/// Called once after BSS and stack initialization, in S-mode with IRQs off.
/// `dtb` must point to an immutable firmware FDT in RAM and remain readable
/// while constructing the bootstrap mappings.
#[unsafe(link_section = ".init.text")]
pub unsafe fn enable_mmu_and_jump(entry: usize, hart_id: usize, dtb: *const u8) -> ! {
    unsafe {
        let fdt = Fdt::new(dtb).expect("invalid firmware DTB");
        let offset = kernel_vma_offset();
        let flags =
            PteFlags::V | PteFlags::R | PteFlags::W | PteFlags::X | PteFlags::A | PteFlags::D;
        let mut table = BootstrapPageTable::<ROOT_LEVEL>::new(flags);
        build_bootstrap_mappings(&fdt, |region, offset| table.map(region, offset));
        asm::page_table::activate_from_pa(table.root_pa());

        asm!("add sp, sp, t1", "jr t0",
            in("t0") entry + offset, in("t1") offset,
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
        let live_kernel = region::live();
        assert_eq!(live_kernel.start.align_down(PAGE_SIZE), live_kernel.start);
        assert_eq!(live_kernel.end.align_down(PAGE_SIZE), live_kernel.end);

        let perms = Perm::R | Perm::W;
        MemoryMap::new(fdt).for_each_mappable_ram(|region| {
            let mut pa = region.start;
            while pa < region.end {
                if !live_kernel.contains(pa) {
                    root.map(pa.into_va(), pa, PAGE_SIZE, perms);
                }
                pa = pa.offset(PAGE_SIZE);
            }
        });
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
