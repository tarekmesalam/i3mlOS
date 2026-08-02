//! Kernel heap: a first-fit free-list allocator over frames from `mem`,
//! registered as the global allocator so safe kernel code gets `alloc`
//! (Vec, String, format!) with zero third-party code underneath.
//!
//! M0 honesty: freed blocks are reinserted but not coalesced yet —
//! fragmentation is bounded by boot-time usage and fixed in the slab
//! milestone (master plan §3). All bookkeeping runs on a 16-byte quantum
//! (see `effective_layout`) so every split is representable.

use core::alloc::{GlobalAlloc, Layout};

use crate::cell::SpinLock;
use crate::mem;
use crate::uefi::PAGE_SIZE;

const HEAP_PAGES: usize = 1024; // 4 MiB

/// A free block header, stored inside the free memory itself.
struct FreeBlock {
    size: usize,
    next: Option<&'static mut FreeBlock>,
}

struct FreeList {
    head: Option<&'static mut FreeBlock>,
}

static HEAP: SpinLock<FreeList> = SpinLock::new(FreeList { head: None });

/// Carve the initial heap out of the frame allocator. Bring-up only.
pub(crate) fn init() -> bool {
    let Some(base) = mem::allocate_frames(HEAP_PAGES) else {
        return false;
    };
    let size = HEAP_PAGES * PAGE_SIZE;
    HEAP.with(|list| {
        let block = unsafe {
            let block = base as *mut FreeBlock;
            block.write(FreeBlock { size, next: None });
            &mut *block
        };
        list.head = Some(block);
    });
    true
}

const MIN_BLOCK: usize = core::mem::size_of::<FreeBlock>();

/// Every effective size and alignment is a multiple of MIN_BLOCK (16). With a
/// page-aligned initial heap this keeps every block address and size on the
/// 16-byte quantum, so split remainders are always 0 or a representable
/// FreeBlock — no unrepresentable gaps, no leaked slivers (review findings,
/// M0). dealloc recomputes the identical size from the same layout.
fn effective_layout(layout: Layout) -> (usize, usize) {
    let align = layout.align().max(MIN_BLOCK);
    let size = layout.size().max(MIN_BLOCK).next_multiple_of(MIN_BLOCK);
    (size, align)
}

pub struct KernelAllocator;

#[global_allocator]
static GLOBAL: KernelAllocator = KernelAllocator;

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let (size, align) = effective_layout(layout);
        HEAP.with(|list| {
            // First-fit walk. `current` is the link we may splice through.
            let mut link: *mut Option<&'static mut FreeBlock> = &mut list.head;
            loop {
                let candidate = unsafe { &mut *link };
                let Some(block) = candidate.as_deref_mut() else {
                    return core::ptr::null_mut();
                };
                let block_address = block as *mut FreeBlock as usize;
                let payload = block_address.next_multiple_of(align);
                let padding = payload - block_address;
                // On the 16-byte quantum padding is always 0 or >= MIN_BLOCK;
                // this guard is defense in depth, not a reachable path.
                if padding != 0 && padding < MIN_BLOCK {
                    link = &mut block.next;
                    continue;
                }
                if block.size < padding + size {
                    link = &mut block.next;
                    continue;
                }

                // Take this block out of the list.
                let mut remainder_next = block.next.take();
                let block_size = block.size;

                // Left padding becomes its own free block.
                if padding >= MIN_BLOCK {
                    let pad_block = unsafe {
                        let p = block_address as *mut FreeBlock;
                        p.write(FreeBlock { size: padding, next: remainder_next.take() });
                        &mut *p
                    };
                    remainder_next = Some(pad_block);
                }
                // Right remainder likewise.
                let used = padding + size;
                if block_size - used >= MIN_BLOCK {
                    let tail = unsafe {
                        let p = (block_address + used) as *mut FreeBlock;
                        p.write(FreeBlock { size: block_size - used, next: remainder_next.take() });
                        &mut *p
                    };
                    remainder_next = Some(tail);
                }
                unsafe { *link = remainder_next };
                return payload as *mut u8;
            }
        })
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        let (size, _) = effective_layout(layout);
        HEAP.with(|list| {
            let block = unsafe {
                let p = pointer as *mut FreeBlock;
                p.write(FreeBlock { size, next: list.head.take() });
                &mut *p
            };
            list.head = Some(block);
        });
    }
}
