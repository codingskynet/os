//! Small architecture-independent utilities.

mod bitmap;
pub use bitmap::Bitmap;

#[allow(non_upper_case_globals)]
pub mod consts;

pub mod linked_list;
pub mod r#macro;
