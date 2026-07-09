use super::*;

pub enum Uninit {}

impl OwnedPageMeta<Uninit> {
    pub fn consume_as_reserved(mut self) {
        *self.as_mut() = PageMetaState::Reserved;
    }
}
