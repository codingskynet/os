pub mod memory;
pub mod prop;

use core::ffi::CStr;
use core::{slice, str};

use arrayvec::ArrayVec;
pub use prop::reg::RegIter;
use winnow::binary::be_u32;
use winnow::error::{ContextError, ErrMode, FromExternalError};
use winnow::token::{take, take_until};
use winnow::{ModalResult, Parser, Stateful};

use crate::dev::dt::prop::Value;

const DEFAULT_CELLS: (u32, u32) = (2, 1);

// Safety: the caller must ensure `buf.offset(offset)..buf.offset(offset + 4)` is readable.
unsafe fn be32(buf: *const u8, offset: isize) -> u32 {
    unsafe {
        let buf = buf.offset(offset) as *const u32;
        u32::from_be(buf.read_unaligned())
    }
}

// https://github.com/devicetree-org/devicetree-specification/blob/main/source/chapter5-flattened-format.rst
#[derive(Clone, Copy)]
pub struct FdtHeader {
    ptr: *const u8,
}

impl FdtHeader {
    /// Create a view over an FDT header.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `ptr` points to a readable FDT header and
    /// that the backing memory is not mutated while this [`FdtHeader`] is used.
    pub unsafe fn new(ptr: *const u8) -> Self {
        Self { ptr }
    }

    pub fn magic(&self) -> u32 {
        unsafe { be32(self.ptr, 0) }
    }

    pub fn total_size(&self) -> u32 {
        unsafe { be32(self.ptr, 4) }
    }

    pub fn off_dt_struct(&self) -> u32 {
        unsafe { be32(self.ptr, 8) }
    }

    pub fn off_dt_strings(&self) -> u32 {
        unsafe { be32(self.ptr, 12) }
    }

    pub fn size_dt_strings(&self) -> u32 {
        unsafe { be32(self.ptr, 32) }
    }

    pub fn size_dt_struct(&self) -> u32 {
        unsafe { be32(self.ptr, 36) }
    }
}

// TODO: Since it uses **raw pointer**, so unsafe. Need to split unsafe FDT(no allocation) and safe FDT(allocation)
#[derive(Clone, Copy)]
pub struct Fdt {
    ptr: *const u8,
    header: FdtHeader,
}

impl Fdt {
    /// Create a flattened device-tree view from the start of an FDT blob.
    ///
    /// # Safety
    ///
    /// The caller must guarantee:
    ///
    /// * `ptr` points to a valid FDT header and backing blob.
    /// * The whole blob remains readable and is not mutated while any [`Fdt`]
    ///   or [`FdtWalker`] derived from it is used.
    /// * The header offsets and sizes describe readable regions inside the
    ///   same blob.
    pub unsafe fn new(ptr: *const u8) -> Option<Self> {
        let header = unsafe { FdtHeader::new(ptr) };
        if header.magic() != 0xd00d_feed {
            return None;
        }
        Some(Self { ptr, header })
    }

    /// Pointer to the start of the FDT blob.
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr
    }

    /// Total size in bytes of the FDT blob, as reported by its header.
    pub fn total_size(&self) -> usize {
        self.header.total_size() as usize
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
    pub fn lookup(&self, path: &str) -> FdtWalker<'_> {
        let mut walker = self.query();

        let trimmed = path.strip_prefix('/').unwrap_or(path);
        if trimmed.is_empty() {
            return walker;
        }
        for component in trimmed.split('/') {
            walker = walker.at(component);
        }
        walker
    }

    /// Start walking the FDT structure block from the root node.
    ///
    /// # Safety
    ///
    /// This method relies on the safety contract of [`Fdt::new`]: the FDT
    /// pointer must still refer to a readable, immutable blob whose structure
    /// and strings blocks are within bounds.
    pub fn query(&self) -> FdtWalker<'_> {
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
                        cells: ArrayVec::new(),
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

#[derive(Clone)]
pub struct FdtWalker<'a> {
    stream: Stateful<&'a [u8], FdtWalkerState<'a>>,
    end_depth: usize,
    is_end: bool,
}

impl<'a> FdtWalker<'a> {
    fn depth(&self) -> usize {
        self.stream.state.depth
    }

