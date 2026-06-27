use core::ffi::CStr;
use core::{slice, str};

use winnow::binary::be_u32;
use winnow::error::{ContextError, ErrMode};
use winnow::token::{take, take_until};
use winnow::{ModalResult, Parser, Stateful};

// Safety: the caller must ensure `buf.offset(offset)..buf.offset(offset + 4)` is readable.
unsafe fn be32(buf: *const u8, offset: isize) -> u32 {
    unsafe {
        let buf = buf.offset(offset) as *const u32;
        u32::from_be(buf.read_unaligned())
    }
}

// https://github.com/devicetree-org/devicetree-specification/blob/main/source/chapter5-flattened-format.rst
pub struct FdtHeader {
    ptr: *const u8,
}

impl FdtHeader {
    pub fn new(ptr: *const u8) -> Self {
        Self { ptr }
    }

    /// Magic number (`0xd00dfeed`) at offset 0.
    ///
    /// # Safety
    ///
    /// `self.ptr` must point to a readable FDT header.
    pub unsafe fn magic(&self) -> u32 {
        unsafe { be32(self.ptr, 0) }
    }

    /// Total size of the FDT blob in bytes.
    ///
    /// # Safety
    ///
    /// `self.ptr` must point to a readable FDT header.
    pub unsafe fn total_size(&self) -> u32 {
        unsafe { be32(self.ptr, 4) }
    }

    /// Offset of the structure block from the start of the FDT blob.
    ///
    /// # Safety
    ///
    /// `self.ptr` must point to a readable FDT header.
    pub unsafe fn off_dt_struct(&self) -> u32 {
        unsafe { be32(self.ptr, 8) }
    }

    /// Offset of the strings block from the start of the FDT blob.
    ///
    /// # Safety
    ///
    /// `self.ptr` must point to a readable FDT header.
    pub unsafe fn off_dt_strings(&self) -> u32 {
        unsafe { be32(self.ptr, 12) }
    }

    /// Size of the strings block in bytes.
    ///
    /// # Safety
    ///
    /// `self.ptr` must point to a readable FDT header.
    pub unsafe fn size_dt_strings(&self) -> u32 {
        unsafe { be32(self.ptr, 32) }
    }

    /// Size of the structure block in bytes.
    ///
    /// # Safety
    ///
    /// `self.ptr` must point to a readable FDT header.
    pub unsafe fn size_dt_struct(&self) -> u32 {
        unsafe { be32(self.ptr, 36) }
    }
}

// TODO: Since it uses **raw pointer**, so unsafe. Need to split unsafe FDT(no allocation) and safe FDT(allocation)
pub struct Fdt {
    ptr: *const u8,
    header: FdtHeader,
}

impl Fdt {
    pub fn new(ptr: usize) -> Self {
        let ptr = ptr as *const u8;
        let header = FdtHeader::new(ptr);
        Self { ptr, header }
    }

    /// Navigate to the node identified by an absolute device-tree path such as
    /// `/soc/serial@10000000`.
    ///
    /// Returns a walker positioned at that node, or [`None`] if the path cannot
    /// be resolved.
    ///
    /// # Safety
    ///
    /// See [`Fdt::query`].
    pub unsafe fn lookup(&self, path: &str) -> FdtWalker<'_> {
        let mut walker = unsafe { self.query() };

