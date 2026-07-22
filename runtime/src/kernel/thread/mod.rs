//! Cooperative kernel threads and context-switch handoff.

pub use context::*;
pub use id::ThreadId;

mod context;
mod id;
mod stack;
mod waker;

use alloc::boxed::Box;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicIsize, Ordering};

use crate::arch;
use crate::fs::FsContext;
use crate::kernel::file::FileContext;
use crate::kernel::scheduler::Scheduler;
use crate::kernel::thread::stack::KernelStack;
use crate::kernel::thread::waker::ThreadWaker;
use crate::mm::MmContext;
use crate::mm::addr::Va;

pub fn spawn(entry: impl FnOnce() + Send + 'static) -> Arc<AtomicIsize> {
    let thread = Thread::new(entry);
    let exit_code = thread.exit_code().unwrap();
    Scheduler::push_ready(thread);
    exit_code
}

pub fn yield_now() {
    // If no normal thread is ready, switch through this hart's idle context.
    // The switch-out path requeues this thread globally, allowing another hart
    // to claim it before the local idle loop schedules again.
    Scheduler::run_next();
}

/// Scheduler-visible lifecycle state for a kernel thread.
#[allow(unused)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadState {
    Ready,
    Running,
    Parking,
    Parked,
    Exited,
}

/// Distinguishes per-hart idle contexts from globally schedulable threads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadKind {
    Idle,
    Normal,
}

/// Kernel thread metadata, stored separately from its guarded stack mappings.
#[repr(C)]
pub struct Thread {
    kind: ThreadKind,
    state: ThreadState,
    switch: arch::switch::SwitchContext,
    stack: KernelStack,
    entry: Option<Box<dyn FnOnce() + Send>>,
    exit_code: Option<Arc<AtomicIsize>>, // todo: split RW?
    waker: Arc<ThreadWaker>,

    pub fs: FsContext,
    pub mm: MmContext,
    pub files: FileContext,
}

impl Thread {
    pub fn new(entry: impl FnOnce() + Send + 'static) -> Box<Thread> {
        Self::new_with_kind(entry, ThreadKind::Normal)
    }

    /// Build one hart's idle context during primary-hart initialization.
    ///
    /// All idle contexts execute the same entry code, but each owns a distinct
    /// [`Thread`], switch context, and guarded kernel stack. Two harts may be
    /// idle concurrently, so those mutable execution resources cannot be
    /// shared. An idle context is parked in its [`PerCore`](crate::kernel::per_core::PerCore)
    /// while normal work runs and never enters the global run queue.
    pub fn new_idle() -> Box<Thread> {
        Self::new_with_kind(
            || {
                crate::kernel::init::idle_online();
                loop {
                    if !Scheduler::run_next() {
                        arch::asm::interrupt::wait();
                    }
                }
            },
            ThreadKind::Idle,
        )
    }

    pub fn stack_bottom(&self) -> Va {
        self.stack.bottom()
    }

    pub fn exit_code(&self) -> Option<Arc<AtomicIsize>> {
        self.exit_code.clone()
    }

    pub fn id(&self) -> ThreadId {
        self.waker.id()
    }

    fn waker(&self) -> Arc<ThreadWaker> {
        self.waker.clone()
    }

    fn new_with_kind(entry: impl FnOnce() + Send + 'static, kind: ThreadKind) -> Box<Thread> {
        // Install the shared kernel-stack mappings before cloning the active
        // kernel page-table subtree into this thread's memory context.
        let stack = KernelStack::new();

        let mut thread = Box::new(Self {
            kind,
            state: ThreadState::Ready,
            switch: arch::switch::SwitchContext::default(),
            stack,
            entry: Some(Box::new(entry)),
            exit_code: Some(Arc::new(AtomicIsize::new(isize::MIN))),
            waker: Arc::new(ThreadWaker::new(ThreadId::issue())),
            fs: FsContext::default(),
            mm: MmContext::default(),
            files: FileContext::default(),
        });

        thread
            .switch
            .as_kernel_thread_trampoline(thread.stack.top());
        thread
    }

    fn set_exit(&mut self, code: isize) {
        self.state = ThreadState::Exited;
        self.exit_code
            .take()
            .unwrap()
            .store(code, Ordering::Relaxed);
    }

    fn begin_parking(&mut self) {
        assert_eq!(self.state, ThreadState::Running);
        assert_eq!(self.kind, ThreadKind::Normal, "idle thread cannot park");
        self.state = ThreadState::Parking;
    }

    pub fn finish_parking(&mut self) {
        assert_eq!(self.state, ThreadState::Parking);
        self.state = ThreadState::Parked;
    }

    pub fn consume_wake(&self) -> bool {
        self.waker.consume_wake()
    }

    pub fn wake(&mut self) {
        debug_assert!(matches!(
            self.state,
            ThreadState::Parking | ThreadState::Parked
        ));
        self.state = ThreadState::Ready;
    }

    fn stack_top(&self) -> Va {
        self.stack.top()
    }
}
