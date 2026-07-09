//! Boot-time physical bump allocator.
//!
//! This allocator runs from `.init.*` code before the runtime allocator is
//! ready. It records firmware RAM ranges, reserves the kernel image and DTB,
//! and hands out aligned physical regions for page tables and page metadata.

use core::mem::{self, MaybeUninit};
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use core::slice::{self, Iter};

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

/// Minimal allocation interface available during boot.
pub trait Alloc {
    fn alloc_raw(&mut self, size: NonZeroUsize, align: NonZeroUsize) -> Option<Pa>;

    #[unsafe(link_section = ".init.text")]
    fn alloc_slice<T>(
        &mut self,
        len: usize,
        init: impl Fn(usize) -> T,
    ) -> Option<&'static mut [T]> {
        let data = self.alloc_slice_uninit(len)?;

        for (i, elem) in data[..].iter_mut().enumerate() {
            elem.write(init(i));
        }

        Some(unsafe { mem::transmute::<&mut [core::mem::MaybeUninit<T>], &mut [T]>(data) })
    }

    #[unsafe(link_section = ".init.text")]
    fn alloc_uninit<T>(&mut self) -> Option<&'static mut MaybeUninit<T>> {
        let Some(size) = NonZeroUsize::new(mem::size_of::<T>()) else {
            return Some(unsafe { &mut *NonNull::dangling().as_ptr() });
        };

        let pa = self.alloc_raw(
            size,
            // Safety: align_of guarantees nonzero.
            mem::align_of::<T>().try_into().unwrap(),
        )?;
        let ptr: *mut MaybeUninit<T> = pa.into_va().as_mut_ptr();

        Some(unsafe { &mut *ptr })
    }

    #[unsafe(link_section = ".init.text")]
    fn alloc_slice_uninit<T>(&mut self, len: usize) -> Option<&'static mut [MaybeUninit<T>]> {
        let Some(size) = NonZeroUsize::new(mem::size_of::<T>() * len) else {
            return Some(unsafe { slice::from_raw_parts_mut(NonNull::dangling().as_ptr(), len) });
        };

        let pa = self.alloc_raw(
            size,
            // Safety: align_of guarantees nonzero.
            mem::align_of::<T>().try_into().unwrap(),
        )?;
        let ptr: *mut MaybeUninit<T> = pa.into_va().as_mut_ptr();

        Some(unsafe { slice::from_raw_parts_mut(ptr, len) })
    }
}

/// Allocator over all discovered physical memory ranges.
pub struct BumpAllocator {
    memories: ArrayVec<Memory, 4>,
}

impl Alloc for BumpAllocator {
    #[unsafe(link_section = ".init.text")]
    fn alloc_raw(&mut self, size: NonZeroUsize, align: NonZeroUsize) -> Option<Pa> {
        for mem in self.memories.iter_mut() {
            if let Some(pa) = mem.alloc_raw(size, align) {
                return Some(pa);
            }
        }

        None
    }
}

impl BumpAllocator {
    #[unsafe(link_section = ".init.text")]
    pub const fn empty() -> Self {
        Self {
            memories: ArrayVec::new_const(),
        }
    }

    #[unsafe(link_section = ".init.text")]
    pub fn init(&mut self, fdt: &Fdt) {
        self.memories.clear();
        for (addr, size) in MemoryIter::new(fdt) {
            let region = Region::from_size(Pa::new(addr as usize), size).unwrap();
            debug!("bump: register memory {region:?}");
            self.memories.push(Memory::new(region));
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
        for mem in self.memories.iter_mut() {
            if mem.reserve(region).is_ok() {
                return;
            }
        }

        printlnk!("bump: failed to reserve region: {region:?}");
    }

    #[unsafe(link_section = ".init.text")]
    pub fn memories_mut(&mut self) -> &mut [Memory] {
        &mut self.memories
    }
}

/// One physical memory range plus sub-ranges already reserved from it.
pub struct Memory {
    region: Region,
    reserved: RegionSet<8>,
}

impl Alloc for Memory {
    #[unsafe(link_section = ".init.text")]
    fn alloc_raw(&mut self, size: NonZeroUsize, align: NonZeroUsize) -> Option<Pa> {
        self.alloc(size, align).map(|region| region.start)
    }
}

impl Memory {
    #[unsafe(link_section = ".init.text")]
    pub fn new(region: Region) -> Self {
        Self {
            region,
            reserved: RegionSet::new(),
        }
    }

    #[unsafe(link_section = ".init.text")]
    pub fn region(&self) -> Region {
        self.region
    }

    #[unsafe(link_section = ".init.text")]
    pub fn reserved(&self) -> &[Region] {
        self.reserved.as_slice()
    }

    #[unsafe(link_section = ".init.text")]
    pub fn reserve(&mut self, region: Region) -> Result<(), Region> {
        if self.region.start <= region.start && region.end <= self.region.end {
            self.reserved.alloc(region)
        } else {
            Err(region)
        }
    }

    #[unsafe(link_section = ".init.text")]
    pub fn alloc(&mut self, size: NonZeroUsize, align: NonZeroUsize) -> Option<Region> {
        let mut allocation = None;
        for free in self.free_iter() {
            let region = Region::from_size(free.start.align_up(align), size).unwrap();
            if region.end > free.end {
                continue;
            }
            if self.reserved.is_allocable(region) {
                allocation = Some(region);
                break;
            }
        }
        if let Some(allocation) = allocation
            && self.reserve(allocation).is_ok()
        {
            Some(allocation)
        } else {
            None
        }
    }

    #[unsafe(link_section = ".init.text")]
    fn free_iter(&self) -> FreeRegionIterator<'_> {
        FreeRegionIterator {
            region: self.region,
            cursor: self.region.start,
            reserved: self.reserved.iter(),
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