        let trimmed = path.strip_prefix('/').unwrap_or(path);
        if trimmed.is_empty() {
            return walker;
        }
        for component in trimmed.split('/') {
            walker = walker.at(component);
        }
        walker
    }

    /// Resolve `#address-cells` and `#size-cells` for a device at `path`.
    ///
    /// Walks up the tree from `path` to find the nearest ancestor (or self) that
    /// defines these properties.  Returns the **reg specifier** per the Devicetree
    /// spec v0.4 §2.3.5: `(#address-cells, #size-cells)` with defaults `(2, 1)`.
    ///
    /// # Safety
    ///
    /// See [`Fdt::query`].
    pub unsafe fn reg_cells(&self, path: &str) -> (u32, u32) {
        // We walk the parent chain by chopping off path components.
        let mut address_cells = None;
        let mut size_cells = None;
        let mut p: &str = path;

        loop {
            let walker = unsafe { self.lookup(p) };
            for (name, value) in walker.props() {
                match name {
                    "#address-cells" if address_cells.is_none() => {
                        address_cells = value.try_into().ok().map(u32::from_be_bytes);
                    }
                    "#size-cells" if size_cells.is_none() => {
                        size_cells = value.try_into().ok().map(u32::from_be_bytes);
                    }
                    _ => {}
                }
            }
            if address_cells.is_some() && size_cells.is_some() {
                break;
            }
            // Move to parent path
            p = match p.rsplit_once('/') {
                Some(("", _)) => "/", // reached root
                Some((parent, _)) => parent,
                None => break,
            };
        }

        (address_cells.unwrap_or(2), size_cells.unwrap_or(1))
    }

    /// Returns a walker over the FDT structure block.
    ///
    /// # Safety
    ///
    /// The pointer passed to [`Fdt::new`] must point to a complete, readable FDT
    /// blob whose header offsets and sizes describe memory inside that blob.
    pub unsafe fn query(&self) -> FdtWalker<'_> {
        unsafe {
            let len = self.header.size_dt_struct() as usize;
            let struct_ptr = self.ptr.offset(self.header.off_dt_struct() as isize);
            let struct_slice = slice::from_raw_parts(struct_ptr, len);

            let strings_ptr = self.ptr.offset(self.header.off_dt_strings() as isize);
            let strings_slice =
                slice::from_raw_parts(strings_ptr, self.header.size_dt_strings() as usize);

            let mut walker = FdtWalker {
                stream: Stateful {
                    input: struct_slice,
                    state: FdtWalkerState {
                        strings_slice,
                        len,
                        depth: 0,
                    },
                },
                end_depth: 0,
                is_end: false,
            };
            let _ = walker.next(); // entering root node
            walker
        }
    }
}

pub struct FdtWalker<'a> {
    stream: Stateful<&'a [u8], FdtWalkerState<'a>>,
    end_depth: usize,
    is_end: bool,
}

impl<'a> FdtWalker<'a> {
    pub fn depth(&self) -> usize {
        self.stream.state.depth
    }

    /// Move to the next node with this local name inside the current walk range.
    ///
    /// Device-tree node names are local path components, so descend with chained
    /// calls such as `query().at("soc").at("serial@10000000")`.
    pub fn at(mut self, name: &str) -> FdtWalker<'a> {
        let start_depth = self.depth();
        while let Some(token) = self.next() {
            if let FdtToken::Node(n) = token
                && n == name
                && self.depth() == start_depth + 1
            {
                break;
            }
        }
        self.end_depth = self.depth();
        self
    }

    pub fn props(self) -> FdtPropWalker<'a> {
        FdtPropWalker(self)
    }

    /// Returns the value of the first direct property with `name` at the current
    /// node, or [`None`] if no such property exists.
    ///
    /// Only properties of the **current** node are searched; child node data is
    /// not traversed.
    pub fn prop(mut self, name: &str) -> Option<&'a [u8]> {
        let mut depth = 0;
        for token in self.by_ref() {
            match token {
                FdtToken::Node(_) => depth += 1,
                FdtToken::NodeEnd => {
                    if depth == 0 {
                        return None;
                    } else {
                        depth -= 1;
                    }
                }
                FdtToken::Prop { name: n, value } if n == name => return Some(value),
                _ => continue,
            }
        }
        None
    }
}

#[derive(Debug)]
struct FdtWalkerState<'a> {
    strings_slice: &'a [u8],
    len: usize,
    depth: usize,
}

