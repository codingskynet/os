use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use crate::arch::asm::floating_point;
use crate::arch::interrupt::InterruptGuard;
use crate::kernel::thread;
use crate::{asm, printlnk};

const THREADS: usize = 16;
const ITERATIONS: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FpState {
    x: u64,
    velocity: u64,
    accumulator: u64,
    fcsr: u64,
}

pub fn smoke() {
    printlnk!("smoke-floating-point: start threads={THREADS} iterations={ITERATIONS}");

    let mut expected = Vec::with_capacity(THREADS);
    for thread_id in 0..THREADS {
        // Reference calculations must not be interleaved with another thread's
        // FP state. The concurrent run below deliberately omits this guard.
        let _guard = InterruptGuard::new();
        floating_point::enable();
        unsafe {
            fp_initialize(thread_id);
            for _ in 0..ITERATIONS {
                fp_step();
            }
            expected.push(fp_state());
        }
        floating_point::disable();
    }

    let mut exit_codes = Vec::with_capacity(THREADS);
    for (thread_id, expected) in expected.into_iter().enumerate() {
        exit_codes.push(thread::spawn(move || {
            floating_point::enable();
            unsafe { fp_initialize(thread_id) };
            floating_point::disable();

            for _ in 0..ITERATIONS {
                floating_point::enable();
                unsafe { fp_step() };
                floating_point::disable();

                // The FP values remain live only in f0-f8 here. A successful
                // yield therefore requires the scheduler to preserve this
                // thread's complete FP register bank and fcsr.
                thread::yield_now();
            }

            floating_point::enable();
            let actual = unsafe { fp_state() };
            floating_point::disable();

            assert_eq!(
                actual, expected,
                "smoke-floating-point: context mismatch for thread {thread_id}"
            );
        }));
    }

    while exit_codes
        .iter()
        .any(|code| code.load(Ordering::Relaxed) == isize::MIN)
    {
        thread::yield_now();
    }

    for exit_code in exit_codes {
        assert_eq!(exit_code.load(Ordering::Relaxed), 0);
    }

    printlnk!("smoke-floating-point: done threads={THREADS} iterations={ITERATIONS}");
}

/// Load a thread-specific nonlinear-system state and its constant parameters.
///
/// # Safety
///
/// The caller must have enabled `sstatus.FS` and must arrange for the FP
/// register bank to remain owned by the current thread until it is saved.
unsafe fn fp_initialize(thread_id: usize) {
    let x = 0x3ff0_0000_0000_0000u64 + ((thread_id as u64) << 48);
    let velocity = 0xbfe0_0000_0000_0000u64 + ((thread_id as u64) << 47);
    let accumulator = 0x3fd0_0000_0000_0000u64 + ((thread_id as u64) << 48);

    unsafe {
        asm!(
            "fmv.d.x f0, {x}",
            "fmv.d.x f1, {velocity}",
            "fmv.d.x f2, {accumulator}",
            "fmv.d.x f3, {dt}",
            "fmv.d.x f4, {damping}",
            "fmv.d.x f5, {omega}",
            "fmv.d.x f6, {drive}",
            "fmv.d.x f7, {one}",
            "fmv.d.x f8, {scale}",
            "csrw fcsr, zero",
            x = in(reg) x,
            velocity = in(reg) velocity,
            accumulator = in(reg) accumulator,
            dt = in(reg) 0x3fa0_0000_0000_0000u64,
            damping = in(reg) 0x3fef_0000_0000_0000u64,
            omega = in(reg) 0x3fe8_0000_0000_0000u64,
            drive = in(reg) 0x3fc0_0000_0000_0000u64,
            one = in(reg) 0x3ff0_0000_0000_0000u64,
            scale = in(reg) 0x3f90_0000_0000_0000u64,
            options(nomem, nostack, preserves_flags),
        );
    }
}

/// Advance a damped, driven nonlinear oscillator while keeping all state in
/// floating-point registers.
///
/// # Safety
///
/// `fp_initialize` must have initialized f0-f8 for the current thread and
/// `sstatus.FS` must be enabled.
unsafe fn fp_step() {
    unsafe {
        asm!(
            // drive / (1 + x*x)
            "fmul.d f9, f0, f0",
            "fadd.d f9, f9, f7",
            "fdiv.d f10, f6, f9",
            // next_velocity = damping*velocity - omega*x + nonlinear_drive
            "fmul.d f11, f5, f0",
            "fsub.d f10, f10, f11",
            "fmul.d f11, f4, f1",
            "fadd.d f10, f10, f11",
            // next_x = x + dt*velocity
            "fmul.d f11, f3, f1",
            "fadd.d f9, f0, f11",
            // accumulator += scale*next_x*next_velocity
            "fmul.d f11, f9, f10",
            "fmul.d f11, f11, f8",
            "fadd.d f2, f2, f11",
            "fmv.d f0, f9",
            "fmv.d f1, f10",
            options(nomem, nostack, preserves_flags),
        );
    }
}

/// Return the exact architectural bits of the live FP state.
///
/// # Safety
///
/// `fp_initialize` must have initialized the current thread's FP state and
/// `sstatus.FS` must be enabled.
unsafe fn fp_state() -> FpState {
    let x: u64;
    let velocity: u64;
    let accumulator: u64;
    let fcsr: u64;

    unsafe {
        asm!(
            "fmv.x.d {x}, f0",
            "fmv.x.d {velocity}, f1",
            "fmv.x.d {accumulator}, f2",
            "csrr {fcsr}, fcsr",
            x = out(reg) x,
            velocity = out(reg) velocity,
            accumulator = out(reg) accumulator,
            fcsr = out(reg) fcsr,
            options(nomem, nostack, preserves_flags),
        );
    }

    FpState {
        x,
        velocity,
        accumulator,
        fcsr,
    }
}
