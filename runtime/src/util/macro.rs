//! Composable `asm!` wrapper, plus small declarative helpers.
//!
//! # `asm!`
//!
//! Drop-in name for `core::arch::asm`. Call sites should `use crate::asm`
//! instead of `use core::arch::asm`. Keep using `core::arch::naked_asm` for
//! naked functions; fragments still go through `asm!(@asm_lines(...))`.
//!
//! The top-level interface matches core: comma-separated template strings,
//! then operands / `options(...)`. Core joins adjacent template arguments
//! with newlines; nested fragments must therefore expand to a *single*
//! string and put `\n` between their own instructions.
//!
//! ## Nesting protocol
//!
//! `@asm_lines` takes comma-separated items. Each item is either:
//! - a string literal — used as one instruction, or
//! - a `(part, part, ...)` tuple — parts are `concat!`enated into one instruction.
//!
//! A trailing `\n` is appended to each instruction so the fragment fills one
//! outer template slot:
//!
//! ```ignore
//! use core::arch::naked_asm;
//!
//! naked_asm!(
//!     "csrr t0, sstatus",
//!     store_regs!("a0"),   // → one string with internal `\n`s
//!     "ret",
//!     ra = const 0,
//! );
//!
//! // fragment:
//! macro_rules! store_regs {
//!     ($base:literal) => {
//!         $crate::asm!(@asm_lines(
//!             ("sd ra, {ra}(", $base, ")"),
//!             ("sd sp, {sp}(", $base, ")"),
//!         ))
//!     };
//! }
//! ```

/// Expand one `@asm_lines` item: a string literal or a `(parts…)` tuple.
#[macro_export]
#[doc(hidden)]
macro_rules! __asm_item {
    ($line:literal) => {
        $line
    };
    // Paste all tokens inside the tuple into `concat!`, including commas, so
    // calls like `stringify!($reg)` stay intact (they are not a single `:tt`).
    (($($part:tt)*)) => {
        ::core::concat!($($part)*)
    };
}

/// Nested-friendly wrapper around [`core::arch::asm`].
///
/// Top-level `$($t:tt)*` forwards unchanged (same `,`-separated interface as
/// core). Use `@asm_lines` from fragment macros.
#[macro_export]
macro_rules! asm {
    // Fragment output: `str` or `(parts…)` per instruction, `\n` after each.
    (@asm_lines( $($item:tt),+ $(,)? )) => {
        ::core::concat!($($crate::__asm_item!($item), "\n"),+)
    };

    ($($t:tt)*) => {
        ::core::arch::asm!($($t)*)
    };
}

// Map numeric codes to enum variants, with optional payload bindings.
// Used for trap cause decoding (`scause` + `stval`) and for syscall number
// decoding (register arguments).

#[macro_export]
macro_rules! args_enum {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $name:ident (
            $code_ty:ty
            $(, $arg:ident : $arg_ty:ty)*
            $(,)?
        ) {
            $($arms:tt)*
        }
    ) => {
        args_enum!(@parse
            meta: [$(#[$enum_meta])*],
            vis: [$vis],
            name: [$name],
            code_ty: [$code_ty],
            args: [$($arg: $arg_ty),*],
            variants: [],
            matches: [],
            rest: [$($arms)*],
        );
    };

    // Nested multi-code arm: Variant(Type) { code => expr, ... },
    (@parse
        meta: [$($meta:tt)*],
        vis: [$vis:vis],
        name: [$name:ident],
        code_ty: [$code_ty:ty],
        args: [$($arg:ident : $arg_ty:ty),*],
        variants: [$($variants:tt)*],
        matches: [$($matches:tt)*],
        rest: [
            $(#[$variant_meta:meta])*
            $variant:ident($value_ty:ty) {
                $($code:literal => $value:expr),+ $(,)?
            },
            $($rest:tt)*
        ],
    ) => {
        args_enum!(@parse
            meta: [$($meta)*],
            vis: [$vis],
            name: [$name],
            code_ty: [$code_ty],
            args: [$($arg: $arg_ty),*],
            variants: [
                $($variants)*
                $(#[$variant_meta])*
                $variant($value_ty),
            ],
            matches: [
                $($matches)*
                $(
                    $code => Self::$variant($value),
                )+
            ],
            rest: [$($rest)*],
        );
    };

    // Simple arm: code => Variant or code => Variant(Type = expr)
    (@parse
        meta: [$($meta:tt)*],
        vis: [$vis:vis],
        name: [$name:ident],
        code_ty: [$code_ty:ty],
        args: [$($arg:ident : $arg_ty:ty),*],
        variants: [$($variants:tt)*],
        matches: [$($matches:tt)*],
        rest: [
            $(#[$variant_meta:meta])*
            $code:literal => $variant:ident $(($value_ty:ty = $value:expr))?,
            $($rest:tt)*
        ],
    ) => {
        args_enum!(@parse
            meta: [$($meta)*],
            vis: [$vis],
            name: [$name],
            code_ty: [$code_ty],
            args: [$($arg: $arg_ty),*],
            variants: [
                $($variants)*
                $(#[$variant_meta])*
                $variant $(($value_ty))?,
            ],
            matches: [
                $($matches)*
                $code => Self::$variant $(($value))?,
            ],
            rest: [$($rest)*],
        );
    };

    (@parse
        meta: [$($meta:tt)*],
        vis: [$vis:vis],
        name: [$name:ident],
        code_ty: [$code_ty:ty],
        args: [$($arg:ident : $arg_ty:ty),*],
        variants: [$($variants:tt)*],
        matches: [$($matches:tt)*],
        rest: [],
    ) => {
        $($meta)*
        $vis enum $name {
            $($variants)*
            Unknown($code_ty),
        }

        impl $name {
            /// Decode `code`, using the bound payload names in variant expressions.
            #[allow(unused_variables)]
            $vis fn new(code: $code_ty $(, $arg: $arg_ty)*) -> Self {
                match code {
                    $($matches)*
                    _ => Self::Unknown(code),
                }
            }
        }
    };
}
