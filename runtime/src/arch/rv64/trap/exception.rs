use crate::arch::trap::{Exception, TrapFrame};
use crate::kernel::syscall::{self, Syscall};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
        if fault_addr == crate::debug::smoke::page_fault::PAGE_FAULT_SMOKE_ADDR {
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
    match Syscall::from(&frame.regs) {
        Syscall::Exit(code) => syscall::exit(code),
        Syscall::Unknown(number) => {
            panic!(
                "unhandled ecall from U-mode: number={}, sepc={}, sstatus={:?}",
                number, frame.sepc, frame.sstatus
            );
        }
    }
    // frame.sepc = frame.sepc.offset(4).unwrap();
}
