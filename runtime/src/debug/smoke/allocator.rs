use core::alloc::{GlobalAlloc, Layout};
use core::num::NonZeroUsize;
use core::ptr;

use crate::arch::consts::PAGE_SIZE;
use crate::arch::interrupt::InterruptGuard;
use crate::debug::dump_page_list;
use crate::mm::addr::{Pa, Va};
use crate::mm::page_meta::PageMetaState;
use crate::mm::{BUDDY, GLOBAL, page_meta_at};
use crate::printlnk;

pub fn smoke() {
    // The smoke test directly inspects allocator metadata, so keep the snapshot
    // stable while dumping and fuzzing it.
    let _guard = InterruptGuard::new();

    dump_page_list();
    printlnk!("{:#?}", *BUDDY.lock());
    run();
    dump_page_list();
    printlnk!("{:#?}", *BUDDY.lock());
}

const ITERATIONS: usize = 65536;
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

/// Minimum guard-band width placed on each side of the user payload.
/// The leading band is padded up to the requested alignment
/// so the payload itself stays aligned.
const REDZONE: usize = 16;

/// Guard byte written into every redzone slot. Any mismatch on verification
/// means the allocator handed out a region that overlaps its neighbour or is
/// smaller than requested (an out-of-bounds / under-allocation bug).
const REDZONE_BYTE: u8 = 0x39;

fn run() {
    printlnk!("allocator fuzz: start");

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

    printlnk!(
        "allocator fuzz: ok allocations={} frees={} checks={}",
        allocations,
        frees,
        checks
    );
}

fn alloc_slot(rng: &mut Rng, step: usize) -> Slot {
    let size = SIZES[rng.index(SIZES.len())];
    let align = ALIGNS[rng.index(ALIGNS.len())];

    // Reserve a guard band on both sides. The leading band is padded up to the
    // alignment so the payload in the middle keeps the requested alignment.
    let head = align_up(REDZONE, align);
    let total = head + size + REDZONE;
    let layout = Layout::from_size_align(total, align).expect("invalid fuzz layout");

    let base = unsafe { GLOBAL.alloc(layout) };
    assert!(
        !base.is_null(),
        "allocator fuzz: allocation failed size={} align={} total={}",
        size,
        align,
        total
    );

    let payload = unsafe { base.add(head) };
    assert_eq!(
        payload as usize & (align - 1),
        0,
        "allocator fuzz: misaligned allocation size={} align={} payload={:#x}",
        size,
        align,
        payload as usize
    );

    Slot {
        base,
        payload,
        size,
        align,
        head,
        total,
        seed: rng.next() ^ ((step as u64) << 32),
    }
}

fn fill_slot(slot: Slot) {
    verify_page_meta(slot);
    write_redzone(slot);

    for offset in 0..slot.size {
        unsafe {
            ptr::write_volatile(slot.payload.add(offset), pattern(slot.seed, offset));
        }
    }
}

fn verify_slot(slot: Slot) {
    verify_page_meta(slot);
    verify_redzone(slot);

    for offset in 0..slot.size {
        let found = unsafe { ptr::read_volatile(slot.payload.add(offset)) };
        let expected = pattern(slot.seed, offset);
        assert_eq!(
            found, expected,
            "allocator fuzz: data mismatch payload={:#x} size={} offset={}",
            slot.payload as usize, slot.size, offset
        );
    }
}

fn dealloc_slot(slot: Slot) {
    // Check the guard bands one last time on free, the way SLUB validates
    // redzones in `kmem_cache_free`.
    verify_redzone(slot);
    unsafe { GLOBAL.dealloc(slot.base, slot.layout()) };
}

/// Paints both guard bands with [`REDZONE_BYTE`].
fn write_redzone(slot: Slot) {
    for offset in 0..slot.head {
        unsafe { ptr::write_volatile(slot.base.add(offset), REDZONE_BYTE) };
    }
    for offset in 0..REDZONE {
        unsafe { ptr::write_volatile(slot.payload.add(slot.size + offset), REDZONE_BYTE) };
    }
}

