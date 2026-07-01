use crate::arch::consts::PAGE_SIZE;
use crate::mm::addr::Pa;
use crate::mm::page::{PageMeta, Status};
use crate::println;

pub fn dump_page_list(page_meta: &PageMeta) {
    let pages = page_meta.pages();
    if pages.is_empty() {
        println!("page metadata: empty");
        return;
    }

    println!("page metadata: {} pages", pages.len());

    let mut start = pages[0].addr;
    let mut status = pages[0].status;
    for (_, page) in pages.iter().enumerate().skip(1) {
        if page.status != status {
            dump_page_range(start, page.addr, status);
            start = page.addr;
            status = page.status;
        }
    }
    dump_page_range(
        start,
        pages[pages.len() - 1]
            .addr
            .checked_offset(PAGE_SIZE.get())
            .unwrap(),
        status,
    );
}

fn dump_page_range(start: Pa, end: Pa, status: Status) {
    println!(
        "  addr {}..{}: {} ({} pages)",
        start,
        end,
        status,
        (end.as_raw() - start.as_raw()) / PAGE_SIZE
    );
}
