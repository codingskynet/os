//! Guarded virtual-address slots for kernel thread stacks.

use crate::arch;
use crate::arch::consts::*;
use crate::arch::paging::{Permission, map_kernel_page_to_active, unmap_page_from_active};
use crate::kernel::sync::SpinLock;
use crate::mm::Pages;
use crate::mm::addr::Va;

const SLOT_COUNT: usize = KERNEL_STACK_VMA_SIZE / KERNEL_STACK_SLOT_SIZE;
const SLOT_WORDS: usize = SLOT_COUNT / u64::BITS as usize;

const MAIN_STACK_OFFSET: usize = KERNEL_STACK_GUARD_SIZE;
const MAIN_STACK_END: usize = MAIN_STACK_OFFSET + STACK_SIZE.get();

const _: () = assert!(KERNEL_STACK_VMA_SIZE.is_multiple_of(KERNEL_STACK_SLOT_SIZE));
const _: () = assert!(SLOT_COUNT.is_multiple_of(u64::BITS as usize));
const _: () = assert!(MAIN_STACK_END <= KERNEL_STACK_SLOT_SIZE);

// TODO: explicitly allocate page tables before initialization
const _: () = assert!(((KERNEL_STACK_VMA_BASE >> 30) & 0x1ff) == ((KERNEL_VMA_BASE >> 30) & 0x1ff));

static STACK_SLOTS: SpinLock<StackSlots> = SpinLock::new(StackSlots::new());

/// One thread's guarded kernel stack.
///
/// The physical pages remain reachable through the direct map, but execution
/// uses only the guarded virtual mappings described below:
///
/// ```text
///                         higher virtual addresses
///
/// slot base + 64 KiB  +------------------------------+  slot end
///                     |                              |
///                     |      unmapped remainder      |  44 KiB
///                     |                              |
/// slot base + 20 KiB  +------------------------------+  stack top (initial sp)
///                     |                              |
///                     |     mapped kernel stack      |  16 KiB
///                     |       normal frames          |
///                     |              |               |
///                     |              v grows down    |
/// slot base +  4 KiB  +------------------------------+  stack bottom
///                     |  guard page (unmapped)       |   4 KiB
/// slot base           +------------------------------+
///
///                          lower virtual addresses
/// ```
pub struct KernelStack {
    slot: usize,
    pages: Pages,
}

impl KernelStack {
    pub fn new() -> Self {
        let pages = Pages::new(STACK_SIZE).expect("out of memory for kernel stack");

        let slot = {
            let mut slots = STACK_SLOTS.lock();
            let slot = slots
                .allocate()
                .expect("kernel stack virtual area exhausted");
            let base = slot_base(slot);
            slots.map_pages(base.offset(MAIN_STACK_OFFSET), &pages);
            arch::asm::page_table::flush_tlb();
            slot
        };

        Self { slot, pages }
    }

    pub fn top(&self) -> Va {
        slot_base(self.slot).offset(MAIN_STACK_END)
    }

    pub fn bottom(&self) -> Va {
        slot_base(self.slot).offset(MAIN_STACK_OFFSET)
    }
}

impl Drop for KernelStack {
    fn drop(&mut self) {
        let mut slots = STACK_SLOTS.lock();
        let base = slot_base(self.slot);
        slots.unmap_pages(base.offset(MAIN_STACK_OFFSET), &self.pages);
        arch::asm::page_table::flush_tlb();
        slots.release(self.slot);
    }
}

struct StackSlots {
    used: [u64; SLOT_WORDS],
}

impl StackSlots {
    // This is evaluated only for the static initializer; the bitmap is emitted
    // directly into BSS and is never materialized on a runtime stack.
    #[allow(clippy::large_stack_arrays)]
    const fn new() -> Self {
        Self {
            used: [0; SLOT_WORDS],
        }
    }

    fn allocate(&mut self) -> Option<usize> {
        for (word_index, word) in self.used.iter_mut().enumerate() {
            if *word == u64::MAX {
                continue;
            }
            let bit = (!*word).trailing_zeros() as usize;
            *word |= 1 << bit;
            return Some(word_index * u64::BITS as usize + bit);
        }
        None
    }

    fn release(&mut self, slot: usize) {
        let word = &mut self.used[slot / u64::BITS as usize];
        let mask = 1 << (slot % u64::BITS as usize);
        assert_ne!(*word & mask, 0, "releasing an unused kernel stack slot");
        *word &= !mask;
    }

    fn map_pages(&mut self, start: Va, pages: &Pages) {
        let mut offset = 0;
        while offset < pages.size().get() {
            let va = start.offset(offset);
            let pa = pages.addr().offset(offset);
            assert!(
                // SAFETY: STACK_SLOTS serializes every kernel-stack mapping
                // mutation, and this allocator exclusively owns the slot.
                unsafe { map_kernel_page_to_active(va, pa, Permission::R | Permission::W) },
                "kernel stack VA is already mapped: {va}"
            );
            offset += PAGE_SIZE.get();
        }
    }

    fn unmap_pages(&mut self, start: Va, pages: &Pages) {
        let mut offset = 0;
        while offset < pages.size().get() {
            let va = start.offset(offset);
            let pa = pages.addr().offset(offset);
            assert_eq!(
                // SAFETY: STACK_SLOTS serializes mapping mutations and Thread
                // destruction happens only after execution left this stack.
                unsafe { unmap_page_from_active(va) },
                Some(pa),
                "kernel stack mapping changed unexpectedly: {va}"
            );
            offset += PAGE_SIZE.get();
        }
    }
}

fn slot_base(slot: usize) -> Va {
    debug_assert!(slot < SLOT_COUNT);
    Va::new(KERNEL_STACK_VMA_BASE + slot * KERNEL_STACK_SLOT_SIZE)
}
