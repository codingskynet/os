//! Decoder for the standard device-tree `reg` property.
//!
//! Devicetree Specification v0.4, section 2.3.6, defines `reg` as address and
//! length pairs. The address width comes from the parent node's
//! `#address-cells`; the length width comes from the parent node's
//! `#size-cells`.
//!
//! Reference: [Devicetree Specification v0.4], section 2.3.6, "`reg`".
//!
//! [Devicetree Specification v0.4]:
//!     https://github.com/devicetree-org/devicetree-specification/releases/download/v0.4/devicetree-specification-v0.4.pdf

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
/// `#address-cells` and `#size-cells` properties. The root defaults are 2
/// address cells and 1 size cell.
pub struct RegIter<'a> {
    data: &'a [u8],
    stride: usize,
    address_len: usize,
    size_len: usize,
}

impl<'a> RegIter<'a> {
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
    /// `(address, size)`; size is `None` when `#size-cells = 0`.
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
