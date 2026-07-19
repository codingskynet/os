//! Register save layouts shared by trap and context-switch code.

macro_rules! regs {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident { $($reg:ident),+ $(,)? }
    ) => {
        regs! {
            $(#[$meta])*
            $vis struct $name(usize) { $($reg),+ }
        }
    };
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident($ty:ty) { $($reg:ident),+ $(,)? }
    ) => {
        $(#[$meta])*
        $vis struct $name {
            $(
                pub $reg: $ty,
            )+
        }
    };
}

regs! {
    /// General-purpose registers saved on trap entry.
    #[derive(Default)]
    #[repr(C)]
    pub struct GeneralRegs {
        ra, sp, gp, tp,
        a0, a1, a2, a3, a4, a5, a6, a7,
        t0, t1, t2, t3, t4, t5, t6,
        s0, s1, s2, s3, s4, s5, s6, s7, s8, s9, s10, s11,
    }
}

regs! {
    /// Callee-saved registers preserved across kernel thread switches.
    #[derive(Default)]
    #[repr(C)]
    pub struct CalleeSavedRegs {
        s0, s1, s2, s3, s4, s5, s6, s7, s8, s9, s10, s11,
    }
}

regs! {
    /// Complete floating-point register bank provided by the RISC-V D extension.
    #[derive(Default)]
    #[repr(C)]
    pub struct FpRegs(u64) {
         f0,  f1,  f2,  f3,  f4,  f5,  f6,  f7,  f8,  f9,
        f10, f11, f12, f13, f14, f15, f16, f17, f18, f19,
        f20, f21, f22, f23, f24, f25, f26, f27, f28, f29,
        f30, f31,
    }
}
