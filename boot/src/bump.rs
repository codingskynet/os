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
use runtime::arch::consts::PAGE_SIZE;
use runtime::dev::dt::Fdt;
use runtime::dev::dt::memory::{MemoryIter, reservations};
use runtime::kernel::sync::SpinLock;
use runtime::mm::addr::Pa;
use runtime::mm::region::Region;
use runtime::{arch, debug};

#[unsafe(link_section = ".init.bss")]
pub static BUMP_ALLOCATOR: SpinLock<BumpAllocator> = SpinLock::new(BumpAllocator::EMPTY);

/// Round the start up and the exclusive end down to page boundaries.
/// Equal rounded bounds yield an empty region. Returns `None` if the
/// rounded start exceeds the rounded end or overflows the address space.
#[unsafe(link_section = ".init.text")]
fn whole_pages(region: Region) -> Option<Region> {
    let start = region
        .start
        .as_raw()
        .checked_next_multiple_of(PAGE_SIZE.get())?;
    Region::new(Pa::new(start), region.end.align_down(PAGE_SIZE))
}

/// Firmware RAM ranges and reservations, available before allocator initialization.
pub(crate) struct MemoryMap<'a> {
    fdt: &'a Fdt,
}

impl<'a> MemoryMap<'a> {
    #[unsafe(link_section = ".init.text")]
    pub(crate) fn new(fdt: &'a Fdt) -> Self {
        Self { fdt }
    }

    #[unsafe(link_section = ".init.text")]
    fn ram_regions(&self) -> impl Iterator<Item = Region> {
        MemoryIter::new(self.fdt).map(|reg| {
            let (addr, size) = reg.expect("unsupported RAM range");
            Region::from_size(Pa::new(addr), size)
        })
    }

    /// Yield representable reservations, rounding no-map ranges out to page boundaries.
    #[unsafe(link_section = ".init.text")]
    fn reservations(&self) -> impl Iterator<Item = (Region, bool)> {
        reservations(self.fdt).filter_map(|reservation| {
            let start = Pa::try_from(reservation.base).ok()?;
            let end = reservation
                .base
                .checked_add(reservation.size)
                .expect("FDT range overflow");
            let end = usize::try_from(end).unwrap_or(usize::MAX);
            let region = if reservation.no_map {
                Region::new(
                    start.align_down(PAGE_SIZE),
                    Pa::new(
                        end.checked_next_multiple_of(PAGE_SIZE.get())
                            .unwrap_or(usize::MAX),
                    ),
                )
            } else {
                Region::new(start, Pa::new(end))
            }?;
            Some((region, reservation.no_map))
        })
    }

    /// Visit complete RAM pages that can be included in ordinary RAM mappings.
    #[unsafe(link_section = ".init.text")]
    pub(crate) fn for_each_mappable_ram(&self, mut visit: impl FnMut(Region)) {
        let mut excluded = RegionSet::new();
        for (region, no_map) in self.reservations() {
            if no_map {
                excluded.insert(region);
            }
        }
        for pages in self.ram_regions().filter_map(whole_pages) {
            for gap in excluded.gaps(pages) {
                visit(gap);
            }
        }
    }
}

pub const MAX_RAM_BANKS: usize = 4;

/// Allocator over all discovered physical memory ranges.
pub struct BumpAllocator {
    banks: ArrayVec<RamBank, MAX_RAM_BANKS>,
}

unsafe impl Allocator for BumpAllocator {
    #[unsafe(link_section = ".init.text")]
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        for bank in &self.banks {
            if let Ok(allocation) = bank.allocate(layout) {
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
    const EMPTY: Self = Self {
        banks: ArrayVec::new_const(),
    };

    #[unsafe(link_section = ".init.text")]
    pub fn init(&mut self, fdt: &Fdt) {
        let map = MemoryMap::new(fdt);
        self.banks.clear();
        for region in map.ram_regions() {
            debug!("bump: register memory {region:?}");
            self.banks.push(RamBank::new(region));
        }
        for (region, _) in map.reservations() {
            self.reserve(region);
        }
        self.reserve(arch::region::kernel());
        self.reserve(Region::from_raw(
            fdt.as_ptr(),
            fdt.as_ptr().wrapping_add(fdt.total_size()),
        ));
    }

    #[unsafe(link_section = ".init.text")]
    fn reserve(&mut self, region: Region) {
        for bank in &mut self.banks {
            if let Some(part) = Region::new(
                bank.region.start.max(region.start),
                bank.region.end.min(region.end),
            ) {
                bank.reserved.get_mut().insert(part);
            }
        }
    }

    #[unsafe(link_section = ".init.text")]
    pub fn banks(&self) -> &[RamBank] {
        &self.banks
    }
}

/// One physical memory range plus sub-ranges already reserved from it.
pub struct RamBank {
    region: Region,
    reserved: RefCell<RegionSet>,
}

unsafe impl Allocator for RamBank {
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

impl RamBank {
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
    pub fn reserved(&self) -> core::cell::Ref<'_, [Region]> {
        core::cell::Ref::map(self.reserved.borrow(), |set| set.regions.as_slice())
    }

    #[unsafe(link_section = ".init.text")]
    pub fn alloc(&self, size: NonZeroUsize, align: NonZeroUsize) -> Option<Region> {
        let allocation = {
            let reserved = self.reserved.borrow();
            reserved.gaps(whole_pages(self.region)?).find_map(|free| {
                let region = Region::from_size(free.start.align_up(align), size);
                (region.end <= free.end).then_some(region)
            })
        };
        if let Some(region) = allocation {
            self.reserved.borrow_mut().insert(region);
        }
        allocation
    }
}

/// Sorted union of fixed boot reservations and bump allocations.
struct RegionSet {
    regions: ArrayVec<Region, 32>,
}

impl RegionSet {
    #[unsafe(link_section = ".init.text")]
    const fn new() -> Self {
        Self {
            regions: ArrayVec::new_const(),
        }
    }

    #[unsafe(link_section = ".init.text")]
    fn insert(&mut self, mut region: Region) {
        if region.is_empty() {
            return;
        }
        let index = self.regions.partition_point(|r| r.end < region.start);
        while let Some(other) = self.regions.get(index) {
            if other.start > region.end {
                break;
            }
            region.start = region.start.min(other.start);
            region.end = region.end.max(other.end);
            self.regions.remove(index);
        }
        self.regions
            .try_insert(index, region)
            .expect("too many disjoint boot reservations");
    }

    #[unsafe(link_section = ".init.text")]
    fn gaps(&self, region: Region) -> FreeRegionIterator<'_> {
        FreeRegionIterator {
            region,
            cursor: region.start,
            reserved: self.regions.iter(),
        }
    }
}

/// Iterator over the free gaps in one [`RamBank`] range.
pub struct FreeRegionIterator<'a> {
    region: Region,
    cursor: Pa,
    reserved: Iter<'a, Region>,
}

impl<'a> Iterator for FreeRegionIterator<'a> {
    type Item = Region;

    #[unsafe(link_section = ".init.text")]
    fn next(&mut self) -> Option<Self::Item> {
        while self.cursor < self.region.end {
            let start = self.cursor;
            let end = if let Some(reserved) = self.reserved.next() {
                self.cursor = self.cursor.max(reserved.end);
                reserved.start.min(self.region.end)
            } else {
                self.cursor = self.region.end;
                self.region.end
            };
            if start < end {
                return Region::new(start, end);
            }
        }
        None
    }
}
