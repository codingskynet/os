use alloc::boxed::Box;
use core::mem::{self, ManuallyDrop};
use core::num::NonZeroUsize;
use core::ptr;

use crate::arch;
use crate::arch::switch::{_switch, _switch_to, SwitchContext};
use crate::kernel::scheduler::SCHEDULER;
use crate::mm::addr::Va;
use crate::util::consts::K;

pub const STACK_SIZE: NonZeroUsize = NonZeroUsize::new(16 * K).unwrap();

pub fn spawn(entry: impl FnOnce() + Send + 'static) {
    SCHEDULER.push(Thread::new(entry));
}

pub fn yield_now() {
    SCHEDULER.run_next();
}

pub fn jump_to_idle() -> ! {
    let mut idle = Thread::new(|| {
        loop {
            core::hint::spin_loop();
            yield_now();
        }
    });

    idle.state = ThreadState::Running;
    unsafe { _switch_to(Box::into_raw(idle).as_ref().unwrap().context()) }
}

fn exit_current() -> ! {
    Thread::with_current(|current| {
        current.state = ThreadState::Exited;
    });
    SCHEDULER.run_next();
    unreachable!("exited thread resumed after scheduler switch")
}

#[allow(unused)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadState {
    Ready,
    Running,
    Blocked,
    Exited,
}

#[repr(C, align(16384))]
pub struct Thread {
    state: ThreadState,
    context: SwitchContext,
    entry: Option<Box<dyn FnOnce() + Send>>,
    stack_bottom: (),
}

impl Thread {
    pub fn new(entry: impl FnOnce() + Send + 'static) -> Box<Thread> {
        assert_eq!(mem::size_of::<Self>(), STACK_SIZE.get());

        let mut thread = Box::new(Self {
            state: ThreadState::Ready,
            context: SwitchContext::default(),
            entry: Some(Box::new(entry)),
            stack_bottom: (),
        });
        let stack_top = thread.stack_top();
        let thread_ptr = Va::from(&*thread);
        thread
            .context
            .as_kernel_thread_trampoline(stack_top, thread_ptr);
        thread
    }

    pub fn context(&self) -> &arch::switch::SwitchContext {
        &self.context
    }

    pub fn stack_top(&self) -> Va {
        let s = Va::from(self);
        s.checked_offset(mem::size_of::<Self>()).unwrap()
    }

    pub fn with_current(f: impl FnOnce(&mut Thread)) {
        let sp = arch::asm::reg::sp();
        f(unsafe { &mut *Va::new(sp & !(mem::align_of::<Thread>() - 1)).as_mut_ptr() })
    }

    pub fn run(mut thread: ManuallyDrop<Box<Self>>) {
        Thread::with_current(|current| {
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
                    _switch(&mut current.context, &thread.context, current);
                }
            }
        });
    }
}

pub extern "C" fn _kernel_thread_start(thread: &mut Thread) -> ! {
    thread.entry.take().unwrap()();
    exit_current()
}

pub extern "C" fn _after_switch(prev: *mut Thread) {
    // SAFETY: `_switch` passes the previously running thread as `prev` and calls
    // this exactly once after the next thread's stack/registers have been
    // restored. At this point `prev` is no longer executing and is not in the
    // ready queue. Rebuilding a `Box` is used only to either requeue that
    // allocation or destroy it when the thread has exited.
    unsafe {
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
