//! Register save layouts shared by trap and context-switch code.

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
    /// General-purpose registers saved on trap entry.
    #[derive(Default)]
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
    pub struct CalleeSavedRegs {
        s0, s1, s2, s3, s4, s5, s6, s7, s8, s9, s10, s11,
    }
}
