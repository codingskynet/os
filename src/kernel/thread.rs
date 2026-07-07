use alloc::boxed::Box;
use core::mem::{self, ManuallyDrop};
use core::num::NonZeroUsize;
use core::ptr;

use crate::arch;
use crate::arch::switch::_switch;
use crate::kernel::scheduler::{self, SCHEDULER};
use crate::mm::addr::Va;
use crate::util::consts::K;

pub const STACK_SIZE: NonZeroUsize = NonZeroUsize::new(16 * K).unwrap();

pub fn spawn(entry: impl FnOnce() + Send + 'static) {
    SCHEDULER.push(Thread::new(entry));
}

pub fn yield_now() {
    scheduler::yield_now();
}

pub fn exit_current() -> ! {
    scheduler::exit_current()
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
    pub state: ThreadState,
    regs: arch::regs::GeneralRegs,
    entry: Option<Box<dyn FnOnce() + Send>>,
    stack_bottom: (),
}

impl Thread {
    pub fn new(entry: impl FnOnce() + Send + 'static) -> Box<Thread> {
        assert_eq!(mem::size_of::<Self>(), STACK_SIZE.get());

        let mut thread = Box::new(Self {
            state: ThreadState::Ready,
            regs: arch::regs::GeneralRegs::default(),
            entry: Some(Box::new(entry)),
            stack_bottom: (),
        });
        let stack_top = thread.stack_top();
        let thread_ptr = Va::from(&*thread);
        thread
            .regs
            .as_kernel_thread_trampoline(stack_top, thread_ptr);
        thread
    }

    pub fn regs(&self) -> &arch::regs::GeneralRegs {
        &self.regs
    }

    pub fn stack_top(&self) -> Va {
        let s = Va::from(self);
        s.checked_offset(mem::size_of::<Self>()).unwrap()
    }

    pub fn with_current(f: impl FnOnce(&mut Thread)) {
        let sp = arch::asm::reg::sp();
        f(unsafe { &mut *Va::new(sp & !(STACK_SIZE.get() - 1)).as_mut_ptr() })
    }

    pub fn run(mut thread: ManuallyDrop<Box<Self>>) {
        Thread::with_current(|current| {
            if !ptr::eq(current, &**thread) {
                thread.state = ThreadState::Running;

                unsafe {
                    _switch(&mut current.regs, &thread.regs, current);
                }
            }
        });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _kernel_thread_start(thread: &mut Thread) -> ! {
    thread.entry.take().unwrap()();
    exit_current()
}

#[unsafe(no_mangle)]
pub extern "C" fn after_switch(prev: *mut Thread) {
    unsafe {
        let prev = &mut *prev;
        match prev.state {
            ThreadState::Ready => unreachable!(),
            ThreadState::Exited => drop(Box::from_raw(prev)),
            ThreadState::Running => {
                prev.state = ThreadState::Ready;
                SCHEDULER.push(Box::from_raw(prev))
            }
            ThreadState::Blocked => todo!("blocked threads are not implemented"),
        }
    }
}
