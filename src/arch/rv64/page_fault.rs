use crate::arch::trap::{Exception, TrapFrame};

pub fn handle_page_fault(frame: &mut TrapFrame, exception: Exception) {
    let fault_addr = match exception {
        Exception::InstructionPageFault(addr)
        | Exception::LoadPageFault(addr)
        | Exception::StorePageFault(addr) => addr,
        _ => return,
    };

    crate::debug!(
        "page fault: {:?}, sepc={}, fault_addr={}, sstatus={:?}",
        exception,
        frame.sepc,
        fault_addr,
        frame.sstatus,
    );

    #[cfg(feature = "smoke-page-fault")]
    {
        if fault_addr == crate::debug::smoke::page_fault::PAGE_FAULT_SMOKE_ADDR {
            frame.sepc = frame.sepc.checked_offset(4).unwrap();
            return;
        }
    }

    panic!(
        "unhandled page fault: {:?}, sepc={}, fault_addr={}",
        exception, frame.sepc, fault_addr
    );
}
