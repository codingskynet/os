use alloc::boxed::Box;
use alloc::sync::Arc;
use core::cell::UnsafeCell;
use core::mem;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::{Context, Poll, Waker};

use crate::arch;
use crate::arch::interrupt::InterruptGuard;
use crate::kernel::per_core::PerCore;
use crate::kernel::scheduler::Scheduler;
use crate::kernel::thread::waker::ThreadWaker;
use crate::kernel::thread::{Thread, ThreadKind, ThreadState};
use crate::mm::MmContext;
use crate::mm::addr::Va;

/// Owns the running thread on one hart.
///
/// Ready threads are normally owned by the scheduler. The idle context is
/// preinstalled here before its hart starts; [`jump_to_scheduler`] promotes it
/// directly to Running. After its first switch, the idle context is parked in
/// its [`PerCore`] instead of entering the global run queue. The scheduler may
/// take that local fallback when no normal thread is globally ready.
///
/// The mutable-borrow flag is deliberately separate from ownership. It rejects
/// reentrant `with_current_mut` calls before a second `&mut Thread` is
/// constructed, and context switching is forbidden while such a borrow is live.
pub struct CurrentThread {
    thread: UnsafeCell<Box<Thread>>,
    borrowed: AtomicBool,
}

// SAFETY: each CurrentThread is stored in one PerCore slot and, once installed,
// is accessed only by the hart whose tp selects that slot. Local interrupts are
// disabled around every access, and `borrowed` rejects reentrant mutable access
// on that hart.
unsafe impl Sync for CurrentThread {}

impl CurrentThread {
    /// Create a per-hart owner preloaded with its private idle context.
    ///
    /// The primary hart calls this while constructing the PerCore allocation,
    /// before secondary harts are released. The context becomes Running only
    /// when its hart calls [`jump_to_scheduler`].
    pub fn with_idle() -> Self {
        Self {
            thread: UnsafeCell::new(Thread::new_idle()),
            borrowed: AtomicBool::new(false),
        }
    }

    /// Run a non-switching operation with exclusive access to the current
    /// thread. Reentrant mutable access and scheduler entry from `f` panic.
    pub fn with_mut<R>(f: impl FnOnce(&mut Thread) -> R) -> R {
        PerCore::with_mut(|per_core| per_core.current.borrow().with_mut(f))
    }

    /// Poll one operation on the current kernel thread until it completes.
    ///
    /// A pending operation parks the current thread. Its waker records
    /// notifications that race with the context-switch handoff and makes an
    /// already parked thread runnable through the scheduler.
    pub fn wait<T>(mut poll: impl FnMut(&mut Context<'_>) -> Poll<T>) -> T {
        let inner_waker = Self::with_mut(|thread| thread.waker());
        let waker = Waker::from(inner_waker.clone());
        let mut context = Context::from_waker(&waker);

        loop {
            inner_waker.consume_wake();

            match poll(&mut context) {
                Poll::Ready(output) => return output,
                Poll::Pending if inner_waker.is_waked() => continue,
                Poll::Pending => park(),
            }
        }
    }

    pub fn waker() -> Arc<ThreadWaker> {
        Self::with_mut(|thread| thread.waker())
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
        let (kernel_sp, stack_bottom) = Self::with_mut(|thread| {
            let stack_bottom = thread.stack_bottom();
            (thread.stack_top(), stack_bottom)
        });

        debug_assert!(
            kernel_sp
                .as_raw()
                .checked_sub(mem::size_of::<arch::trap::TrapFrame>())
                .is_some_and(|frame_bottom| frame_bottom >= stack_bottom.as_raw()),
            "kernel stack cannot hold a user trap frame",
        );

        // SAFETY: the assertion above proves that a complete TrapFrame fits in
        // the current thread's mapped kernel stack below `kernel_sp`.
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
            PerCore::with_mut(|per_core| {
                let guard = per_core.current.borrow();
                let next_ptr = &mut *next as *mut Thread;
                let previous = guard.replace(next);
                (next_ptr, Box::into_raw(previous))
            })
        };

        // SAFETY: both allocations are live and distinct. PerCore.current owns `next`;
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
    fn replace(&self, next: Box<Thread>) -> Box<Thread> {
        unsafe { mem::replace(&mut *self.current.thread.get(), next) }
    }

    fn with_mut<R>(&self, f: impl FnOnce(&mut Thread) -> R) -> R {
        let current = unsafe { &mut *self.current.thread.get() };
        f(current)
    }
}

impl Drop for CurrentThreadGuard<'_> {
    fn drop(&mut self) {
        self.current.borrowed.store(false, Ordering::Release);
    }
}

