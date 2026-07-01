use core::iter::once;
use core::mem::{self, MaybeUninit};
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use core::slice::{self, Iter};

use arrayvec::ArrayVec;

use crate::arch::consts::*;
use crate::mm::addr::{Pa, Va};

pub struct PhysicalAllocator {
    memory: ArrayVec<Memory, 2>,
}

impl PhysicalAllocator {
    pub unsafe fn new(memory: Region) -> Result<Self, Error> {
        unsafe {
            let mut memory = ArrayVec::from_iter(once(Memory::new(memory)));

            let mut reserved: ArrayVec<Region, 32> = ArrayVec::new();
            reserved.push(Region::from_linker_symbols(&_stext, &_etext));
            reserved.push(Region::from_linker_symbols(&_rodata_start, &_rodata_end));
            reserved.push(Region::from_linker_symbols(&_data_start, &_data_end));
            reserved.push(Region::from_linker_symbols(&_bss_start, &_bss_end));
            // TODO: add reserve-memory from FDT
            reserved.sort_unstable();

            for r in reserved {
                for mem in memory.iter_mut() {
                    if mem.reserve(r).is_ok() {
                        break;
                    }
                }
            }

            Ok(Self { memory })
        }
    }

    pub fn alloc<T>(&mut self, init: impl FnOnce() -> T) -> Result<&'static mut T, Error> {
        let ptr = self.alloc_uninit()?;
        Ok(ptr.write(init()))
    }

    pub fn alloc_slice<T>(
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

    pub fn alloc_uninit<T>(&mut self) -> Result<&'static mut MaybeUninit<T>, Error> {
        let Some(size) = NonZeroUsize::new(mem::size_of::<T>()) else {
            return Ok(unsafe { &mut *NonNull::dangling().as_ptr() });
        };

        let pa = self.alloc_pa(
            size,
            // Safety: align_of guarantees nonzero.
            mem::align_of::<T>().try_into().unwrap(),
        )?;
        let ptr: *mut MaybeUninit<T> = pa.into_va().as_mut_ptr();

        Ok(unsafe { &mut *ptr })
    }

    pub fn alloc_slice_uninit<T>(
        &mut self,
        len: usize,
    ) -> Result<&'static mut [MaybeUninit<T>], Error> {
        let Some(size) = NonZeroUsize::new(mem::size_of::<T>() * len) else {
            return Ok(unsafe { slice::from_raw_parts_mut(NonNull::dangling().as_ptr(), len) });
        };

        let pa = self.alloc_pa(
            size,
            // Safety: align_of guarantees nonzero.
            mem::align_of::<T>().try_into().unwrap(),
        )?;
        let ptr: *mut MaybeUninit<T> = pa.into_va().as_mut_ptr();

        Ok(unsafe { slice::from_raw_parts_mut(ptr, len) })
    }

    pub fn alloc_pa(&mut self, size: NonZeroUsize, align: NonZeroUsize) -> Result<Pa, Error> {
        for mem in self.memory.iter_mut() {
            if let Ok(region) = mem.alloc(size, align) {
                return Ok(region.start);
            }
        }

        Err(Error::OutOfMemory)
    }

    pub fn reserved_iter(&self) -> impl Iterator<Item = Region> + '_ {
        self.memory
            .iter()
            .flat_map(|memory| memory.reserved.iter().copied())
    }
}

pub struct Memory {
    region: Region,
    reserved: RegionSet<8>,
}

impl Memory {
    pub fn new(region: Region) -> Self {
        Self {
            region,
            reserved: RegionSet::new(),
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Region {
    pub start: Pa,
    pub end: Pa,
}

impl Region {
    fn new(start: Pa, end: Pa) -> Option<Self> {
        if start < end {
            Some(Self { start, end })
        } else {
            None
        }
    }

    pub fn from_size(addr: Pa, size: NonZeroUsize) -> Option<Self> {
        let end = addr.checked_offset(size.into())?;
        Some(Region { start: addr, end })
    }

    fn from_linker_symbols(start: *const u8, end: *const u8) -> Self {
        Self {
            start: Va::new(start.addr()).into_pa(),
            end: Va::new(end.addr()).into_pa(),
        }
    }

    fn is_collide(&self, other: Region) -> bool {
        self.start < other.end && other.start < self.end
    }
}

pub struct RegionSet<const N: usize> {
    regions: ArrayVec<Region, N>,
}

impl<const N: usize> RegionSet<N> {
    pub const fn new() -> Self {
        Self {
            regions: ArrayVec::new_const(),
        }
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

    pub fn is_allocable(&self, region: Region) -> bool {
        if region.start == region.end {
            return true;
        }

        let (_, left, right) = self.neighbors(region);
        !(left.is_some_and(|left| region.is_collide(left))
            || right.is_some_and(|right| region.is_collide(right)))
    }

    pub fn alloc(&mut self, region: Region) -> Result<(), Region> {
        if region.start == region.end {
            return Ok(());
        }

        let (index, left, right) = self.neighbors(region);
        if left.is_some_and(|left| region.is_collide(left))
            || right.is_some_and(|right| region.is_collide(right))
        {
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
            .map_err(|e| e.element())?;

        Ok(())
    }

    pub fn iter(&self) -> Iter<'_, Region> {
        self.regions.iter()
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

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Out of memory")]
    OutOfMemory,
}
