use core::sync::atomic::{AtomicUsize, Ordering};

static THREAD_ID_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ThreadId(usize);

impl ThreadId {
    pub fn issue() -> Self {
        Self(THREAD_ID_COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}
