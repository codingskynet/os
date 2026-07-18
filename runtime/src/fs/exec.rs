use alloc::collections::btree_map::BTreeMap;
use alloc::vec::Vec;
use core::convert::Infallible;
use core::ptr;

use elf::ElfBytes;
use elf::abi::{EM_RISCV, ET_EXEC, PF_R, PF_W, PF_X, PT_LOAD};
use elf::endian::LittleEndian;
use elf::file::Class;

use super::{Error, Result};
use crate::arch;
use crate::arch::consts::{LOWER_CANONICAL_END, PAGE_SIZE};
use crate::arch::paging::Permission;
use crate::fs::{AbsolutePath, MOUNTS};
use crate::kernel::thread::Thread;
use crate::mm::addr::{Uva, Va};
use crate::mm::{MmContext, Pages};
use crate::util::consts::M;

const USER_STACK_PAGES: usize = 16;
const USER_STACK_TOP: usize = LOWER_CANONICAL_END - PAGE_SIZE.get();
const USER_STACK_START: usize = USER_STACK_TOP - USER_STACK_PAGES * PAGE_SIZE.get();

// TODO: Move the maximum stack size and guard size into process configuration.
const USER_STACK_MAX_SIZE: usize = 8 * M;
const USER_STACK_GUARD_SIZE: usize = 256 * PAGE_SIZE.get();
const USER_STACK_LIMIT: usize = USER_STACK_TOP - USER_STACK_MAX_SIZE;
const USER_STACK_GUARD_START: usize = USER_STACK_LIMIT - USER_STACK_GUARD_SIZE;

pub fn kernel_exec(path: &str) -> Result<Infallible> {
    let image = read_all(path)?;
    let (mm, entry) = load_elf(&image)?;
    drop(image);

    Thread::replace_current_mm(mm);

    // SAFETY: the ELF entry was checked to lie in an executable PT_LOAD,
    // the stack and load pages are present with U permissions, and `mm` is now
    // the current thread's active, owned address space.
    unsafe { arch::trap::enter_user(entry, Va::new(USER_STACK_TOP)) }
}

#[allow(clippy::large_stack_frames)]
fn load_elf(image: &[u8]) -> Result<(MmContext, Va)> {
    let elf = ElfBytes::<LittleEndian>::minimal_parse(image)?;

    if elf.ehdr.class != Class::ELF64
        || elf.ehdr.e_type != ET_EXEC
        || elf.ehdr.e_machine != EM_RISCV
    {
        return Err(Error::InvalidExecutable);
    }

    let segments = elf.segments().ok_or(Error::InvalidExecutable)?;
    let mut allocations = BTreeMap::<Uva, (Pages, Permission)>::new();
    let mut entry_is_executable = false;

    for phdr in segments.iter() {
        if phdr.p_type != PT_LOAD {
            continue;
        }
        if phdr.p_filesz > phdr.p_memsz {
            return Err(Error::InvalidExecutable);
        }
        if phdr.p_align > 1
            && (!phdr.p_align.is_power_of_two()
                || phdr.p_vaddr % phdr.p_align != phdr.p_offset % phdr.p_align)
        {
            return Err(Error::InvalidExecutable);
        }

        let start = usize::try_from(phdr.p_vaddr).map_err(|_| Error::InvalidExecutable)?;
        let mem_size = usize::try_from(phdr.p_memsz).map_err(|_| Error::InvalidExecutable)?;
        let end = start
            .checked_add(mem_size)
            .filter(|end| start >= PAGE_SIZE.get() && *end <= USER_STACK_GUARD_START)
            .ok_or(Error::InvalidExecutable)?;
        let entry = usize::try_from(elf.ehdr.e_entry).map_err(|_| Error::InvalidExecutable)?;
        if phdr.p_flags & PF_X != 0 && (start..end).contains(&entry) {
            entry_is_executable = true;
        }

        let permissions = permissions(phdr.p_flags);
        if start < end && permissions.is_empty() {
            return Err(Error::InvalidExecutable);
        }
        ensure_pages(&mut allocations, start, end, permissions)?;

        let data = elf
            .segment_data(&phdr)
            .map_err(|_| Error::InvalidExecutable)?;
        copy_to_pages(&allocations, start, data)?;
    }

    if !entry_is_executable {
        return Err(Error::InvalidExecutable);
    }

    ensure_pages(
        &mut allocations,
        USER_STACK_START,
        USER_STACK_TOP,
        Permission::R | Permission::W,
    )?;

    let mut mm = MmContext::new();
    for (addr, (pages, permissions)) in allocations {
        mm.map_user_page(addr.into_va(), pages, permissions)
            .map_err(|_| Error::InvalidExecutable)?;
    }

    let entry = Va::new(usize::try_from(elf.ehdr.e_entry).map_err(|_| Error::InvalidExecutable)?);
    Ok((mm, entry))
}

