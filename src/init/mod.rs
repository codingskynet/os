use crate::println;

pub fn kernel_init() {
    #[cfg(feature = "fuzz-allocator")]
    {
        use crate::debug::dump_page_list;
        use crate::mm::BUDDY;

        dump_page_list();
        println!("{:#?}", *BUDDY.lock());
        crate::debug::fuzz::allocator::run();
        dump_page_list();
        println!("{:#?}", *BUDDY.lock());
    }

    println!("hello, init!");
}
