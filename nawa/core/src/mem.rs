//! Physical-frame allocator: a bitmap over the largest conventional region
//! from the UEFI memory map (master plan §3: bitmap → linked-list → slab).
//!
//! Deliberately conservative for M0: only EFI_CONVENTIONAL_MEMORY is touched.
//! Boot-services memory is reclaimable in principle, but the firmware's page
//! tables — which we still run on — live there; reclaiming is a later
//! milestone, after we build our own tables.

use crate::cell::SpinLock;
use crate::uefi::{MemoryMap, EFI_CONVENTIONAL_MEMORY, PAGE_SIZE};

pub struct MemoryStats {
    pub usable_bytes: u64,
    pub managed_bytes: u64,
}

struct BitmapAllocator {
    /// Physical address of the first managed frame.
    base: usize,
    /// Total frames managed (bits in the bitmap).
    frames: usize,
    /// The bitmap itself lives at the START of the managed region;
    /// its own frames are pre-marked as used.
    bitmap: *mut u8,
    next_hint: usize,
}

static ALLOCATOR: SpinLock<Option<BitmapAllocator>> = SpinLock::new(None);

/// Initialize from the final memory map. Single-threaded bring-up.
pub(crate) fn init(map: MemoryMap) -> MemoryStats {
    let mut usable_bytes = 0u64;
    let mut largest_base = 0u64;
    let mut largest_pages = 0u64;
    for region in map.regions() {
        if region.r#type == EFI_CONVENTIONAL_MEMORY {
            usable_bytes += region.pages * PAGE_SIZE as u64;
            if region.pages > largest_pages {
                largest_pages = region.pages;
                largest_base = region.physical_start;
            }
        }
    }

    let frames = largest_pages as usize;
    let bitmap_bytes = frames.div_ceil(8);
    let bitmap_frames = bitmap_bytes.div_ceil(PAGE_SIZE);
    let bitmap = largest_base as *mut u8;
    unsafe {
        core::ptr::write_bytes(bitmap, 0, bitmap_bytes);
    }

    let mut allocator = BitmapAllocator {
        base: largest_base as usize,
        frames,
        bitmap,
        next_hint: 0,
    };
    for frame in 0..bitmap_frames {
        allocator.mark_used(frame);
    }
    let managed_bytes = ((frames - bitmap_frames) * PAGE_SIZE) as u64;
    ALLOCATOR.with(|slot| *slot = Some(allocator));

    MemoryStats { usable_bytes, managed_bytes }
}

impl BitmapAllocator {
    fn is_used(&self, frame: usize) -> bool {
        unsafe { *self.bitmap.add(frame / 8) & (1 << (frame % 8)) != 0 }
    }

    fn mark_used(&mut self, frame: usize) {
        unsafe { *self.bitmap.add(frame / 8) |= 1 << (frame % 8) }
    }

    fn mark_free(&mut self, frame: usize) {
        unsafe { *self.bitmap.add(frame / 8) &= !(1 << (frame % 8)) }
    }

    /// First-fit search for `count` contiguous free frames.
    fn allocate_contiguous(&mut self, count: usize) -> Option<usize> {
        let mut run_start = self.next_hint;
        let mut run_length = 0;
        for frame in self.next_hint..self.frames {
            if self.is_used(frame) {
                run_start = frame + 1;
                run_length = 0;
            } else {
                run_length += 1;
                if run_length == count {
                    for f in run_start..=frame {
                        self.mark_used(f);
                    }
                    self.next_hint = frame + 1;
                    return Some(self.base + run_start * PAGE_SIZE);
                }
            }
        }
        // Wrap once from the beginning before giving up.
        if self.next_hint != 0 {
            self.next_hint = 0;
            return self.allocate_contiguous(count);
        }
        None
    }
}

/// Allocate `count` contiguous physical frames (identity-mapped for now:
/// we still run on the firmware's full-RAM mapping until we own paging).
pub fn allocate_frames(count: usize) -> Option<usize> {
    ALLOCATOR.with(|slot| slot.as_mut().and_then(|a| a.allocate_contiguous(count)))
}

/// Return frames to the allocator. This is a safe API, so it must be safe to
/// MISUSE: anything inconsistent (foreign address, misalignment, range
/// overflow, double-free) is a kernel bug and panics loudly rather than
/// corrupting the bitmap or neighboring memory (review finding, M0).
pub fn free_frames(physical_address: usize, count: usize) {
    ALLOCATOR.with(|slot| {
        let Some(a) = slot.as_mut() else {
            panic!("free_frames before mem::init");
        };
        assert!(count > 0, "free_frames: empty range");
        assert!(
            physical_address % PAGE_SIZE == 0,
            "free_frames: unaligned address {physical_address:#x}"
        );
        let first = physical_address
            .checked_sub(a.base)
            .map(|offset| offset / PAGE_SIZE)
            .unwrap_or_else(|| panic!("free_frames: {physical_address:#x} below managed region"));
        let end = first
            .checked_add(count)
            .filter(|&end| end <= a.frames)
            .unwrap_or_else(|| panic!("free_frames: range past managed region"));
        for frame in first..end {
            assert!(a.is_used(frame), "free_frames: double free of frame {frame}");
            a.mark_free(frame);
        }
    });
}