const FDT_BEGIN_NODE: u32 = 1;
const FDT_END_NODE: u32 = 2;
const FDT_PROP: u32 = 3;
const FDT_NOP: u32 = 4;
const FDT_END: u32 = 9;

#[derive(Debug, PartialEq, Eq)]
pub enum FdtToken<'a> {
    Node(&'a str),
    NodeEnd,
    Prop { name: &'a str, value: &'a [u8] },
}

impl<'a> Iterator for FdtWalker<'a> {
    type Item = FdtToken<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        fn resolve_name<'a>(
            input: &mut Stateful<&'a [u8], FdtWalkerState<'a>>,
            nameoff: u32,
        ) -> ModalResult<&'a str> {
            unsafe {
                let strings = input.state.strings_slice;
                if nameoff as usize >= strings.len() {
                    return Err(ErrMode::Cut(ContextError::new()));
                }

                CStr::from_ptr(strings.as_ptr().offset(nameoff as isize))
                    .to_str()
                    .map_err(|_| ErrMode::Cut(ContextError::new()))
            }
        }

        fn align4(input: &mut Stateful<&[u8], FdtWalkerState>) -> ModalResult<()> {
            let position = input.state.len - input.input.len();
            let pad = (4 - (position % 4)) % 4;
            take(pad).void().parse_next(input)
        }

        fn parse_cstr<'a>(input: &mut Stateful<&'a [u8], FdtWalkerState>) -> ModalResult<&'a str> {
            let str = take_until(0.., 0).parse_next(input)?;
            let _ = take(1_usize).parse_next(input)?;
            str::from_utf8(str).map_err(|_| ErrMode::Cut(ContextError::new()))
        }

        fn parse_event<'a>(
            input: &mut Stateful<&'a [u8], FdtWalkerState<'a>>,
        ) -> ModalResult<Option<FdtToken<'a>>> {
            loop {
                let token = be_u32.parse_next(input)?;
                match token {
                    FDT_BEGIN_NODE => {
                        input.state.depth += 1;
                        let name = parse_cstr.parse_next(input)?;
                        align4(input)?;
                        return Ok(Some(FdtToken::Node(name)));
                    }
                    FDT_END_NODE => {
                        if input.state.depth == 0 {
                            return Err(ErrMode::Cut(ContextError::new()));
                        }
                        input.state.depth -= 1;
                        return Ok(Some(FdtToken::NodeEnd));
                    }
                    FDT_PROP => {
                        let len = be_u32.parse_next(input)?;
                        let nameoff = be_u32.parse_next(input)?;
                        let value = take(len).parse_next(input)?;
                        align4(input)?;
                        return Ok(Some(FdtToken::Prop {
                            name: resolve_name(input, nameoff)?,
                            value,
                        }));
                    }
                    FDT_NOP => {}
                    FDT_END => return Ok(None),
                    _ => {
                        return Err(ErrMode::Cut(ContextError::new()));
                    }
                }
            }
        }

        if self.is_end {
            return None;
        }

        let item = parse_event(&mut self.stream).unwrap();
        if item.is_none() || self.depth() < self.end_depth {
            self.is_end = true;
        }
        item
    }
}

pub struct FdtPropWalker<'a>(FdtWalker<'a>);

impl<'a> Iterator for FdtPropWalker<'a> {
    type Item = (&'a str, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        let mut depth = 0;
        loop {
            let item = self.0.next()?;
            match item {
                FdtToken::Node(_) => depth += 1,
                FdtToken::NodeEnd => depth -= 1,
                FdtToken::Prop { name, value } if depth == 0 => return Some((name, value)),
                _ => {}
            }
        }
    }
}

