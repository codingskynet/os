//! Cooperative kernel threads and context-switch handoff.

pub use context::*;

mod context;
mod stack;

use alloc::boxed::Box;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicIsize, Ordering};

use crate::arch;
use crate::fs::FsContext;
use crate::kernel::file::FileContext;
use crate::kernel::scheduler::SCHEDULER;
use crate::kernel::thread::stack::KernelStack;
use crate::mm::MmContext;
use crate::mm::addr::Va;

pub fn spawn(entry: impl FnOnce() + Send + 'static) -> Arc<AtomicIsize> {
    let thread = Thread::new(entry);
    let exit_code = thread.exit_code().unwrap();
    SCHEDULER.push(thread);
    exit_code
}

pub fn yield_now() {
    SCHEDULER.run_next();
}

/// Scheduler-visible lifecycle state for a kernel thread.
#[allow(unused)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadState {
    Ready,
    Running,
    Blocked,
    Exited,
}

/// Kernel thread metadata, stored separately from its guarded stack mappings.
#[repr(C)]
pub struct Thread {
    state: ThreadState,
    switch: arch::switch::SwitchContext,
    stack: KernelStack,
    entry: Option<Box<dyn FnOnce() + Send>>,
    exit_code: Option<Arc<AtomicIsize>>, // todo: split RW?

    pub fs: FsContext,
    pub mm: MmContext,
    pub files: FileContext,
}

impl Thread {
    pub fn new(entry: impl FnOnce() + Send + 'static) -> Box<Thread> {
        // Install the shared kernel-stack mappings before cloning the active
        // kernel page-table subtree into this thread's memory context.
        let stack = KernelStack::new();

        let mut thread = Box::new(Self {
            state: ThreadState::Ready,
            switch: arch::switch::SwitchContext::default(),
            stack,
            entry: Some(Box::new(entry)),
            exit_code: Some(Arc::new(AtomicIsize::new(isize::MIN))),
            fs: FsContext::default(),
            mm: MmContext::default(),
            files: FileContext::default(),
        });

        thread
            .switch
            .as_kernel_thread_trampoline(thread.stack.top());
        thread
    }

    pub fn exit_code(&self) -> Option<Arc<AtomicIsize>> {
        self.exit_code.clone()
    }

    fn set_exit(&mut self, code: isize) {
        self.state = ThreadState::Exited;
        self.exit_code
            .take()
            .unwrap()
            .store(code, Ordering::Relaxed);
    }

    pub fn stack_bottom(&self) -> Va {
        self.stack.bottom()
    }

    fn stack_top(&self) -> Va {
        self.stack.top()
    }
}
