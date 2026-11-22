//! RISC-V page tables built in physical memory before translation is enabled.

use core::mem::MaybeUninit;

use runtime::arch::consts::PAGE_SIZE;
use runtime::arch::page_table::{PageTable, PteFlags};
use runtime::mm::addr::Pa;
use runtime::mm::region::Region;

pub use super::native::paging::*;

pub struct BootstrapPageTable<const ROOT_LEVEL: u32> {
    root: &'static mut PageTable,
    tables: &'static mut [MaybeUninit<PageTable>],
    flags: PteFlags,
}

impl<const ROOT_LEVEL: u32> BootstrapPageTable<ROOT_LEVEL> {
    /// # Safety
    /// Called once on the boot hart with translation disabled. The table and
    /// all its mappings must stop being used before init memory is reclaimed.
    #[unsafe(link_section = ".init.text")]
    pub unsafe fn new(flags: PteFlags) -> Self {
        #[unsafe(link_section = ".init.bss")]
        static mut ROOT: MaybeUninit<PageTable> = MaybeUninit::uninit();
        // Reclaimable scratch for subtrees at RAM edges and no-map holes.
        #[unsafe(link_section = ".init.bss")]
        static mut TABLES: [MaybeUninit<PageTable>; 32] = [const { MaybeUninit::uninit() }; 32];

        Self {
            root: unsafe { &mut *PageTable::init_raw_mut(&raw mut ROOT) },
            tables: unsafe { &mut *(&raw mut TABLES) },
            flags,
        }
    }

    /// Map the largest aligned leaves that fit entirely inside this RAM interval.
    #[unsafe(link_section = ".init.text")]
    pub fn map(&mut self, range: Region, offset: usize) {
        let entries = self.root.len();
        let mut pa = range.start;
        while pa < range.end {
            let va = pa
                .as_raw()
                .checked_add(offset)
                .expect("bootstrap VA overflow");
            let mut table = &mut *self.root;
            for level in (0..=ROOT_LEVEL).rev() {
                let size = PAGE_SIZE.get() * entries.pow(level);
                let pte = table.entry((va / size) % entries);
                if (va | pa.as_raw()).is_multiple_of(size)
                    && size <= range.end.as_raw() - pa.as_raw()
                {
                    pte.mut_address(pa).mut_flags(self.flags);
                    pa = pa.offset(size);
                    break;
                }
                if !pte.is_valid() {
                    let slot = self
                        .tables
                        .split_off_first_mut()
                        .expect("bootstrap page-table pool exhausted");
                    let child = PageTable::init_mut(slot);
                    pte.mut_address(Pa::new(child as *mut _ as usize))
                        .mut_flags(PteFlags::V);
                }
                assert!(!pte.is_leaf());
                // With translation disabled the child PTE holds a usable physical pointer.
                table = unsafe { &mut *(pte.address().as_raw() as *mut PageTable) };
            }
        }
    }

    #[unsafe(link_section = ".init.text")]
    pub fn root_pa(self) -> Pa {
        Pa::new(self.root as *mut _ as usize)
    }
}
