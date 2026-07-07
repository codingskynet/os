use crate::arch::switch::_kernel_thread_trampoline;
use crate::mm::addr::Va;

macro_rules! regs {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident { $($reg:ident),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[repr(C)]
        $vis struct $name {
            $(
                pub $reg: usize,
            )+
        }
    };
}

regs! {
    #[derive(Default)]
    pub struct GeneralRegs {
        ra, sp, gp, tp,
        a0, a1, a2, a3, a4, a5, a6, a7,
        t0, t1, t2, t3, t4, t5, t6,
        s0, s1, s2, s3, s4, s5, s6, s7, s8, s9, s10, s11,
    }
}

impl GeneralRegs {
    pub fn as_kernel_thread_trampoline(&mut self, sp: Va, entry: Va) {
        self.ra = _kernel_thread_trampoline as *const () as usize;
        self.sp = sp.as_raw();
        self.a0 = entry.as_raw();
    }
}
