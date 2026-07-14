use crate::fs::Path;

#[allow(unused)]
#[derive(Debug, Default)]
pub struct FsContext {
    root: Path,
    cwd: Path,
}
