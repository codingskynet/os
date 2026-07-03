use core::alloc::{GlobalAlloc, Layout};
use core::num::NonZeroUsize;
use core::ptr;

use crate::arch::consts::PAGE_SIZE;
use crate::mm::addr::{Pa, Va};
use crate::mm::page_meta::PageMetaState;
use crate::mm::{GLOBAL, page_meta_at};
use crate::println;

const ITERATIONS: usize = 16384;
const SLOTS: usize = 64;

const SIZES: [usize; 37] = [
    1, 2, 3, 4, 7, 8, 15, 16, 24, 31, 32, 33, 63, 64, 65, 127, 128, 129, 255, 256, 257, 511, 512,
    513, 1023, 1024, 1025, 2047, 2048, 2049, 4095, 4096, 4097, 6000, 8192, 12000, 16384,
];

const ALIGNS: [usize; 14] = [
    1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192,
];

const SLAB_MIN_SIZE: usize = 32;
const SLAB_MAX_SIZE: usize = 4096;
const OBJECTS_PER_SLAB_BLOCK: usize = 4;

pub fn run() {
    println!("allocator fuzz: start");

    let mut rng = Rng::new(0x0f5a_b10c_a110_ca7e);
    let mut slots = [Slot::empty(); SLOTS];
    let mut allocations = 0;
    let mut frees = 0;
    let mut checks = 0;

    for step in 0..ITERATIONS {
        let index = rng.index(SLOTS);

        if slots[index].is_allocated() {
            verify_slot(slots[index]);
            checks += 1;

            if rng.next() & 0b11 != 0 {
                dealloc_slot(slots[index]);
                slots[index] = Slot::empty();
                frees += 1;
            }
        } else {
            let slot = alloc_slot(&mut rng, step);
            fill_slot(slot);
            slots[index] = slot;
            allocations += 1;
        }

        if step % 17 == 0 {
            let check_index = rng.index(SLOTS);
            if slots[check_index].is_allocated() {
                verify_slot(slots[check_index]);
                checks += 1;
            }
        }
    }

    for slot in &mut slots {
        if slot.is_allocated() {
            verify_slot(*slot);
            dealloc_slot(*slot);
            *slot = Slot::empty();
            checks += 1;
            frees += 1;
        }
    }

    println!(
        "allocator fuzz: ok allocations={} frees={} checks={}",
        allocations, frees, checks
    );
}

fn alloc_slot(rng: &mut Rng, step: usize) -> Slot {
    let size = SIZES[rng.index(SIZES.len())];
    let align = ALIGNS[rng.index(ALIGNS.len())];
    let layout = Layout::from_size_align(size, align).expect("invalid fuzz layout");
    let ptr = unsafe { GLOBAL.alloc(layout) };
    assert!(
        !ptr.is_null(),
        "allocator fuzz: allocation failed size={} align={}",
        size,
        align
    );

    assert_eq!(
        ptr as usize & (align - 1),
        0,
        "allocator fuzz: misaligned allocation size={} align={} ptr={:#x}",
        size,
        align,
        ptr as usize
    );

    Slot {
        ptr,
        size,
        align,
        seed: rng.next() ^ ((step as u64) << 32),
    }
}

fn fill_slot(slot: Slot) {
    verify_page_meta(slot);

    for offset in 0..slot.size {
        unsafe {
            ptr::write_volatile(slot.ptr.add(offset), pattern(slot.seed, offset));
        }
    }
}

fn verify_slot(slot: Slot) {
    verify_page_meta(slot);

    for offset in 0..slot.size {
        let found = unsafe { ptr::read_volatile(slot.ptr.add(offset)) };
        let expected = pattern(slot.seed, offset);
        assert_eq!(
            found, expected,
            "allocator fuzz: data mismatch ptr={:#x} size={} offset={}",
            slot.ptr as usize, slot.size, offset
        );
    }
}

fn dealloc_slot(slot: Slot) {
    let layout = Layout::from_size_align(slot.size, slot.align).expect("invalid fuzz layout");
    unsafe { GLOBAL.dealloc(slot.ptr, layout) };
}

fn verify_page_meta(slot: Slot) {
    match allocation_kind(slot) {
        AllocationKind::Slab { size, block } => verify_slab_page_meta(slot, size, block),
        AllocationKind::Buddy { order } => verify_buddy_page_meta(slot, order),
    }
}

