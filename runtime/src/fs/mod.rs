pub use context::FsContext;
pub use exec::kernel_exec;
pub use file::{File, Fnode};

mod context;
mod exec;
mod file;
mod tarfs;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::convert::Infallible;
use core::str::FromStr;

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

static MOUNTS: SpinLock<Option<HashMap<Path, Box<dyn Fs>>>> = SpinLock::new(None);

pub fn init() {
    let mut guard = MOUNTS.lock();

    let mut map = HashMap::new();
    map.insert(
        Path::from_str("/").unwrap(),
        Box::new(Tarfs::from(INITARFS)) as Box<dyn Fs>,
    );

    *guard = Some(map);
}

trait Fs: Send {
    fn open(&self, path: &Path) -> Result<File>;
}

#[derive(Debug, Default, PartialEq, Eq, Hash)]
pub struct Path(Vec<String>);

impl FromStr for Path {
    type Err = Infallible;

    fn from_str(s: &str) -> core::result::Result<Self, Self::Err> {
        Ok(Self(
            s.split('/')
                .filter(|component| !component.is_empty() && *component != ".")
                .map(String::from)
                .collect(),
        ))
    }
}

pub fn open(path: &str) -> Result<File> {
    let guard = MOUNTS.lock();
    let mounts = guard.as_ref().ok_or(Error::NotFound)?;
    let root = mounts
        .get(&Path::from_str("/").unwrap())
        .ok_or(Error::NotFound)?;

    root.open(&Path::from_str(path).unwrap())
}
