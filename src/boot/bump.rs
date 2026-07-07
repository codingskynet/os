use core::mem::{self, MaybeUninit};
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use core::slice::{self, Iter};

use arrayvec::ArrayVec;

use crate::arch::{self};
use crate::dev::dt::Fdt;
use crate::dev::dt::memory::MemoryIter;
use crate::mm::addr::Pa;
use crate::mm::region::Region;
use crate::{debug, printlnk};

pub trait Alloc {
    fn alloc_raw(&mut self, size: NonZeroUsize, align: NonZeroUsize) -> Result<Pa, Error>;

    fn alloc_slice<T>(
        &mut self,
        len: usize,
        init: impl Fn(usize) -> T,
    ) -> Result<&'static mut [T], Error> {
        let data = self.alloc_slice_uninit(len)?;

        for (i, elem) in data[..].iter_mut().enumerate() {
            elem.write(init(i));
        }

        Ok(unsafe { mem::transmute::<&mut [core::mem::MaybeUninit<T>], &mut [T]>(data) })
    }

    fn alloc_uninit<T>(&mut self) -> Result<&'static mut MaybeUninit<T>, Error> {
        let Some(size) = NonZeroUsize::new(mem::size_of::<T>()) else {
            return Ok(unsafe { &mut *NonNull::dangling().as_ptr() });
        };

        let pa = self.alloc_raw(
            size,
            // Safety: align_of guarantees nonzero.
            mem::align_of::<T>().try_into().unwrap(),
        )?;
        let ptr: *mut MaybeUninit<T> = pa.into_va().as_mut_ptr();

        Ok(unsafe { &mut *ptr })
    }

    fn alloc_slice_uninit<T>(
        &mut self,
        len: usize,
    ) -> Result<&'static mut [MaybeUninit<T>], Error> {
        let Some(size) = NonZeroUsize::new(mem::size_of::<T>() * len) else {
            return Ok(unsafe { slice::from_raw_parts_mut(NonNull::dangling().as_ptr(), len) });
        };

        let pa = self.alloc_raw(
            size,
            // Safety: align_of guarantees nonzero.
            mem::align_of::<T>().try_into().unwrap(),
        )?;
        let ptr: *mut MaybeUninit<T> = pa.into_va().as_mut_ptr();

        Ok(unsafe { slice::from_raw_parts_mut(ptr, len) })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Out of memory")]
    OutOfMemory,
}

pub struct BumpAllocator {
    memories: ArrayVec<Memory, 4>,
}

impl Alloc for BumpAllocator {
    fn alloc_raw(&mut self, size: NonZeroUsize, align: NonZeroUsize) -> Result<Pa, Error> {
        for mem in self.memories.iter_mut() {
            if let Ok(pa) = mem.alloc_raw(size, align) {
                return Ok(pa);
            }
        }

        Err(Error::OutOfMemory)
    }
}

impl BumpAllocator {
    pub fn new(fdt: &Fdt) -> Result<Self, Error> {
        let regs = MemoryIter::new(fdt);
        let mut memory = ArrayVec::from_iter(
            regs.into_iter()
                .map(|(addr, size)| {
                    let addr = Pa::new(addr as usize);
                    (addr, addr.checked_offset(size.get()).unwrap())
                })
                .inspect(|(from, to)| debug!("bump: register memory {from} ~ {to}"))
                .map(|(from, to)| Region::new(from, to).unwrap())
                .map(Memory::new),
        );

        let mut reserved: ArrayVec<Region, 32> = ArrayVec::new();
        reserved.push(arch::region::kernel());
        reserved.push(Region::from_raw(
            fdt.as_ptr(),
            fdt.as_ptr().wrapping_add(fdt.total_size()),
        ));
        // TODO: add reserve-memory from FDT
        reserved.sort_unstable();

        for r in reserved {
            debug!("bump: reserve {r:?}");
            let mut reserved = false;
            for mem in memory.iter_mut() {
                if mem.reserve(r).is_ok() {
                    reserved = true;
                    break;
                }
            }
            if !reserved {
                printlnk!("bump: failed to reserve region: {r:?}");
            }
        }

        Ok(Self { memories: memory })
    }

    pub fn memories_mut(&mut self) -> &mut [Memory] {
        &mut self.memories
    }
}

pub struct Memory {
    region: Region,
    reserved: RegionSet<8>,
}

impl Alloc for Memory {
    fn alloc_raw(&mut self, size: NonZeroUsize, align: NonZeroUsize) -> Result<Pa, Error> {
        match self.alloc(size, align) {
            Ok(region) => Ok(region.start),
            Err(_) => Err(Error::OutOfMemory),
        }
    }
}

impl Memory {
    pub fn new(region: Region) -> Self {
        Self {
            region,
            reserved: RegionSet::new(),
        }
    }

    pub fn region(&self) -> Region {
        self.region
    }

    pub fn reserved(&self) -> &[Region] {
        self.reserved.as_slice()
    }

    pub fn reserve(&mut self, region: Region) -> Result<(), Region> {
        if self.region.start <= region.start && region.end <= self.region.end {
            self.reserved.alloc(region)
        } else {
            Err(region)
        }
    }

    pub fn alloc(&mut self, size: NonZeroUsize, align: NonZeroUsize) -> Result<Region, ()> {
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
            Ok(allocation)
        } else {
            Err(())
        }
    }

    fn free_iter(&self) -> FreeRegionIterator<'_> {
        FreeRegionIterator {
            region: self.region,
            cursor: self.region.start,
            reserved: self.reserved.iter(),
            is_end: false,
        }
    }
}

pub struct RegionSet<const N: usize> {
    regions: ArrayVec<Region, N>,
}

fn overlap(region: Region, other: Option<Region>) -> bool {
    other.is_some_and(|other| region.overlap(other))
}

impl<const N: usize> RegionSet<N> {
    pub const fn new() -> Self {
        Self {
            regions: ArrayVec::new_const(),
        }
    }

    pub fn as_slice(&self) -> &[Region] {
        &self.regions
    }

    pub fn is_allocable(&self, region: Region) -> bool {
        if region.is_empty() {
            return true;
        }

        let (_, left, right) = self.neighbors(region);
        !overlap(region, left) && !overlap(region, right)
    }

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

    pub fn iter(&self) -> Iter<'_, Region> {
        self.regions.iter()
    }

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

pub struct FreeRegionIterator<'a> {
    region: Region,
    cursor: Pa,
    reserved: Iter<'a, Region>,
    is_end: bool,
}

impl<'a> Iterator for FreeRegionIterator<'a> {
    type Item = Region;

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
