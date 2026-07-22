//! Boot-time physical bump allocator.
//!
//! This allocator runs from `.init.*` code before the runtime allocator is
//! ready. It records firmware RAM ranges, reserves the kernel image and DTB,
//! and hands out aligned physical regions for page tables and page metadata.

use core::alloc::{AllocError, Allocator, Layout};
use core::cell::RefCell;
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use core::slice::Iter;

use arrayvec::ArrayVec;
use runtime::arch::{self};
use runtime::dev::dt::Fdt;
use runtime::dev::dt::memory::MemoryIter;
use runtime::kernel::sync::SpinLock;
use runtime::mm::addr::Pa;
use runtime::mm::region::Region;
use runtime::{debug, printlnk};

#[unsafe(link_section = ".init.bss")]
pub static BUMP_ALLOCATOR: SpinLock<BumpAllocator> = SpinLock::new(BumpAllocator::empty());

/// Allocator over all discovered physical memory ranges.
pub struct BumpAllocator {
    memories: RefCell<ArrayVec<Memory, 4>>,
}

unsafe impl Allocator for BumpAllocator {
    #[unsafe(link_section = ".init.text")]
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        for memory in self.memories.borrow().iter() {
            if let Ok(allocation) = memory.allocate(layout) {
                return Ok(allocation);
            }
        }

        Err(AllocError)
    }

    #[unsafe(link_section = ".init.text")]
    unsafe fn deallocate(&self, _ptr: NonNull<u8>, _layout: Layout) {
        // Boot allocations live until the runtime allocator takes ownership of
        // all remaining physical memory, so individual frees are unnecessary.
    }
}

impl BumpAllocator {
    #[unsafe(link_section = ".init.text")]
    pub const fn empty() -> Self {
        Self {
            memories: RefCell::new(ArrayVec::new_const()),
        }
    }

    #[unsafe(link_section = ".init.text")]
    pub fn init(&mut self, fdt: &Fdt) {
        let memories = self.memories.get_mut();
        memories.clear();
        for reg in MemoryIter::new(fdt) {
            let (addr, size) = reg.expect("memory reg is incompatible with this target");
            let region = Region::from_size(Pa::new(addr), size);
            debug!("bump: register memory {region:?}");
            memories.push(Memory::new(region));
        }

        // TODO: add reserve-memory from FDT
        self.reserve(arch::region::kernel());
        self.reserve(Region::from_raw(
            fdt.as_ptr(),
            fdt.as_ptr().wrapping_add(fdt.total_size()),
        ));
    }

    #[unsafe(link_section = ".init.text")]
    fn reserve(&mut self, region: Region) {
        for mem in self.memories.get_mut().iter_mut() {
            if mem.reserve(region).is_ok() {
                return;
            }
        }

        printlnk!("bump: failed to reserve region: {region:?}");
    }

    #[unsafe(link_section = ".init.text")]
    pub fn memories_mut(&mut self) -> &mut [Memory] {
        self.memories.get_mut()
    }
}

/// One physical memory range plus sub-ranges already reserved from it.
pub struct Memory {
    region: Region,
    reserved: RefCell<RegionSet<8>>,
}

unsafe impl Allocator for Memory {
    #[unsafe(link_section = ".init.text")]
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let align = NonZeroUsize::new(layout.align()).unwrap();
        let Some(size) = NonZeroUsize::new(layout.size()) else {
            let ptr = NonNull::without_provenance(align);
            return Ok(NonNull::slice_from_raw_parts(ptr, 0));
        };
        let region = self.alloc(size, align).ok_or(AllocError)?;
        let ptr = NonNull::new(region.start.into_va().as_mut_ptr()).ok_or(AllocError)?;

        Ok(NonNull::slice_from_raw_parts(ptr, size.get()))
    }

    #[unsafe(link_section = ".init.text")]
    unsafe fn deallocate(&self, _ptr: NonNull<u8>, _layout: Layout) {
        // Bump allocations are reclaimed together after boot.
    }
}

impl Memory {
    #[unsafe(link_section = ".init.text")]
    pub fn new(region: Region) -> Self {
        Self {
            region,
            reserved: RefCell::new(RegionSet::new()),
        }
    }

    #[unsafe(link_section = ".init.text")]
    pub fn region(&self) -> Region {
        self.region
    }

    #[unsafe(link_section = ".init.text")]
    pub fn reserved(&mut self) -> &[Region] {
        self.reserved.get_mut().as_slice()
    }