fn read_all(path: &str) -> Result<Vec<u8>> {
    let node = {
        let guard = MOUNTS.lock();
        let mounts = guard.as_ref().ok_or(Error::NotFound)?;

        let root = mounts.get(AbsolutePath::ROOT).ok_or(Error::NotFound)?;
        root.open(&AbsolutePath::ROOT.join(path))
    }?;
    let mut image = Vec::new();
    let mut offset = 0;
    loop {
        let start = image.len();
        image.resize(start + PAGE_SIZE.get(), 0);
        let read = node.read(offset, &mut image[start..]);
        offset += read;
        image.truncate(start + read);
        if read < PAGE_SIZE.get() {
            return Ok(image);
        }
    }
}

fn permissions(flags: u32) -> Permission {
    let mut permissions = Permission::empty();
    if flags & PF_R != 0 {
        permissions |= Permission::R;
    }
    if flags & PF_W != 0 {
        permissions |= Permission::W;
    }
    if flags & PF_X != 0 {
        permissions |= Permission::X;
    }
    permissions
}

fn ensure_pages(
    pages: &mut BTreeMap<Uva, (Pages, Permission)>,
    start: usize,
    end: usize,
    permissions: Permission,
) -> Result<()> {
    if start > end {
        return Err(Error::InvalidExecutable);
    }
    let mut addr = Uva::new(start)
        .ok_or(Error::InvalidExecutable)?
        .align_down(PAGE_SIZE);
    while addr.as_raw() < end {
        if let Some((_, page_permissions)) = pages.get_mut(&addr) {
            *page_permissions |= permissions;
        } else {
            let page = Pages::new(PAGE_SIZE).ok_or(Error::OutOfMemory)?;
            // A newly mapped anonymous/load page must expose zeroed BSS rather
            // than stale allocator contents.
            unsafe { ptr::write_bytes(page.as_mut_ptr::<u8>(), 0, PAGE_SIZE.get()) };
            pages.insert(addr, (page, permissions));
        }
        addr = addr.offset(PAGE_SIZE);
    }
    Ok(())
}

fn copy_to_pages(
    pages: &BTreeMap<Uva, (Pages, Permission)>,
    start: usize,
    data: &[u8],
) -> Result<()> {
    let mut copied = 0;
    while copied < data.len() {
        let destination = start.checked_add(copied).ok_or(Error::InvalidExecutable)?;
        let page_addr = Uva::new(destination)
            .ok_or(Error::InvalidExecutable)?
            .align_down(PAGE_SIZE);
        let (page, _) = pages.get(&page_addr).ok_or(Error::InvalidExecutable)?;
        let offset = destination - page_addr.as_raw();
        let len = (PAGE_SIZE.get() - offset).min(data.len() - copied);
        unsafe {
            ptr::copy_nonoverlapping(
                data[copied..].as_ptr(),
                page.as_mut_ptr::<u8>().add(offset),
                len,
            );
        }
        copied += len;
    }
    Ok(())
}