/// Parse a big-endian cell value of `bytes` width (1 or 2 cells = 4 or 8 bytes).
///
/// Returns `u64` zero-extended.
fn read_cell_be(buf: &[u8], bytes: usize) -> Option<u64> {
    if buf.len() < bytes {
        return None;
    }
    match bytes {
        4 => Some(u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as u64),
        8 => Some(u64::from_be_bytes([
            buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
        ])),
        _ => None,
    }
}

/// Iterator over `(address, size)` tuples decoded from a DTB `reg` property.
///
/// `address_cells` and `size_cells` are inherited from the parent node's
/// `#address-cells` and `#size-cells` properties (defaults: 2 and 1).
pub struct RegIter<'a> {
    data: &'a [u8],
    stride: usize,
    address_len: usize,
    size_len: usize,
}

impl<'a> RegIter<'a> {
    /// Create a new `RegIter`.
    ///
    /// * `reg` — raw bytes of the `reg` property.
    /// * `address_cells` — value of the parent's `#address-cells`.
    /// * `size_cells` — value of the parent's `#size-cells`.
    pub fn new(reg: &'a [u8], address_cells: u32, size_cells: u32) -> Self {
        let address_len = address_cells as usize * 4;
        let size_len = size_cells as usize * 4;
        Self {
            data: reg,
            stride: address_len + size_len,
            address_len,
            size_len,
        }
    }
}

impl<'a> Iterator for RegIter<'a> {
    /// `(address, size)` — size is `None` when `#size-cells = 0`.
    type Item = (u64, Option<u64>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.data.is_empty() || self.data.len() < self.address_len {
            return None;
        }
        let address = read_cell_be(self.data, self.address_len)?;
        let (size, consumed) = if self.size_len > 0 {
            (
                Some(read_cell_be(&self.data[self.address_len..], self.size_len)?),
                self.stride,
            )
        } else {
            (None, self.address_len)
        };
        self.data = &self.data[consumed..];
        Some((address, size))
    }
}

/// Create a [`RegIter`] from a raw `reg` property value.
pub fn reg_iter(reg: &[u8], address_cells: u32, size_cells: u32) -> RegIter<'_> {
    RegIter::new(reg, address_cells, size_cells)
}

#[cfg(test)]
mod tests {
    use std::vec::Vec;

    use super::*;

    static QEMU_VIRT_DTB: &[u8] = include_bytes!("test_data/qemu_virt.dtb");

