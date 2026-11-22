//! Hardware drivers available to board boot code.
#![no_std]

pub mod interrupt {
    automod::dir!(pub "src/interrupt");
}

pub mod timer {
    automod::dir!(pub "src/timer");
}

pub mod uart {
    automod::dir!(pub "src/uart");
}

include!(concat!(env!("OUT_DIR"), "/drivers.rs"));