    /// Returns the `#address-cells` and `#size-cells` values used to decode a
    /// `reg` property on the current node.
    pub fn reg_cells(&self) -> (u32, u32) {
        let cells = &self.stream.state.cells;
        cells
            .len()
            .checked_sub(2)
            .and_then(|index| cells.get(index).copied())
            .unwrap_or(DEFAULT_CELLS)
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
    pub fn prop(mut self, name: &str) -> Option<Value<'a>> {
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

#[derive(Debug, Clone)]
struct FdtWalkerState<'a> {
    strings_slice: &'a [u8],
    len: usize,
    depth: usize,
    cells: ArrayVec<(u32, u32), 8>,
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
    Prop { name: &'a str, value: Value<'a> },
}

impl<'a> Iterator for FdtWalker<'a> {
    type Item = FdtToken<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        macro_rules! ctx_err {
            ($input:expr, $e:expr) => {{ $e.map_err(|e| ErrMode::Cut(ContextError::from_external_error($input, e))) }};
        }

        fn resolve_name<'a>(
            input: &mut Stateful<&'a [u8], FdtWalkerState<'a>>,
            nameoff: u32,
        ) -> ModalResult<&'a str> {
            let strings = input.state.strings_slice;
            if nameoff as usize >= strings.len() {
                return Err(ErrMode::Cut(ContextError::new()));
            }