/// Asserts that neither guard band has been touched, reporting the exact byte
/// on the first mismatch (an out-of-bounds write or overlapping allocation).
fn verify_redzone(slot: Slot) {
    for offset in 0..slot.head {
        let found = unsafe { ptr::read_volatile(slot.base.add(offset)) };
        assert_eq!(
            found, REDZONE_BYTE,
            "allocator fuzz: leading redzone corrupted base={:#x} size={} align={} offset={} found={:#x}",
            slot.base as usize, slot.size, slot.align, offset, found
        );
    }
    for offset in 0..REDZONE {
        let found = unsafe { ptr::read_volatile(slot.payload.add(slot.size + offset)) };
        assert_eq!(
            found, REDZONE_BYTE,
            "allocator fuzz: trailing redzone corrupted payload={:#x} size={} align={} offset={} found={:#x}",
            slot.payload as usize, slot.size, slot.align, offset, found
        );
    }
}

fn verify_page_meta(slot: Slot) {
    match allocation_kind(slot) {
        AllocationKind::Slab { size, block } => verify_slab_page_meta(slot, size, block),
        AllocationKind::Buddy { order } => verify_buddy_page_meta(slot, order),
    }
}

fn allocation_kind(slot: Slot) -> AllocationKind {
    // The allocator sees the padded layout, not the user payload, so classify
    // against the total size.
    let size = slot.total.max(slot.align).max(SLAB_MIN_SIZE);
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
    let block_base = align_down(slot.base as usize, block);
    let block_base_pa = Va::new(block_base).into_pa();
    let page_meta = page_meta_at(block_base_pa);

    let PageMetaState::Slab(slab) = &**page_meta else {
        panic!(
            "allocator fuzz: expected slab page meta base={:#x} block_base={:#x}",
            slot.base as usize, block_base
        );
    };

    assert_eq!(
        slab.size,
        NonZeroUsize::new(size).unwrap(),
        "allocator fuzz: unexpected slab size base={:#x}",
        slot.base as usize
    );
    assert!(
        slab.used > 0,
        "allocator fuzz: allocated slab has zero used count base={:#x}",
        slot.base as usize
    );
    assert!(
        slab.used <= block / size,
        "allocator fuzz: slab used count exceeds capacity base={:#x}",
        slot.base as usize
    );
    assert_eq!(
        slab.buddy_meta.reserved.len() + 1,
        block / PAGE_SIZE.get(),
        "allocator fuzz: slab reserved page count mismatch base={:#x}",
        slot.base as usize
    );

    verify_reserved_pages(block_base_pa, block / PAGE_SIZE.get());
}

fn verify_buddy_page_meta(slot: Slot, order: usize) {
    let block = PAGE_SIZE.get() << order;
    let block_base = align_down(slot.base as usize, block);
    let block_base_pa = Va::new(block_base).into_pa();
    let page_meta = page_meta_at(block_base_pa);

    let PageMetaState::Buddy(buddy) = &**page_meta else {
        panic!(
            "allocator fuzz: expected buddy page meta base={:#x} block_base={:#x}",
            slot.base as usize, block_base
        );
    };

    assert_eq!(
        (buddy.reserved.len() + 1).trailing_zeros() as usize,
        order,
        "allocator fuzz: unexpected buddy order base={:#x} payload_size={} align={} head={} total={}",
        slot.base as usize,
        slot.size,
        slot.align,
        slot.head,
        slot.total
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

fn align_up(addr: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (addr + align - 1) & !(align - 1)
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
    /// Start of the whole allocation, i.e. the first byte of the leading
    /// redzone. This is what gets passed back to the allocator on free.
    base: *mut u8,
    /// User-visible payload pointer (`base + head`), aligned to `align`.
    payload: *mut u8,
    /// Payload length requested by the fuzz "user".
    size: usize,
    /// Requested payload alignment.
    align: usize,
    /// Leading redzone length (`>= REDZONE`, rounded up to `align`).
    head: usize,
    /// Total allocated layout size (`head + size + REDZONE`).
    total: usize,
    seed: u64,
}

impl Slot {
    const fn empty() -> Self {
        Self {
            base: ptr::null_mut(),
            payload: ptr::null_mut(),
            size: 0,
            align: 1,
            head: 0,
            total: 0,
            seed: 0,
        }
    }

    fn is_allocated(&self) -> bool {
        !self.base.is_null()
    }

    fn layout(&self) -> Layout {
        Layout::from_size_align(self.total, self.align).expect("invalid fuzz layout")
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
