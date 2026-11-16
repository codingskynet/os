//! Select either target's real address constants for host-side conversion tests.
#[cfg(feature = "rv32-layout")]
#[path = "../../runtime/src/arch/rv32/consts.rs"]
pub mod consts;
#[cfg(not(feature = "rv32-layout"))]
#[path = "../../runtime/src/arch/rv64/consts.rs"]
pub mod consts;
