use alloc::boxed::Box;
use alloc::collections::btree_map::BTreeMap;
use alloc::sync::Arc;
use core::ops::Deref;

use crate::fs::Fnode;
use crate::kernel::console::ConsoleFnode;
use crate::kernel::sync::SpinLock;

pub enum FileKind {
    Regular {
        node: Box<dyn Fnode>,
        position: usize,
    },
}

pub struct File {
    kind: FileKind,
}

impl From<Box<dyn Fnode>> for File {
    fn from(node: Box<dyn Fnode>) -> Self {
        Self {
            kind: FileKind::Regular { node, position: 0 },
        }
    }
}

impl<T: Fnode + 'static> From<T> for File {
    fn from(value: T) -> Self {
        Self {
            kind: FileKind::Regular {
                node: Box::new(value),
                position: 0,
            },
        }
    }
}

impl File {
    pub fn read(&mut self, buffer: &mut [u8]) -> usize {
        match &mut self.kind {
            FileKind::Regular { node, position } => {
                let len = node.read(*position, buffer);
                *position += len;
                len
            }
        }
    }

    pub fn write(&mut self, buffer: &[u8]) -> usize {
        match &mut self.kind {
            FileKind::Regular { node, position } => {
                let len = node.write(*position, buffer);
                *position += len;
                len
            }
        }
    }
}

pub type FileRef = Arc<SpinLock<File>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileDescriptor(usize);

impl From<usize> for FileDescriptor {
    fn from(value: usize) -> Self {
        Self::new(value)
    }
}

impl From<FileDescriptor> for usize {
    fn from(value: FileDescriptor) -> Self {
        value.as_raw()
    }
}

impl Deref for FileDescriptor {
    type Target = usize;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FileDescriptor {
    pub const STD_IN: FileDescriptor = FileDescriptor(0);
    pub const STD_OUT: FileDescriptor = FileDescriptor(1);
    pub const STD_ERR: FileDescriptor = FileDescriptor(2);

    pub const fn new(raw: usize) -> Self {
        Self(raw)
    }

    pub const fn as_raw(self) -> usize {
        self.0
    }
}

pub struct FileContext {
    descriptors: BTreeMap<FileDescriptor, FileRef>,
}

impl Default for FileContext {
    fn default() -> Self {
        Self::new()
    }
}

impl FileContext {
    pub fn new() -> Self {
        let mut descriptors = BTreeMap::default();

        let stdio = Arc::new(SpinLock::new(ConsoleFnode.into()));
        descriptors.insert(FileDescriptor::STD_IN, stdio.clone());
        descriptors.insert(FileDescriptor::STD_OUT, stdio.clone());
        descriptors.insert(FileDescriptor::STD_ERR, stdio.clone());

        Self { descriptors }
    }

    pub fn insert(&mut self, file: impl Into<File>) -> FileDescriptor {
        self.insert_ref(Arc::new(SpinLock::new(file.into())))
    }

    fn insert_ref(&mut self, file: FileRef) -> FileDescriptor {
        let mut fd = 0;
        for used in self.descriptors.keys() {
            if fd != **used {
                break;
            }
            fd += 1;
        }

        let fd = FileDescriptor(fd);
        self.descriptors.insert(fd, file);
        fd
    }

    pub fn get(&self, fd: FileDescriptor) -> Option<FileRef> {
        self.descriptors.get(&fd).cloned()
    }

    pub fn read(&self, fd: FileDescriptor, buffer: &mut [u8]) -> Option<usize> {
        let file = self.get(fd)?;
        let read = file.lock().read(buffer);
        Some(read)
    }

    pub fn write(&self, fd: FileDescriptor, buffer: &[u8]) -> Option<usize> {
        let file = self.get(fd)?;
        let written = file.lock().write(buffer);
        Some(written)
    }

    pub fn remove(&mut self, fd: FileDescriptor) -> Option<FileRef> {
        self.descriptors.remove(&fd)
    }
}
