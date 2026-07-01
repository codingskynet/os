use crate::dev::dt::{Fdt, FdtToken};

/// Locate the first `memory` node in the device tree and return its `reg`
/// property together with the address/size cell counts for `/`.
///
/// # Safety
///
/// The `fdt` must point to a valid, complete FDT blob that remains readable for
/// the entire call. In particular:
///
/// * The pointer stored inside `fdt` must point to a correctly laid-out FDT blob
///   whose header fields (offsets, sizes) describe memory within that blob.
/// * The blob must not be mutated or deallocated while this function runs.
/// * The `reg` property value slice returned to the caller borrows from the FDT
///   blob — the caller must not deallocate the blob before it is done with the
///   value.
pub unsafe fn find_memory_reg(fdt: &Fdt) -> Option<(&[u8], u32, u32)> {
    unsafe {
        let mut node_name = None;
        let mut reg = None;
        let mut is_memory = false;

        for token in fdt.query() {
            match token {
                FdtToken::Node(name) => {
                    if name.split('@').next() == Some("memory") {
                        node_name = Some(name);
                        is_memory = true;
                        reg = None;
                    } else if node_name.is_some() {
                        node_name = None;
                        is_memory = false;
                        reg = None;
                    }
                }
                FdtToken::Prop { name, value } if node_name.is_some() => match name {
                    "device_type" if value == b"memory\0" => is_memory = true,
                    "reg" => reg = Some(value),
                    _ => {}
                },
                FdtToken::NodeEnd if node_name.is_some() => {
                    if is_memory && let Some(reg) = reg {
                        let (ac, sc) = fdt.reg_cells("/");
                        return Some((reg, ac, sc));
                    }
                    node_name = None;
                    is_memory = false;
                    reg = None;
                }
                _ => {}
            }
        }

        None
    }
}
