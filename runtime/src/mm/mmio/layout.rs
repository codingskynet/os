//! Checked page layout for a permanent MMIO mapping.
use core::num::NonZeroUsize;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    PhysicalOverflow,
    VirtualSpaceExhausted,
    AlreadyMapped,
}

pub struct Layout {
    pub physical_start: usize,
    pub virtual_start: usize,
    pub address: usize,
    pub pages: usize,
    pub next: Option<usize>,
}
impl Layout {
    pub fn new(
        physical: usize,
        size: NonZeroUsize,
        next: Option<usize>,
        last: usize,
        page_size: usize,
    ) -> Result<Self, Error> {
        debug_assert!(page_size.is_power_of_two());
        let mask = page_size - 1;
        let physical_last = physical
            .checked_add(size.get() - 1)
            .ok_or(Error::PhysicalOverflow)?;
        let physical_start = physical & !mask;
        let pages = ((physical_last & !mask) - physical_start) / page_size + 1;
        let virtual_start = next.ok_or(Error::VirtualSpaceExhausted)?;
        debug_assert_eq!(virtual_start & mask, 0);
        let span_minus_one = (pages - 1)
            .checked_mul(page_size)
            .and_then(|span| span.checked_add(mask))
            .ok_or(Error::VirtualSpaceExhausted)?;
        let virtual_last = virtual_start
            .checked_add(span_minus_one)
            .filter(|end| *end <= last)
            .ok_or(Error::VirtualSpaceExhausted)?;
        Ok(Self {
            physical_start,
            virtual_start,
            address: virtual_start + (physical & mask),
            pages,
            next: virtual_last.checked_add(1).filter(|next| *next <= last),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn size(n: usize) -> NonZeroUsize {
        NonZeroUsize::new(n).unwrap()
    }
    #[test]
    fn unaligned_resource_preserves_offset_and_covers_both_pages() {
        let layout = Layout::new(0x1234, size(0x1000), Some(0xf0000000), 0xffffffff, 4096).unwrap();
        assert_eq!(
            (
                layout.physical_start,
                layout.address,
                layout.pages,
                layout.next
            ),
            (0x1000, 0xf0000234, 2, Some(0xf0002000))
        );
    }
    #[test]
    fn last_virtual_page_can_be_allocated_without_wrapping() {
        let start = usize::MAX & !4095;
        let layout = Layout::new(0x8000, size(4096), Some(start), usize::MAX, 4096).unwrap();
        assert_eq!(layout.pages, 1);
        assert_eq!(layout.next, None);
        assert!(matches!(
            Layout::new(0x9000, size(1), layout.next, usize::MAX, 4096),
            Err(Error::VirtualSpaceExhausted)
        ));
    }
    #[test]
    fn exact_page_does_not_allocate_an_extra_page() {
        let layout = Layout::new(0x8000, size(4096), Some(0x10000), 0x10fff, 4096).unwrap();
        assert_eq!(layout.pages, 1);
        assert_eq!(layout.next, None);
    }
    #[test]
    fn overflowing_physical_resource_is_rejected() {
        assert!(matches!(
            Layout::new(usize::MAX, size(2), Some(0), 0xffff, 4096),
            Err(Error::PhysicalOverflow)
        ));
    }
    #[test]
    fn last_physical_page_is_representable() {
        let layout = Layout::new(usize::MAX - 7, size(8), Some(0x10000), 0x1ffff, 4096).unwrap();
        assert_eq!(layout.pages, 1);
        assert_eq!(layout.address, 0x10ff8);
    }
    #[test]
    fn insufficient_virtual_window_is_rejected() {
        assert!(matches!(
            Layout::new(0xfff, size(2), Some(0x10000), 0x10fff, 4096),
            Err(Error::VirtualSpaceExhausted)
        ));
    }
}
