use crate::arch::trap::{Exception, TrapFrame};
use crate::debug;
use crate::kernel::syscall::Syscall;
use crate::kernel::thread::Thread;
use crate::mm::addr::Va;

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PageFaultReason {
    Instruction(Va),
    LoadPage(Va),
    StorePage(Va),
}

impl PageFaultReason {
    fn addr(&self) -> Va {
        match self {
            PageFaultReason::Instruction(addr) => *addr,
            PageFaultReason::LoadPage(addr) => *addr,
            PageFaultReason::StorePage(addr) => *addr,
        }
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

    Thread::with_current(|thread| (frame.regs.a0, frame.regs.a1) = thread.syscall(syscall));

    frame.sepc = frame.sepc.offset(4usize);
}
