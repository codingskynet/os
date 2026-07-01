pub mod addr;
pub mod buddy;
pub mod page;
pub mod slab;

use crate::mm::buddy::BuddyAllocator;
use crate::util::Global;

pub static BUDDY: Global<BuddyAllocator> = Global::new(BuddyAllocator::empty());
