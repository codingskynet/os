use crate::arch::trap::TrapFrame;
use crate::arch::{Exception, PageFaultReason};
use crate::debug;
use crate::kernel::syscall::{self, Syscall};

pub fn handle_exception(frame: &mut TrapFrame, exception: Exception) {
    match exception {
        Exception::PageFault(reason) => handle_page_fault(frame, reason),
        Exception::EnvironmentCallFromUMode => handle_ecall_from_umode(frame),
        _ => panic!(
            "unhandled exception: {:?}, sepc={}, stval={:#x}",
            exception, frame.sepc, frame.stval
        ),
    }
}

fn handle_page_fault(frame: &mut TrapFrame, reason: PageFaultReason) {
    let fault_addr = reason.addr();

    crate::debug!(
        "page fault: {:?}, sepc={}, fault_addr={}, sstatus={:?}",
        reason,
        frame.sepc,
        fault_addr,
        frame.sstatus,
    );

    #[cfg(feature = "smoke-page-fault")]
    {
        if fault_addr.as_raw() == crate::debug::smoke::page_fault::PAGE_FAULT_SMOKE_ADDR {
            frame.sepc = frame.sepc.offset(4usize);
            return;
        }
    }

    panic!(
        "unhandled page fault: {:?}, sepc={}, fault_addr={}",
        reason, frame.sepc, fault_addr
    );
}

fn handle_ecall_from_umode(frame: &mut TrapFrame) {
    let syscall = Syscall::from(&frame.regs);
    debug!("user program calls {syscall:?}");

    (frame.regs.a0, frame.regs.a1) = syscall::handle(syscall);
    frame.sepc = frame.sepc.offset(4usize);
}
