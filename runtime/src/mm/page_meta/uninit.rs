//! Page metadata for pages not yet assigned to an allocator state.

use super::*;

/// Marker type for freshly created page metadata.
pub enum Uninit {}

impl OwnedPageMeta<Uninit> {
    pub fn consume_as_reserved(mut self) {
        *self.as_mut() = PageMetaState::Reserved;
    }
}