            let cstr = ctx_err!(
                input,
                CStr::from_bytes_until_nul(&strings[nameoff as usize..])
            )?;
            ctx_err!(input, cstr.to_str())
        }

        fn align4(input: &mut Stateful<&[u8], FdtWalkerState>) -> ModalResult<()> {
            let position = input.state.len - input.input.len();
            let pad = (4 - (position % 4)) % 4;
            take(pad).void().parse_next(input)
        }

        fn parse_cstr<'a>(input: &mut Stateful<&'a [u8], FdtWalkerState>) -> ModalResult<&'a str> {
            let str = take_until(0.., 0).parse_next(input)?;
            let _ = take(1_usize).parse_next(input)?;
            ctx_err!(input, str::from_utf8(str))
        }

        fn parse_u32(
            input: &mut Stateful<&[u8], FdtWalkerState>,
            value: &[u8],
        ) -> ModalResult<u32> {
            ctx_err!(input, value.try_into().map(u32::from_be_bytes))
        }

        fn parse_props(
            input: &mut Stateful<&[u8], FdtWalkerState>,
            name: &str,
            value: &[u8],
        ) -> ModalResult<()> {
            match name {
                "#address-cells" => {
                    input.state.cells.last_mut().unwrap().0 = parse_u32(input, value)?;
                }
                "#size-cells" => {
                    input.state.cells.last_mut().unwrap().1 = parse_u32(input, value)?;
                }
                _ => {}
            }
            Ok(())
        }

        fn parse_event<'a>(
            input: &mut Stateful<&'a [u8], FdtWalkerState<'a>>,
        ) -> ModalResult<Option<FdtToken<'a>>> {
            loop {
                let token = be_u32.parse_next(input)?;
                match token {
                    FDT_BEGIN_NODE => {
                        input.state.cells.push(DEFAULT_CELLS);
                        input.state.depth += 1;
                        let name = parse_cstr.parse_next(input)?;
                        align4(input)?;
                        return Ok(Some(FdtToken::Node(name)));
                    }
                    FDT_END_NODE => {
                        if input.state.depth == 0 {
                            return Err(ErrMode::Cut(ContextError::new()));
                        }
                        input.state.cells.pop();
                        input.state.depth -= 1;
                        return Ok(Some(FdtToken::NodeEnd));
                    }
                    FDT_PROP => {
                        let len = be_u32.parse_next(input)?;
                        let nameoff = be_u32.parse_next(input)?;
                        let value = take(len).parse_next(input)?;
                        align4(input)?;

                        let name = resolve_name(input, nameoff)?;
                        parse_props(input, name, value)?;

                        return Ok(Some(FdtToken::Prop {
                            name,
                            value: Value::new(value),
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
    type Item = (&'a str, Value<'a>);

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
        unsafe { Fdt::new(QEMU_VIRT_DTB.as_ptr()).unwrap() }
    }

    fn collect_props<'a>(walker: FdtWalker<'a>) -> Vec<Prop<'a>> {
        walker
            .filter_map(|token| match token {
                FdtToken::Prop { name, value } => Some(Prop {
                    name,
                    value: value.into_slice(),
                }),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn qemu_virt_header_matches_binary() {
        let fdt = qemu_fdt();
        let header = [
            fdt.header.magic(),
            fdt.header.total_size(),
            fdt.header.off_dt_struct(),
            fdt.header.off_dt_strings(),
            fdt.header.size_dt_strings(),
            fdt.header.size_dt_struct(),
        ];

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
            collect_props(fdt.query().at("chosen")).as_slice(),
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
            collect_props(fdt.query().at("cpus").at("cpu-map")).as_slice(),
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
        let walker = fdt.query().at("soc").at("serial@10000000");
        let (address_cells, size_cells) = walker.reg_cells();
        let reg = walker
            .prop("reg")
            .expect("serial@10000000 should have a reg property");

        let mut it = reg.into_reg(address_cells, size_cells);
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
        let walker = fdt.query().at("flash@20000000");
        let (address_cells, size_cells) = walker.reg_cells();
        let reg = walker
            .prop("reg")
            .expect("flash should have a reg property");

        let tuples: Vec<_> = reg.into_reg(address_cells, size_cells).collect();
        assert_eq!(tuples.len(), 2);
        assert_eq!(tuples[0], (0x20000000, Some(0x02000000)));
        assert_eq!(tuples[1], (0x22000000, Some(0x02000000)));
    }

    #[test]
    fn reg_iter_4byte_address() {
        // Simulate a 32-bit address with #address-cells=1, #size-cells=1
        let reg = b"\x80\x00\x00\x00\x00\x10\x00\x00";
        let tuples: Vec<_> = RegIter::new(reg, 1, 1).collect();
        assert_eq!(tuples.len(), 1);
        assert_eq!(tuples[0], (0x80000000, Some(0x100000)));
    }

    #[test]
    fn reg_iter_zero_size_cells() {
        // #size-cells=0: reg is just addresses, no sizes
        let reg = b"\x00\x00\x00\x00\x10\x00\x00\x00";
        let tuples: Vec<_> = RegIter::new(reg, 2, 0).collect();
        assert_eq!(tuples.len(), 1);
        assert_eq!(tuples[0], (0x10000000, None));
    }

    #[test]
    fn walker_reg_cells_serial_uses_soc_cells() {
        // serial@10000000 is under /soc, which sets #address-cells=2, #size-cells=2
        let fdt = qemu_fdt();
        let walker = fdt.query().at("soc").at("serial@10000000");
        assert_eq!(walker.reg_cells(), (2, 2));
    }

    #[test]
    fn walker_reg_cells_soc_uses_root_cells() {
        let fdt = qemu_fdt();
        let walker = fdt.query().at("soc");
        assert_eq!(walker.reg_cells(), (2, 2));
    }

    #[test]
    fn walker_reg_cells_root_defaults() {
        // root itself might not have #address-cells/#size-cells
        let fdt = qemu_fdt();
        assert_eq!(fdt.query().reg_cells(), (2, 1));
    }

    #[test]
    fn walker_reg_cells_update_after_props_are_parsed() {
        let fdt = qemu_fdt();
        let mut walker = fdt.query();
        assert_eq!(walker.reg_cells(), (2, 1));

        assert!(matches!(
            walker.next(),
            Some(FdtToken::Prop {
                name: "#address-cells",
                ..
            })
        ));
        assert_eq!(walker.reg_cells(), (2, 1));

        assert!(matches!(
            walker.next(),
            Some(FdtToken::Prop {
                name: "#size-cells",
                ..
            })
        ));
        assert_eq!(walker.reg_cells(), (2, 1));

        let flash = walker.at("flash@20000000");
        assert_eq!(flash.reg_cells(), (2, 2));
    }
}
