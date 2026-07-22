use crate::fs::ReadResult;
use crate::{fs, printlnk};

const EXPECTED: &[u8] = include_bytes!("../../../../userland/rootfs/hello.txt");

pub fn smoke() {
    printlnk!("smoke-initarfs: start");

    let file = fs::FsContext::default()
        .open("/hello.txt")
        .expect("smoke-initarfs: failed to open /hello.txt");
    let mut buffer = [0; 64];
    let ReadResult::Complete(len) = file.read(0, &mut buffer) else {
        panic!("smoke-initarfs: regular file read blocked")
    };

    assert_eq!(
        &buffer[..len],
        EXPECTED,
        "smoke-initarfs: unexpected /hello.txt contents"
    );
    assert_eq!(
        file.read(len, &mut buffer),
        ReadResult::Complete(0),
        "smoke-initarfs: read at EOF returned data"
    );

    printlnk!("smoke-initarfs: done bytes={len}");
}
