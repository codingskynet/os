pub use context::FsContext;
pub use exec::kernel_exec;
pub use path::{AbsolutePath, AbsolutePathBuf};

mod context;
mod exec;
mod path;
mod tarfs;

use alloc::boxed::Box;

use hashbrown::HashMap;
use tarfs::Tarfs;

use crate::kernel::sync::SpinLock;

type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not found")]
    NotFound,
    #[error("invalid elf")]
    InvalidElf(#[from] elf::ParseError),
    #[error("invalid executable")]
    InvalidExecutable,
    #[error("out of memory")]
    OutOfMemory,
}

const INITARFS: &[u8] = include_bytes!("../../../artifacts/initarfs");

static MOUNTS: SpinLock<Option<HashMap<AbsolutePathBuf, Box<dyn Fs>>>> = SpinLock::new(None);

pub fn init() {
    let mut guard = MOUNTS.lock();

    let mut map = HashMap::new();
    map.insert(
        AbsolutePathBuf::root(),
        Box::new(Tarfs::from(INITARFS)) as Box<dyn Fs>,
    );

    *guard = Some(map);
}

trait Fs: Send {
    fn open(&self, path: &AbsolutePathBuf) -> Result<Box<dyn Fnode>>;
}

pub trait Fnode: Send + Sync {
    fn read(&self, offset: usize, buffer: &mut [u8]) -> usize;
    fn write(&self, offset: usize, buffer: &[u8]) -> usize;
}
