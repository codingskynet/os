use alloc::boxed::Box;
use core::arch::naked_asm;
use core::mem::{offset_of, size_of};
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use crate::arch;
use crate::arch::interrupt::InterruptGuard;
use crate::dev::dt::Fdt;
use crate::kernel::thread::CurrentThread;
use crate::panic::PanicStack;

/// Next dense index to assign to a hart installing its per-core pointer.
static NEXT_PER_CORE_INDEX: AtomicUsize = AtomicUsize::new(0);

static PER_CORE: AtomicPtr<PerCore> = AtomicPtr::new(ptr::null_mut());
static PER_CORE_COUNT: AtomicUsize = AtomicUsize::new(0);

fn allocation() -> (*mut PerCore, usize) {
    let ptr = PER_CORE.load(Ordering::Acquire);
    assert!(!ptr.is_null(), "PER_CORE is not initialized");

    (ptr, PER_CORE_COUNT.load(Ordering::Relaxed))
}

#[repr(C)]
pub struct PerCore {
    pub index: usize,
    pub hart_id: usize,
    pub current: CurrentThread,
    panic_stack: Box<PanicStack>,
}

impl PerCore {
    pub const PANIC_STACK_OFFSET: usize = offset_of!(Self, panic_stack);

    fn new(index: usize) -> Self {
        let current = CurrentThread::with_idle();
        Self {
            index,
            hart_id: usize::MAX,
            current,
            panic_stack: PanicStack::allocate(),
        }
    }

    pub fn init(fdt: &Fdt, boot_hart_id: usize) {
        let cpu_count = fdt.cpu_count();
        assert!(cpu_count > 0, "DT does not contain an enabled CPU");
        assert!(
            PER_CORE.load(Ordering::Relaxed).is_null(),
            "PER_CORE is already initialized"
        );

        let mut per_core = Box::<[PerCore]>::new_uninit_slice(cpu_count);
        for (index, slot) in per_core.iter_mut().enumerate() {
            // The primary hart creates every idle context and installs all of
            // their shared kernel-stack mappings before secondary harts are
            // released. Each slot still owns a distinct stack and switch
            // context because harts may execute idle concurrently.
            slot.write(PerCore::new(index));
        }

        // SAFETY: every element in the boxed slice was initialized above.
        let per_core = unsafe { per_core.assume_init() };
        let per_core = Box::leak(per_core);

        PER_CORE_COUNT.store(cpu_count, Ordering::Relaxed);
        PER_CORE.store(per_core.as_mut_ptr(), Ordering::Release);

        let per_core = PerCore::assign(boot_hart_id);
        unsafe { arch::asm::reg::write_tp(per_core as usize) };
    }

    fn assign(hart_id: usize) -> *mut PerCore {
        let index = NEXT_PER_CORE_INDEX.fetch_add(1, Ordering::Relaxed);
        assert!(index == 0, "only boot core can call assign method");
        let (base, cpu_count) = allocation();
        assert!(index < cpu_count, "more harts arrived than the DT declares");
        let per_core = unsafe { base.add(index) };

        // SAFETY: each index is returned once by NEXT_PER_CORE_INDEX, so this
        // hart has exclusive initialization access to its slot. The backing
        // allocation is leaked and remains valid for the kernel lifetime.
        unsafe {
            (*per_core).hart_id = hart_id;
        }
        per_core
    }

    pub fn count() -> usize {
        allocation().1
    }

    pub fn with_mut<R>(f: impl FnOnce(&mut PerCore) -> R) -> R {
        let _guard = InterruptGuard::new();
        let per_core = unsafe { &mut *(arch::asm::reg::tp() as *mut _) };
        f(per_core)
    }

    pub fn is_boot_core() -> bool {
        PerCore::with_mut(|c| c.index == 0)
    }

    pub fn core_id() -> usize {
        PerCore::with_mut(|c| c.hart_id)
    }
}

unsafe extern "C" {
    static secondary_stack_top: u8;
}

/// Install `tp` and acquire the temporary init stack before entering Rust.
///
/// This entry is deliberately stackless: every secondary reaches it with the
/// shared kernel page table active but without a usable `sp`. The atomic index
/// assigns one prebuilt PerCore slot. Secondaries then serialize the short Rust
/// initialization phase on `secondary_stack_top`; `_switch_to` abandons it for
/// the selected hart's permanent idle stack.
///
/// # Safety
///
/// `a0` must contain this hart's hardware ID. `PER_CORE` must have been fully
/// published, and each secondary hart may enter exactly once.
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn secondary_install(_hart_id: usize) -> ! {
    naked_asm!(
        // Naked assembly is emitted without the crate target features. Enable
        // the A/M instructions used by this small allocation trampoline only
        // within this option scope.
        ".option push",
        ".option arch, +a, +m",

        "la t0, {next_index}",
        "li t1, 1",
        "amoadd.d.aqrl t1, t1, (t0)",
        "la t0, {per_core_count}",
        "ld t2, 0(t0)",
        "bgeu t1, t2, 2f",
        "la t0, {per_core}",
        "ld tp, 0(t0)",
        // PerCore index N may use the shared stack after N-1 secondaries have
        // reached their permanent idle stacks.
        "addi t3, t1, -1",
        "li t0, {per_core_size}",
        "mul t1, t1, t0",
        "add tp, tp, t1",
        "sd a0, {hart_id}(tp)",
        // Wait stacklessly for the preceding ticket to publish completion.
        "la t0, {secondary_online}",

        "1:",
        "ld t2, 0(t0)",
        "bne t2, t3, 1b",
        "fence r, rw",
        "la sp, {secondary_stack_top}",
        "tail {secondary_init}",

        "2:",
        // No PerCore slot means the early-boot contract is irrecoverably
        // broken. This hart has neither a stack nor an initialized trap vector,
        // so fail immediately with an illegal instruction.
        "unimp",

        ".option pop",
        next_index = sym NEXT_PER_CORE_INDEX,
        per_core = sym PER_CORE,
        per_core_count = sym PER_CORE_COUNT,
        secondary_online = sym crate::kernel::init::SECONDARY_ONLINE,
        secondary_stack_top = sym secondary_stack_top,
        per_core_size = const size_of::<PerCore>(),
        hart_id = const offset_of!(PerCore, hart_id),
        secondary_init = sym crate::kernel::init::secondary_init,
    )
}
