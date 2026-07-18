//! Cooperative kernel threads and context-switch handoff.

use alloc::boxed::Box;
use alloc::sync::Arc;
use core::mem::{self, ManuallyDrop};
use core::ptr;
use core::sync::atomic::{AtomicIsize, Ordering};

use crate::arch;
use crate::fs::FsContext;
use crate::kernel::file::FileContext;
use crate::kernel::scheduler::SCHEDULER;
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

pub fn jump_to_idle() -> ! {
    let mut idle = Thread::new(|| {
        loop {
            core::hint::spin_loop();
            SCHEDULER.try_run_next();
        }
    });

    idle.state = ThreadState::Running;
    let idle = Box::into_raw(idle);
    // SAFETY: `idle` is intentionally leaked and therefore keeps its page-table
    // root alive. Every kernel mapping needed to finish the direct switch is
    // shared with the currently active address space.
    unsafe {
        (*idle).mm.activate();
        arch::switch::_switch_to((*idle).context())
    }
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

/// Kernel thread allocation and stack.
///
/// The thread object is exactly one stack-sized, stack-aligned allocation. The
/// bottom of the allocation stores the thread metadata and the rest is used as
/// that thread's kernel stack.
#[repr(C, align(16384))]
pub struct Thread {
    state: ThreadState,
    switch: arch::switch::SwitchContext,
    pub fs: FsContext,
    pub mm: MmContext,
    pub files: FileContext,
    entry: Option<Box<dyn FnOnce() + Send>>,
    exit_code: Option<Arc<AtomicIsize>>, // todo: split RW?
}

impl Thread {
    pub fn new(entry: impl FnOnce() + Send + 'static) -> Box<Thread> {
        assert_eq!(mem::size_of::<Self>(), arch::consts::STACK_SIZE.get());

        // Avoid materializing the 16 KiB, 16 KiB-aligned thread object on the
        // current stack before moving it into the heap allocation.
        let mut thread = unsafe {
            let mut thread = Box::<Self>::new_uninit();
            let thread_ptr = thread.as_mut_ptr();
            ptr::addr_of_mut!((*thread_ptr).state).write(ThreadState::Ready);
            ptr::addr_of_mut!((*thread_ptr).switch).write(arch::switch::SwitchContext::default());
            ptr::addr_of_mut!((*thread_ptr).fs).write(FsContext::default());
            ptr::addr_of_mut!((*thread_ptr).mm).write(MmContext::default());
            ptr::addr_of_mut!((*thread_ptr).files).write(FileContext::default());
            ptr::addr_of_mut!((*thread_ptr).entry).write(Some(Box::new(entry)));
            ptr::addr_of_mut!((*thread_ptr).exit_code)
                .write(Some(Arc::new(AtomicIsize::new(isize::MIN))));
            thread.assume_init()
        };

        let stack_top = thread.stack_top();
        let thread_ptr = Va::from(&*thread);
        thread
            .switch
            .as_kernel_thread_trampoline(stack_top, thread_ptr);
        thread
    }

    pub fn context(&self) -> &arch::switch::SwitchContext {
        &self.switch
    }

    pub fn stack_top(&self) -> Va {
        let s = Va::from(self);
        s.offset(mem::size_of::<Self>())
    }

    pub fn exit_code(&self) -> Option<Arc<AtomicIsize>> {
        self.exit_code.clone()
    }

    pub fn set_exit(&mut self, code: isize) {
        self.state = ThreadState::Exited;
        self.exit_code
            .take()
            .unwrap()
            .store(code, Ordering::Relaxed);
    }

    pub fn with_current(f: impl FnOnce(&mut Thread)) {
        let sp = arch::asm::reg::sp();
        f(unsafe { &mut *Va::new(sp & !(mem::align_of::<Thread>() - 1)).as_mut_ptr() })
    }

    pub fn fs_context(&self) -> &FsContext {
        &self.fs
    }

    pub fn file_context(&mut self) -> &mut FileContext {
        &mut self.files
    }

    pub fn replace_current_mm(mm: MmContext) {
        Self::with_current(|current| {
            let old = mem::replace(&mut current.mm, mm);
            // SAFETY: the new context contains the shared kernel mappings and
            // remains owned by the current thread after activation.
            unsafe { current.mm.activate() };
            // Do not release the old root until satp points at the new one.
            drop(old);
        });
    }

    pub fn run(mut thread: ManuallyDrop<Box<Self>>) {
        Self::with_current(|current| {
            if !ptr::eq(current, &**thread) {
                thread.state = ThreadState::Running;

                // SAFETY: `current` is the currently running thread recovered
                // from the active stack, and `thread` was just removed from the
                // scheduler queue, so both switch contexts are live and stable
                // across the context switch. `thread` is intentionally
                // `ManuallyDrop`: ownership of the selected thread allocation
                // stays parked in this suspended stack frame until this call
                // returns on a later switch back to `current`.
                unsafe {
                    arch::switch::_switch(&mut current.switch, &thread.switch, current);
                }
            }
        });
    }
}

pub extern "C" fn _kernel_thread_start(thread: &mut Thread) -> ! {
    thread.entry.take().unwrap()();
    thread.exit(0);
}

/// Requeue or destroy the thread that just stopped running after a context
/// switch.
///
/// # Safety
///
/// `prev` must be the previously running thread passed by `_switch`. It must
/// be a live `Thread` allocation that is no longer executing and is not present
/// in the ready queue.
pub unsafe extern "C" fn _after_switch(prev: *mut Thread) {
    // SAFETY: `_switch` passes the previously running thread as `prev` and calls
    // this exactly once after the next thread's stack/registers have been
    // restored. At this point `prev` is no longer executing and is not in the
    // ready queue. Rebuilding a `Box` is used only to either requeue that
    // allocation or destroy it when the thread has exited.
    unsafe {
        Thread::with_current(|current| {
            // SAFETY: `_switch` has restored `current`'s stack while retaining the
            // thread allocation that owns its memory context. Kernel mappings are
            // shared across contexts, and the switch epilogue keeps interrupts
            // disabled until this function returns.
            current.mm.activate();
        });

        let prev = &mut *prev;
        match prev.state {
            ThreadState::Ready => unreachable!("previous thread was ready during after_switch"),
            ThreadState::Exited => drop(Box::from_raw(prev)),
            ThreadState::Running => {
                prev.state = ThreadState::Ready;
                SCHEDULER.push(Box::from_raw(prev))
            }
            ThreadState::Blocked => todo!("blocked threads are not implemented"),
        }
    }
}
