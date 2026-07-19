use alloc::boxed::Box;
use core::cell::UnsafeCell;
use core::mem;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::arch;
use crate::arch::interrupt::InterruptGuard;
use crate::kernel::scheduler::SCHEDULER;
use crate::kernel::thread::{Thread, ThreadState};
use crate::mm::MmContext;
use crate::mm::addr::Va;

static CURRENT: CurrentThread = CurrentThread::empty(); // TODO: per-core

/// Owns the running thread on the boot hart.
///
/// Ready threads are owned by the scheduler. Immediately before a context
/// switch, ownership moves here from the run queue and the previous running
/// thread is converted to the raw pointer consumed by `_after_switch`.
///
/// The mutable-borrow flag is deliberately separate from ownership. It rejects
/// reentrant `with_current_mut` calls before a second `&mut Thread` is
/// constructed, and context switching is forbidden while such a borrow is live.
pub struct CurrentThread {
    thread: UnsafeCell<Option<Box<Thread>>>,
    borrowed: AtomicBool,
}

// SAFETY: the kernel currently runs one hart. Every access to `thread` happens
// with local interrupts disabled, and `borrowed` rejects reentrant mutable
// access. SMP bring-up must replace this singleton with per-hart storage.
unsafe impl Sync for CurrentThread {}

impl CurrentThread {
    pub const fn empty() -> Self {
        Self {
            thread: UnsafeCell::new(None),
            borrowed: AtomicBool::new(false),
        }
    }

    /// Run a non-switching operation with exclusive access to the current
    /// thread. Reentrant mutable access and scheduler entry from `f` panic.
    pub fn with_mut<R>(f: impl FnOnce(&mut Thread) -> R) -> R {
        CURRENT.borrow().with_mut(f)
    }

    pub fn replace_mm(mm: MmContext) {
        Self::with_mut(|current| {
            let old = mem::replace(&mut current.mm, mm);
            // SAFETY: the new context contains the shared kernel mappings and
            // remains owned by the current thread after activation.
            unsafe { current.mm.activate() };
            // Do not release the old root until satp points at the new one.
            drop(old);
        })
    }

    /// Leave S-mode using the current thread's full guarded stack as the trap
    /// anchor for subsequent U-mode entries.
    ///
    /// # Safety
    ///
    /// `entry` and the memory below `user_sp` must be valid user mappings in
    /// the current thread's active address space.
    pub unsafe fn enter_user(entry: Va, user_sp: Va) -> ! {
        let kernel_sp = Self::with_mut(|thread| {
            if cfg!(feature = "smoke-user-trap-stack-overflow") {
                thread
                    .stack_bottom()
                    .offset(arch::trap::TRAP_ENTRY_SCRATCH_SIZE)
            } else {
                thread.stack_top()
            }
        });

        // SAFETY: normally `kernel_sp` is the mapped top of the guarded stack.
        // The U-mode trap overflow smoke deliberately selects its mapped lower
        // edge so trap entry must reject the frame before touching it.
        unsafe { arch::trap::enter_user(entry, user_sp, kernel_sp) }
    }

    /// Replace the running owner and enter `next`'s saved context.
    ///
    /// The scheduler calls this with local interrupts disabled. No Rust borrow
    /// is allowed to cross the context switch; only the previous allocation's
    /// raw pointer crosses the assembly boundary until `_after_switch` rebuilds
    /// its `Box` on the new stack.
    pub fn switch_to(mut next: Box<Thread>) {
        assert!(!arch::asm::interrupt::is_enabled());
        next.state = ThreadState::Running;

        unsafe { next.mm.activate() };

        let (next_ptr, prev_ptr) = {
            let guard = CURRENT.borrow();
            let next_ptr = &mut *next as *mut Thread;
            let previous = guard.replace(next).expect("no current kernel thread");
            (next_ptr, Box::into_raw(previous))
        };

        // SAFETY: both allocations are live and distinct. CURRENT owns `next`;
        // `_after_switch` consumes `prev_ptr` after execution leaves its stack.
        // The owner-slot borrow was released before entering the new context,
        // and the scheduler keeps interrupts disabled across this handoff.
        unsafe {
            arch::switch::_switch(
                core::ptr::addr_of_mut!((*prev_ptr).switch),
                core::ptr::addr_of!((*next_ptr).switch),
                prev_ptr,
            );
        }
    }

    fn borrow(&self) -> CurrentThreadGuard<'_> {
        let guard = InterruptGuard::new();
        assert!(
            self.borrowed
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok(),
            "current thread is already borrowed"
        );
        CurrentThreadGuard {
            current: self,
            _guard: guard,
        }
    }
}

struct CurrentThreadGuard<'a> {
    current: &'a CurrentThread,
    _guard: InterruptGuard,
}

impl CurrentThreadGuard<'_> {
    fn replace(&self, next: Box<Thread>) -> Option<Box<Thread>> {
        unsafe { (&mut *self.current.thread.get()).replace(next) }
    }

    fn with_mut<R>(&self, f: impl FnOnce(&mut Thread) -> R) -> R {
        let current = unsafe { &mut *self.current.thread.get() }
            .as_deref_mut()
            .expect("no current kernel thread");
        f(current)
    }
}

impl Drop for CurrentThreadGuard<'_> {
    fn drop(&mut self) {
        self.current.borrowed.store(false, Ordering::Release);
    }
}

pub fn jump_to_idle() -> ! {
    let mut idle = Thread::new(|| {
        arch::trap::init();
        arch::timer::init();

        loop {
            core::hint::spin_loop();
            SCHEDULER.try_run_next();
        }
    });
    idle.state = ThreadState::Running;

    assert!(!arch::asm::interrupt::is_enabled());
    let idle = {
        unsafe { idle.mm.activate() };
        let idle_ptr = &mut *idle as *mut Thread;
        assert!(
            CURRENT.borrow().replace(idle).is_none(),
            "current thread is already installed"
        );
        idle_ptr
    };

    // SAFETY: CURRENT owns `idle` for the lifetime of its running context. Every
    // kernel mapping needed to finish the direct switch is shared with the
    // currently active address space. Boot keeps interrupts disabled until
    // `_switch_to` restores the idle thread's saved SIE state.
    unsafe { arch::switch::_switch_to(&(*idle).switch) }
}

pub extern "C" fn _kernel_thread_start() -> ! {
    let entry = CurrentThread::with_mut(|thread| thread.entry.take().unwrap());
    entry();
    exit(0);
}

/// Requeue or destroy the thread that just stopped running after a context
/// switch.
///
/// # Safety
///
/// `prev` must be the previously running thread passed by `_switch`. CURRENT
/// must own the thread whose context and stack `_switch` just restored. `prev`
/// must no longer be executing or present in the ready queue.
pub unsafe extern "C" fn _after_switch(prev: *mut Thread) {
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

/// Terminate the running thread and transfer its ownership through the normal
/// scheduler handoff. This function never returns to the exiting stack.
pub fn exit(code: isize) -> ! {
    // There is deliberately no matching re-enable. The next thread's saved
    // sstatus controls its interrupt state, and the exiting allocation is
    // destroyed by `_after_switch` only after execution leaves this stack.
    arch::asm::interrupt::disable();
    CurrentThread::with_mut(|current| current.set_exit(code));
    SCHEDULER.run_next();
    unreachable!("exited thread resumed after scheduler switch")
}
