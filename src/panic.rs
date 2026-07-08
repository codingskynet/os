use core::panic::PanicInfo;

use crate::println;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("kernel panic");

    if let Some(location) = info.location() {
        println!(
            "  at {}:{}:{}",
            location.file(),
            location.line(),
            location.column()
        );
    } else {
        println!("  at <unknown>");
    }

    println!("  message: {}", info.message());

    #[allow(clippy::empty_loop)]
    loop {
        core::hint::spin_loop();
    }
}