    #[unsafe(link_section = ".init.text")]
    pub fn reserve(&mut self, region: Region) -> Result<(), Region> {
        if self.region.start <= region.start && region.end <= self.region.end {
            self.reserved.get_mut().alloc(region)
        } else {
            Err(region)
        }
    }

    #[unsafe(link_section = ".init.text")]
    pub fn alloc(&self, size: NonZeroUsize, align: NonZeroUsize) -> Option<Region> {
        let allocation = {
            let reserved = self.reserved.borrow();
            self.free_iter(&reserved).find_map(|free| {
                let region = Region::from_size(free.start.align_up(align), size);
                (region.end <= free.end && reserved.is_allocable(region)).then_some(region)
            })
        };
        if let Some(allocation) = allocation
            && self.reserved.borrow_mut().alloc(allocation).is_ok()
        {
            Some(allocation)
        } else {
            None
        }
    }

    #[unsafe(link_section = ".init.text")]
    fn free_iter<'a>(&self, reserved: &'a RegionSet<8>) -> FreeRegionIterator<'a> {
        FreeRegionIterator {
            region: self.region,
            cursor: self.region.start,
            reserved: reserved.iter(),
            is_end: false,
        }
    }
}

/// Sorted, coalescing set of reserved regions.
pub struct RegionSet<const N: usize> {
    regions: ArrayVec<Region, N>,
}

#[unsafe(link_section = ".init.text")]
fn overlap(region: Region, other: Option<Region>) -> bool {
    other.is_some_and(|other| region.overlap(other))
}

impl<const N: usize> RegionSet<N> {
    #[unsafe(link_section = ".init.text")]
    pub const fn new() -> Self {
        Self {
            regions: ArrayVec::new_const(),
        }
    }

    #[unsafe(link_section = ".init.text")]
    pub fn as_slice(&self) -> &[Region] {
        &self.regions
    }

    #[unsafe(link_section = ".init.text")]
    pub fn is_allocable(&self, region: Region) -> bool {
        if region.is_empty() {
            return true;
        }

        let (_, left, right) = self.neighbors(region);
        !overlap(region, left) && !overlap(region, right)
    }

    #[unsafe(link_section = ".init.text")]
    pub fn alloc(&mut self, region: Region) -> Result<(), Region> {
        if region.is_empty() {
            return Ok(());
        }

        let (index, left, right) = self.neighbors(region);
        if overlap(region, left) || overlap(region, right) {
            return Err(region);
        }
        if let Some(left) = left
            && let Some(right) = right
            && left.end == region.start
            && region.end == right.start
        {
            self.regions[index - 1] = Region::new(left.start, right.end).unwrap();
            self.regions.remove(index);
            return Ok(());
        }
        if let Some(left) = left
            && left.end == region.start
        {
            self.regions[index - 1] = Region::new(left.start, region.end).unwrap();
            return Ok(());
        }
        if let Some(right) = right
            && region.end == right.start
        {
            self.regions[index] = Region::new(region.start, right.end).unwrap();
            return Ok(());
        }

        self.regions
            .try_insert(index, region)
            .map_err(|e| e.element())
    }

    #[unsafe(link_section = ".init.text")]
    pub fn iter(&self) -> Iter<'_, Region> {
        self.regions.iter()
    }

    #[unsafe(link_section = ".init.text")]
    fn neighbors(&self, region: Region) -> (usize, Option<Region>, Option<Region>) {
        let index = self
            .regions
            .iter()
            .position(|now| region.start < now.start)
            .unwrap_or(self.regions.len());
        let left = index
            .checked_sub(1)
            .and_then(|i| self.regions.get(i))
            .copied();
        let right = self.regions.get(index).copied();
        (index, left, right)
    }
}

/// Iterator over the free gaps in one [`Memory`] range.
pub struct FreeRegionIterator<'a> {
    region: Region,
    cursor: Pa,
    reserved: Iter<'a, Region>,
    is_end: bool,
}

impl<'a> Iterator for FreeRegionIterator<'a> {
    type Item = Region;

    #[unsafe(link_section = ".init.text")]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.is_end {
                return None;
            }
            match self.reserved.next() {
                Some(reserved) => {
                    let region = Region::new(self.cursor, reserved.start);
                    self.cursor = reserved.end;
                    if let Some(region) = region {
                        return Some(region);
                    }
                }
                None => {
                    self.is_end = true;
                    let region = Region::new(self.cursor, self.region.end);
                    if let Some(region) = region {
                        return Some(region);
                    }
                }
            }
        }
    }
}