pub fn jump_to_scheduler() -> ! {
    assert!(!arch::asm::interrupt::is_enabled());
    let idle = CurrentThread::with_mut(|idle| {
        assert_eq!(
            idle.state,
            ThreadState::Ready,
            "idle thread already started"
        );
        idle.state = ThreadState::Running;
        unsafe { idle.mm.activate() };
        idle as *mut Thread
    });

    // SAFETY: this hart's PerCore.current has owned its private `idle` since
    // primary-hart initialization. Every kernel mapping needed to finish the
    // direct switch is shared with the currently active address space. Boot
    // keeps interrupts disabled until `_switch_to` restores the idle thread's
    // saved SIE state.
    unsafe { arch::switch::_switch_to(&(*idle).switch) }
}

pub extern "C" fn _kernel_thread_start() -> ! {
    let entry = CurrentThread::with_mut(|thread| thread.entry.take().unwrap());
    entry();
    exit(0);
}

/// Requeue, park, or destroy the thread that just stopped running after a
/// context switch.
///
/// # Safety
///
/// `prev` must be the previously running thread passed by `_switch`. This
/// hart's PerCore.current must own the thread whose context and stack `_switch`
/// just restored. `prev` must no longer be executing or present in the ready
/// queue.
pub unsafe extern "C" fn _after_switch(prev: *mut Thread) {
    // SAFETY: `_switch` passes the previously running thread as `prev` and calls
    // this exactly once after the next thread's stack/registers have been
    // restored. At this point `prev` is no longer executing and is not in the
    // ready queue. Rebuilding a `Box` is used only to requeue a normal thread,
    // park an idle thread in its PerCore, or destroy an exited thread.
    unsafe {
        let prev = &mut *prev;
        match prev.state {
            ThreadState::Ready => unreachable!("previous thread was ready during after_switch"),
            ThreadState::Exited => drop(Box::from_raw(prev)),
            ThreadState::Running => {
                prev.state = ThreadState::Ready;
                if prev.kind == ThreadKind::Idle {
                    assert!(
                        PerCore::try_park_idle(Box::from_raw(prev)).is_none(),
                        "idle is unique"
                    );
                } else {
                    Scheduler::push_ready(Box::from_raw(prev));
                }
            }
            ThreadState::Parking => Scheduler::push_parked(Box::from_raw(prev)),
            ThreadState::Parked => unreachable!("parked thread was running"),
        }
    }
}

/// Park the running thread until its async waker makes it runnable again.
pub fn park() {
    let _guard = InterruptGuard::new();
    CurrentThread::with_mut(|thread| thread.begin_parking());
    assert!(Scheduler::run_next(), "running thread has no idle fallback");
}

/// Terminate the running thread and transfer its ownership through the normal
/// scheduler handoff. This function never returns to the exiting stack.
pub fn exit(code: isize) -> ! {
    // There is deliberately no matching re-enable. The next thread's saved
    // sstatus controls its interrupt state, and the exiting allocation is
    // destroyed by `_after_switch` only after execution leaves this stack.
    arch::asm::interrupt::disable();
    CurrentThread::with_mut(|current| current.set_exit(code));
    Scheduler::run_next();
    unreachable!("exited thread resumed after scheduler switch")
}