    #[derive(Debug, PartialEq, Eq)]
    struct Prop<'a> {
        name: &'a str,
        value: &'a [u8],
    }

    fn qemu_fdt() -> Fdt {
        Fdt::new(QEMU_VIRT_DTB.as_ptr() as usize)
    }

    fn query(fdt: &Fdt) -> FdtWalker<'_> {
        unsafe { fdt.query() }
    }

    fn collect_props<'a>(walker: FdtWalker<'a>) -> Vec<Prop<'a>> {
        walker
            .filter_map(|token| match token {
                FdtToken::Prop { name, value } => Some(Prop { name, value }),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn qemu_virt_header_matches_binary() {
        let fdt = qemu_fdt();
        let header = unsafe {
            [
                fdt.header.magic(),
                fdt.header.total_size(),
                fdt.header.off_dt_struct(),
                fdt.header.off_dt_strings(),
                fdt.header.size_dt_strings(),
                fdt.header.size_dt_struct(),
            ]
        };

        assert_eq!(
            header,
            [
                0xd00d_feed,
                QEMU_VIRT_DTB.len() as u32,
                0x38,
                0x11b0,
                0x1f4,
                0x1178,
            ]
        );
    }

    #[test]
    fn nested_props_match_expected_values() {
        let fdt = qemu_fdt();

        assert_eq!(
            collect_props(query(&fdt).at("chosen")).as_slice(),
            &[
                Prop {
                    name: "stdout-path",
                    value: b"/soc/serial@10000000\0",
                },
                Prop {
                    name: "rng-seed",
                    value: &[
                        0xa0, 0xdf, 0x1d, 0xc8, 0x21, 0x8f, 0x10, 0x91, 0x3a, 0x38, 0xf7, 0xe2,
                        0x43, 0x58, 0x0b, 0xbd, 0x1e, 0xfb, 0x0b, 0xf0, 0x33, 0x84, 0xe0, 0x5f,
                        0x7c, 0x07, 0xae, 0xd3, 0x42, 0x2e, 0x13, 0x1d,
                    ],
                },
            ]
        );

        assert_eq!(
            collect_props(query(&fdt).at("cpus").at("cpu-map")).as_slice(),
            &[Prop {
                name: "cpu",
                value: &[0, 0, 0, 1],
            }]
        );
    }

    #[test]
    fn reg_iter_serial_10000000() {
        // serial@10000000 has reg = <0x0 0x10000000 0x0 0x100>
        // under soc where #address-cells=2, #size-cells=2
        let fdt = qemu_fdt();
        let reg = query(&fdt)
            .at("soc")
            .at("serial@10000000")
            .prop("reg")
            .expect("serial@10000000 should have a reg property");

        let mut it = super::reg_iter(reg, 2, 2);
        let (addr, size) = it.next().expect("should yield one reg tuple");
        assert_eq!(addr, 0x10000000);
        assert_eq!(size, Some(0x100));
        assert!(it.next().is_none(), "only one reg tuple expected");
    }

    #[test]
    fn reg_iter_flash_two_tuples() {
        // flash@20000000 has two reg tuples
        // #address-cells=2, #size-cells=2 from root
        let fdt = qemu_fdt();
        let reg = query(&fdt)
            .at("flash@20000000")
            .prop("reg")
            .expect("flash should have a reg property");

        let tuples: Vec<_> = super::reg_iter(reg, 2, 2).collect();
        assert_eq!(tuples.len(), 2);
        assert_eq!(tuples[0], (0x20000000, Some(0x02000000)));
        assert_eq!(tuples[1], (0x22000000, Some(0x02000000)));
    }

    #[test]
    fn reg_iter_4byte_address() {
        // Simulate a 32-bit address with #address-cells=1, #size-cells=1
        let reg = b"\x80\x00\x00\x00\x00\x10\x00\x00";
        let tuples: Vec<_> = super::reg_iter(reg, 1, 1).collect();
        assert_eq!(tuples.len(), 1);
        assert_eq!(tuples[0], (0x80000000, Some(0x100000)));
    }

    #[test]
    fn reg_iter_zero_size_cells() {
        // #size-cells=0: reg is just addresses, no sizes
        let reg = b"\x00\x00\x00\x00\x10\x00\x00\x00";
        let tuples: Vec<_> = super::reg_iter(reg, 2, 0).collect();
        assert_eq!(tuples.len(), 1);
        assert_eq!(tuples[0], (0x10000000, None));
    }

    #[test]
    fn reg_cells_serial_inherits_from_soc() {
        // serial@10000000 is under /soc, which sets #address-cells=2, #size-cells=2
        let fdt = qemu_fdt();
        let (ac, sc) = unsafe { fdt.reg_cells("/soc/serial@10000000") };
        assert_eq!(ac, 2);
        assert_eq!(sc, 2);
    }

    #[test]
    fn reg_cells_soc_explicit() {
        let fdt = qemu_fdt();
        let (ac, sc) = unsafe { fdt.reg_cells("/soc") };
        assert_eq!(ac, 2);
        assert_eq!(sc, 2);
    }

    #[test]
    fn reg_cells_root_defaults() {
        // root itself might not have #address-cells/#size-cells
        // (QEMU virt root doesn't set them explicitly)
        let fdt = qemu_fdt();
        let (ac, sc) = unsafe { fdt.reg_cells("/") };
        // QEMU virt root actually has #address-cells=2, #size-cells=2
        assert_eq!(ac, 2);
        assert_eq!(sc, 2);
    }
}
