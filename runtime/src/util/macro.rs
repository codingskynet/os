//! Declarative macros shared across the runtime.
//!
//! This module provides:
//!
//! - [`crate::asm!`], a composable wrapper around [`core::arch::asm`].
//! - [`crate::nonzero_enum!`], for defining error-code types backed by
//!   [`NonZeroUsize`](core::num::NonZeroUsize).
//! - [`crate::args_enum!`], for decoding numeric codes and their arguments
//!   into enum variants.
//!
//! The public macros are exported at the crate root so call sites can import
//! them directly, for example with `use crate::asm`.

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

/// A composable, drop-in wrapper around [`core::arch::asm`].
///
/// Call sites should import this macro with `use crate::asm` instead of
/// importing `core::arch::asm` directly. Naked functions should continue to
/// use [`core::arch::naked_asm`], while reusable instruction fragments can be
/// constructed with `asm!(@asm_lines(...))`.
///
/// # Top-level interface
///
/// Regular invocations are forwarded unchanged, preserving the core macro's
/// comma-separated template strings, operands, and `options(...)` interface.
/// Core joins adjacent template strings with newlines, so a nested fragment
/// must expand to one string containing its own `\n` separators.
///
/// # Composable instruction fragments
///
/// `@asm_lines` accepts comma-separated items. Each item is either:
///
/// - A string literal representing one instruction.
/// - A `(part, part, ...)` tuple whose parts are concatenated into one
///   instruction.
///
/// Each generated instruction ends with `\n`, allowing the complete fragment
/// to occupy one outer assembly template slot.
///
/// ```ignore
/// use core::arch::naked_asm;
///
/// macro_rules! store_regs {
///     ($base:literal) => {
///         $crate::asm!(@asm_lines(
///             ("sd ra, {ra}(", $base, ")"),
///             ("sd sp, {sp}(", $base, ")"),
///         ))
///     };
/// }
///
/// naked_asm!(
///     "csrr t0, sstatus",
///     store_regs!("a0"),
///     "ret",
///     ra = const 0,
///     sp = const 8,
/// );
/// ```
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

/// Define an error-code type whose every value is a
/// [`NonZeroUsize`](core::num::NonZeroUsize).
///
/// The generated type is transparent over `NonZeroUsize`, and each listed
/// error is exposed as an associated constant so call sites can continue to
/// use enum-like paths such as `Error::InvalidBuffer`.  Error codes are
/// constructed in a const context; assigning zero therefore fails to compile.
///
/// ```ignore
/// nonzero_enum! {
///     #[derive(Clone, Copy, Debug, PartialEq, Eq)]
///     pub struct Error {
///         InvalidBuffer = 1,
///         BadFileDescriptor = 2,
///     }
/// }
/// ```
#[macro_export]
macro_rules! nonzero_enum {
    (
        $(#[$type_meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$error_meta:meta])*
                $error:ident = $code:expr
            ),+ $(,)?
        }
    ) => {
        $(#[$type_meta])*
        #[repr(transparent)]
        $vis struct $name(::core::num::NonZeroUsize);

        #[allow(non_upper_case_globals)]
        impl $name {
            $(
                $(#[$error_meta])*
                $vis const $error: Self = Self(
                    ::core::num::NonZeroUsize::new($code)
                        .expect("error code must be nonzero"),
                );
            )+

            $vis const fn code(self) -> ::core::num::NonZeroUsize {
                self.0
            }
        }

        impl ::core::convert::From<$name> for ::core::num::NonZeroUsize {
            fn from(error: $name) -> Self {
                error.code()
            }
        }
    };
}

/// Defines an enum that decodes numeric codes and optional argument payloads.
///
/// Each declared code maps to a generated enum variant, while unmatched codes
/// are preserved in an `Unknown` variant. Variants can bind values derived
/// from the arguments passed to the generated `new` function.
///
/// This is used for trap-cause decoding from `scause` and `stval`, and for
/// decoding syscall numbers and register arguments.
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
