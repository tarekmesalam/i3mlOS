//! The split virtqueue: a descriptor table, a ring the driver fills, and a
//! ring the device fills back.
//!
//! Two rules govern every line here, and both are about a second processor
//! reading our memory while we write it:
//!
//! * **Volatile, always.** The device is not the compiler's business. Every
//!   shared word is read and written volatilely, or the optimizer will cache
//!   values that another agent of the machine is changing.
//! * **Barriers where the spec says.** Descriptors must be visible before the
//!   index that publishes them, and the used index must be read before the
//!   entries it covers.

use core::sync::atomic::{fence, Ordering};

use nawa_core::uefi::PAGE_SIZE;
use nawa_core::{mem, mmio};

pub const DESCRIPTOR_NEXT: u16 = 1;
pub const DESCRIPTOR_WRITE: u16 = 2;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Descriptor {
    pub address: u64,
    pub length: u32,
    pub flags: u16,
    pub next: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct UsedElement {
    pub id: u32,
    pub length: u32,
}

/// One queue's memory. Each ring gets its own frame: virtio 1.0 lets the
/// three areas live at separate addresses, so there is no reason to compute
/// fragile offsets into a single allocation.
pub struct Queue {
    pub size: u16,
    descriptors: u64,
    available: u64,
    used: u64,
    /// Next descriptor slot to hand out.
    next_descriptor: u16,
    /// Where we last saw the device's used index.
    last_used: u16,
    /// Set when a request was abandoned before the device answered.
    ///
    /// After a timeout the device still owns descriptors we handed it, and it
    /// may answer later. Reusing those slots would let it write into a newer
    /// request's buffers, and accepting the late completion would attribute
    /// one request's result to another. Neither is a risk worth taking for a
    /// disk, so the queue fails closed and says so.
    broken: bool,
}

const MAX_QUEUE_SIZE: u16 = 128;

impl Queue {
    /// Allocate the three rings. `size` must be a power of two, as the ring
    /// arithmetic assumes.
    pub fn new(size: u16) -> Option<Queue> {
        if size == 0 || size > MAX_QUEUE_SIZE || size & (size - 1) != 0 {
            return None;
        }
        let descriptors = mem::allocate_frames(1)? as u64;
        let available = mem::allocate_frames(1)? as u64;
        let used = mem::allocate_frames(1)? as u64;
        for frame in [descriptors, available, used] {
            mmio::zero(frame, PAGE_SIZE);
        }
        Some(Queue {
            size,
            descriptors,
            available,
            used,
            next_descriptor: 0,
            last_used: 0,
            broken: false,
        })
    }

    pub fn descriptor_address(&self) -> u64 {
        self.descriptors
    }

    pub fn available_address(&self) -> u64 {
        self.available
    }

    pub fn used_address(&self) -> u64 {
        self.used
    }

    fn write_descriptor(&self, index: u16, descriptor: Descriptor) {
        let slot = self.descriptors + index as u64 * core::mem::size_of::<Descriptor>() as u64;
        mmio::write_struct(slot, descriptor);
    }

    /// available.flags / available.idx / available.ring[]
    fn available_index(&self) -> u16 {
        mmio::read16(self.available + 2)
    }

    fn set_available_index(&self, value: u16) {
        mmio::write16(self.available + 2, value);
    }

    fn set_available_ring(&self, position: u16, descriptor: u16) {
        mmio::write16(self.available + 4 + (position % self.size) as u64 * 2, descriptor);
    }

    fn used_index(&self) -> u16 {
        mmio::read16(self.used + 2)
    }

    fn used_element(&self, position: u16) -> UsedElement {
        // used: flags(u16), idx(u16), then the ring — 4-byte aligned.
        let ring = self.used + 4;
        mmio::read_struct(ring + (position % self.size) as u64 * 8)
    }

    /// Publish a chain of buffers. Returns the head descriptor index, which
    /// is what the device will name when it is finished.
    pub fn submit(&mut self, buffers: &[(u64, u32, bool)]) -> Option<u16> {
        if self.broken || buffers.is_empty() || buffers.len() > self.size as usize {
            return None;
        }
        let head = self.next_descriptor;
        let mut index = head;
        for (position, (address, length, device_writes)) in buffers.iter().enumerate() {
            let last = position + 1 == buffers.len();
            let next = if last { 0 } else { (index + 1) % self.size };
            let mut flags = 0;
            if *device_writes {
                flags |= DESCRIPTOR_WRITE;
            }
            if !last {
                flags |= DESCRIPTOR_NEXT;
            }
            self.write_descriptor(index, Descriptor {
                address: *address,
                length: *length,
                flags,
                next,
            });
            index = next;
        }
        self.next_descriptor = (head + buffers.len() as u16) % self.size;

        let available = self.available_index();
        self.set_available_ring(available, head);
        // The descriptors must be visible before the index that publishes
        // them; the device may be looking at this instant.
        fence(Ordering::SeqCst);
        self.set_available_index(available.wrapping_add(1));
        Some(head)
    }

    /// Has the device finished anything? Returns (head descriptor, bytes).
    pub fn take_used(&mut self) -> Option<(u16, u32)> {
        let used = self.used_index();
        if used == self.last_used {
            return None;
        }
        // Read the index before the entry it covers.
        fence(Ordering::SeqCst);
        let element = self.used_element(self.last_used);
        self.last_used = self.last_used.wrapping_add(1);
        Some((element.id as u16, element.length))
    }

    /// Spin until **this** request completes, or until `attempts` runs out.
    ///
    /// The head descriptor is checked, not assumed: a used entry names the
    /// request it belongs to, and a driver that ignores that name will
    /// eventually hand one request's answer to another. A device that never
    /// answers must not become a dead machine either — so the wait is bounded,
    /// and giving up marks the queue broken rather than pretending the
    /// outstanding request went away.
    pub fn wait_for(&mut self, head: u16, attempts: u64) -> Option<u32> {
        for _ in 0..attempts {
            match self.take_used() {
                Some((id, length)) if id == head => return Some(length),
                Some(_) => {
                    // A completion for something else: the queue is no longer
                    // in a state this driver can reason about.
                    self.broken = true;
                    return None;
                }
                None => core::hint::spin_loop(),
            }
        }
        self.broken = true;
        None
    }

    /// Has this queue been abandoned mid-request?
    pub fn is_broken(&self) -> bool {
        self.broken
    }
}
