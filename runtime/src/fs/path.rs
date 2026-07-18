use alloc::borrow::ToOwned;
use alloc::string::{String, ToString};
use core::borrow::Borrow;
use core::ops::Deref;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AbsolutePathBuf(String);

impl Default for AbsolutePathBuf {
    fn default() -> Self {
        Self::root()
    }
}

impl Borrow<AbsolutePath> for AbsolutePathBuf {
    fn borrow(&self) -> &AbsolutePath {
        self.as_path()
    }
}

impl AsRef<AbsolutePath> for AbsolutePathBuf {
    fn as_ref(&self) -> &AbsolutePath {
        self.as_path()
    }
}

impl Deref for AbsolutePathBuf {
    type Target = AbsolutePath;

    fn deref(&self) -> &Self::Target {
        self.as_path()
    }
}

impl AbsolutePathBuf {
    pub fn root() -> Self {
        AbsolutePath::ROOT.to_owned()
    }

    pub fn as_path(&self) -> &AbsolutePath {
        // `AbsolutePathBuf` can only be constructed from an absolute path.
        unsafe { AbsolutePath::from_str_unchecked(&self.0) }
    }
}

/// Borrowed form of an absolute path.
///
/// This is an unsized newtype for the same reason that `str` and
/// `std::path::Path` are unsized: an `&AbsolutePath` is a view into string
/// storage owned elsewhere, usually an `AbsolutePathBuf`.
#[derive(Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct AbsolutePath(str);

impl ToOwned for AbsolutePath {
    type Owned = AbsolutePathBuf;

    fn to_owned(&self) -> Self::Owned {
        AbsolutePathBuf(self.0.to_string())
    }

    fn clone_into(&self, target: &mut Self::Owned) {
        self.0.clone_into(&mut target.0);
    }
}

impl AsRef<AbsolutePath> for AbsolutePath {
    fn as_ref(&self) -> &AbsolutePath {
        self
    }
}

impl AsRef<str> for AbsolutePath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl AbsolutePath {
    pub const ROOT: &'static Self = unsafe { Self::from_str_unchecked("/") };

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn join(&self, path: &str) -> AbsolutePathBuf {
        let mut joined = if path.starts_with('/') {
            String::from("/")
        } else {
            self.0.to_string()
        };

        for component in path.split('/') {
            match component {
                "" | "." => {}
                ".." => {
                    if joined.len() == 1 {
                        continue;
                    }
                    let parent = joined.rfind('/').unwrap_or(0);
                    joined.truncate(parent.max(1));
                }
                component => {
                    if joined.len() > 1 {
                        joined.push('/');
                    }
                    joined.push_str(component);
                }
            }
        }

        AbsolutePathBuf(joined)
    }

    const unsafe fn from_str_unchecked(path: &str) -> &Self {
        // SAFETY: `AbsolutePath` is transparent over `str`, so the data pointer
        // and slice metadata have exactly the same representation.
        unsafe { &*(core::ptr::from_ref(path) as *const Self) }
    }
}
