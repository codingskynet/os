use alloc::collections::btree_map::BTreeMap;
use core::num::NonZeroUsize;

use crate::arch;
use crate::arch::paging::PageTable;
use crate::mm::addr::Va;

pub struct MmContext {
    page_table: PageTable,
    mappings: Mappings,
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
    pub unsafe fn activate(&self) {
        unsafe { arch::asm::page_table::activate(self.page_table.as_ptr()) };
    }
}

#[derive(Default)]
struct Mappings {
    inner: BTreeMap<Va, NonZeroUsize>,
}

impl Mappings {
    pub fn insert(&mut self, addr: Va, size: NonZeroUsize) -> bool {
        let end = addr.offset(size);

        if let Some((prev_addr, prev_size)) = self.inner.range(..=addr).next_back()
            && prev_addr.offset(*prev_size) > addr
        {
            return false;
        }

        if let Some((next_addr, _)) = self.inner.range(addr..).next()
            && end > *next_addr
        {
            return false;
        }

        self.inner.insert(addr, size);
        true
    }

    pub fn remove(&mut self, addr: Va) -> Option<Va> {
        self.inner.remove(&addr).map(|_| addr)
    }
}
