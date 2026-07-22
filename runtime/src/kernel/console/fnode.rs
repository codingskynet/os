//! VFS adapter for the kernel console.

use core::task::{Context, Poll};

use super::CONSOLE;
use crate::dev::uart::{Read, Write};
use crate::fs::Fnode;

pub struct ConsoleFnode;

impl Fnode for ConsoleFnode {
    fn poll_read(&self, _offset: usize, buffer: &mut [u8], cx: &mut Context<'_>) -> Poll<usize> {
        let mut console = CONSOLE.lock();
        match console.read(buffer).unwrap() {
            Some(read) => Poll::Ready(read),
            None => {
                console.register_rx_waker(cx.waker());
                Poll::Pending
            }
        }
    }

    fn write(&self, _offset: usize, buffer: &[u8]) -> usize {
        CONSOLE.lock().write(buffer).unwrap()
    }
}
