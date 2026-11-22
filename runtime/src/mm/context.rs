use alloc::collections::btree_map::BTreeMap;
use core::result::Result;

use crate::arch;
use crate::arch::consts::{LOWER_CANONICAL_END, PAGE_SIZE};
use crate::arch::paging::{PageTable, Permission};
use crate::kernel::per_core::PerCore;
use crate::mm::Pages;
use crate::mm::addr::{Uva, Va};
use crate::util::Bitmap;

pub struct MmContext {
    page_table: PageTable,
    mappings: Mappings,

    /// Each hart needs a local `fence.i` before its instruction fetches are
    /// guaranteed to see the code written by the loader. A set bit at a hart's
    /// `PerCore` index records that it has run the fence. Empty for kernel threads.
    icache_synced_harts: Bitmap,
}

impl Default for MmContext {
    fn default() -> Self {
        Self::new()
    }
}

impl MmContext {
    pub fn new() -> Self {
        Self {
            page_table: PageTable::new_from_active(),
            mappings: Mappings::default(),
            icache_synced_harts: Bitmap::new(0),
        }
    }

    pub fn new_for_exec() -> Self {
        Self {
            page_table: PageTable::new_from_active(),
            mappings: Mappings::default(),
            icache_synced_harts: Bitmap::new(PerCore::count()),
        }
    }

    /// Activate this memory context on the current hart.
    ///
    /// # Safety
    ///
    /// This context and every page table reachable from its root must remain
    /// alive until another context is activated. Its mappings must support
    /// continued execution on the current code and stack and must provide a
    /// valid trap path. The caller must serialize the transition against
    /// interrupts, concurrent mutation, and destruction of this context.
    pub unsafe fn activate(&mut self) {
        unsafe { arch::asm::page_table::activate(self.page_table.as_ptr()) };

        // TODO: arch-specific implementation
        // For AMD64/x86, not require to flush I-cache since it is always consistent to D-cache
        // For ARM64, depends on case by case
        if self.icache_synced_harts.is_empty() {
            return;
        }
        let core_index = PerCore::current_index();
        if !self.icache_synced_harts.is_set(core_index) {
            // The scheduler's release/acquire handoff publishes the loader's
            // stores to a new hart. Synchronize local instruction fetch as well.
            unsafe { core::arch::asm!("fence.i", options(nostack)) };
            self.icache_synced_harts.set(core_index);
        }
    }

    pub fn map_user_page(
        &mut self,
        addr: Va,
        pages: Pages,
        permissions: Permission,
    ) -> Result<(), Pages> {
        let Ok(addr) = Uva::try_from(addr) else {
            return Err(pages);
        };

        let size = pages.size();
        let pa = pages.addr();
        assert_eq!(size, PAGE_SIZE, "TODO: later support contiguous pages");
        if addr.align_down(PAGE_SIZE) != addr {
            return Err(pages);
        }

        self.mappings.insert(addr, pages, permissions)?;
        if !self.page_table.map(addr, pa, size, permissions) {
            return Err(self.mappings.remove(addr).unwrap());
        }

        Ok(())
    }

    pub fn is_accessible(&self, addr: Uva, len: usize, permissions: Permission) -> bool {
        self.mappings.is_accessible(addr, len, permissions)
    }
}

#[derive(Default)]
struct Mappings {
    inner: BTreeMap<Uva, Mapping>,
}

struct Mapping {
    pages: Pages,
    permissions: Permission,
}

impl Mappings {
    pub fn insert(
        &mut self,
        addr: Uva,
        pages: Pages,
        permissions: Permission,
    ) -> Result<(), Pages> {
        let Some(end) = mapping_end(addr, pages.size().get()) else {
            return Err(pages);
        };

        if let Some((prev_addr, prev)) = self.inner.range(..=addr).next_back()
            && mapping_end(*prev_addr, prev.pages.size().get())
                .is_none_or(|prev_end| prev_end > addr.as_raw())
        {
            return Err(pages);
        }

        if let Some((next_addr, _)) = self.inner.range(addr..).next()
            && end > next_addr.as_raw()
        {
            return Err(pages);
        }

        self.inner.insert(addr, Mapping { pages, permissions });
        Ok(())
    }

    pub fn remove(&mut self, addr: Uva) -> Option<Pages> {
        self.inner.remove(&addr).map(|mapping| mapping.pages)
    }

    fn is_accessible(&self, addr: Uva, len: usize, permissions: Permission) -> bool {
        if len == 0 {
            return true;
        }
        let Some(last) = addr.checked_offset(len - 1) else {
            return false;
        };

        let mut current = addr;
        loop {
            let Some((mapping_addr, mapping)) = self.inner.range(..=current).next_back() else {
                return false;
            };
            let Some(mapping_last) = mapping_addr.checked_offset(mapping.pages.size().get() - 1)
            else {
                return false;
            };
            if current > mapping_last || !mapping.permissions.contains(permissions) {
                return false;
            }
            if last <= mapping_last {
                return true;
            }
            let Some(next) = mapping_last.checked_offset(1usize) else {
                return false;
            };
            current = next;
        }
    }
}

fn mapping_end(addr: Uva, size: usize) -> Option<usize> {
    addr.as_raw()
        .checked_add(size)
        .filter(|end| *end <= LOWER_CANONICAL_END)
}