fn allocation_kind(slot: Slot) -> AllocationKind {
    let size = slot.size.max(slot.align).max(SLAB_MIN_SIZE);
    if size <= SLAB_MAX_SIZE {
        let size = size.next_power_of_two();
        AllocationKind::Slab {
            size,
            block: slab_block_size(size),
        }
    } else {
        let size = size.max(PAGE_SIZE.get());
        let pages = size.div_ceil(PAGE_SIZE.get());
        let order = pages.next_power_of_two().trailing_zeros() as usize;
        AllocationKind::Buddy { order }
    }
}

fn verify_slab_page_meta(slot: Slot, size: usize, block: usize) {
    let block_base = align_down(slot.ptr as usize, block);
    let block_base_pa = Va::new(block_base).into_pa();
    let page_meta = page_meta_at(block_base_pa);

    let PageMetaState::Slab(slab) = &**page_meta else {
        panic!(
            "allocator fuzz: expected slab page meta ptr={:#x} block_base={:#x}",
            slot.ptr as usize, block_base
        );
    };

    assert_eq!(
        slab.size,
        NonZeroUsize::new(size).unwrap(),
        "allocator fuzz: unexpected slab size ptr={:#x}",
        slot.ptr as usize
    );
    assert!(
        slab.used > 0,
        "allocator fuzz: allocated slab has zero used count ptr={:#x}",
        slot.ptr as usize
    );
    assert!(
        slab.used <= block / size,
        "allocator fuzz: slab used count exceeds capacity ptr={:#x}",
        slot.ptr as usize
    );
    assert_eq!(
        slab.buddy_meta.reserved.len() + 1,
        block / PAGE_SIZE.get(),
        "allocator fuzz: slab reserved page count mismatch ptr={:#x}",
        slot.ptr as usize
    );

    verify_reserved_pages(block_base_pa, block / PAGE_SIZE.get());
}

fn verify_buddy_page_meta(slot: Slot, order: usize) {
    let block = PAGE_SIZE.get() << order;
    let block_base = align_down(slot.ptr as usize, block);
    let block_base_pa = Va::new(block_base).into_pa();
    let page_meta = page_meta_at(block_base_pa);

    let PageMetaState::Buddy(buddy) = &**page_meta else {
        panic!(
            "allocator fuzz: expected buddy page meta ptr={:#x} block_base={:#x}",
            slot.ptr as usize, block_base
        );
    };

    assert_eq!(
        (buddy.reserved.len() + 1).trailing_zeros() as usize,
        order,
        "allocator fuzz: unexpected buddy order ptr={:#x}",
        slot.ptr as usize
    );

    verify_reserved_pages(block_base_pa, 1 << order);
}

fn verify_reserved_pages(block_base: Pa, pages: usize) {
    for page in 1..pages {
        let page_meta = page_meta_at(Pa::new(block_base.as_raw() + page * PAGE_SIZE.get()));
        assert!(
            matches!(&**page_meta, PageMetaState::BuddyReserved),
            "allocator fuzz: expected reserved page block_base={:#x} page={}",
            block_base.as_raw(),
            page
        );
    }
}

fn slab_block_size(size: usize) -> usize {
    (size * OBJECTS_PER_SLAB_BLOCK).max(PAGE_SIZE.get())
}

fn align_down(addr: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    addr & !(align - 1)
}

fn pattern(seed: u64, offset: usize) -> u8 {
    let mut x = seed ^ (offset as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 29;
    x as u8
}

enum AllocationKind {
    Slab { size: usize, block: usize },
    Buddy { order: usize },
}

#[derive(Clone, Copy)]
struct Slot {
    ptr: *mut u8,
    size: usize,
    align: usize,
    seed: u64,
}

impl Slot {
    const fn empty() -> Self {
        Self {
            ptr: ptr::null_mut(),
            size: 0,
            align: 1,
            seed: 0,
        }
    }

    fn is_allocated(&self) -> bool {
        !self.ptr.is_null()
    }
}

struct Rng {
    state: u64,
}

impl Rng {
    const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 7;
        x ^= x >> 9;
        x ^= x << 8;
        self.state = x;
        x
    }

    fn index(&mut self, len: usize) -> usize {
        (self.next() as usize) % len
    }
}
