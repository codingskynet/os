use tar_no_std::{ArchiveEntry, TarArchiveRef};

use super::*;

pub struct Tarfs {
    entries: HashMap<AbsolutePathBuf, ArchiveEntry<'static>>,
}

impl From<&'static [u8]> for Tarfs {
    fn from(value: &'static [u8]) -> Self {
        Self {
            entries: TarArchiveRef::new(value)
                .unwrap()
                .entries()
                .map(|entry| {
                    (
                        AbsolutePath::ROOT.join(entry.filename().as_str().unwrap()),
                        entry,
                    )
                })
                .collect(),
        }
    }
}

impl Fs for Tarfs {
    fn open(&self, path: &AbsolutePathBuf) -> Result<Box<dyn Fnode>> {
        match self.entries.get(path) {
            Some(entry) => Ok(Box::new(TarfsFnode::new(entry.data()))),
            None => Err(Error::NotFound),
        }
    }
}

pub struct TarfsFnode<'a> {
    data: &'a [u8],
}

impl<'a> TarfsFnode<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data }
    }
}

impl<'a> Fnode for TarfsFnode<'a> {
    fn read(&self, offset: usize, buffer: &mut [u8]) -> usize {
        let Some(data) = self.data.get(offset..) else {
            return 0;
        };

        let len = data.len().min(buffer.len());
        buffer[..len].copy_from_slice(&data[..len]);
        len
    }

    fn write(&self, _offset: usize, _buffer: &[u8]) -> usize {
        0
    }
}
