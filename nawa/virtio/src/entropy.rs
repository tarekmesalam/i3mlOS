//! virtio-rng — the simplest possible complete driver, and therefore the
//! right one to prove the transport with: one queue, one buffer, one answer.
//!
//! It also matters on its own. Randomness a kernel cannot produce for itself
//! is randomness it must not pretend to have, and every later security
//! decision — capability tokens, journal nonces, TLS — needs a real source.

use nawa_core::uefi::PAGE_SIZE;
use nawa_core::{mem, mmio};

use crate::queue::Queue;
use crate::transport::{Transport, DEVICE_ENTROPY};

pub struct Entropy {
    transport: Transport,
    queue: Queue,
    doorbell: u64,
    /// A frame the device fills. Kernel memory, identity-mapped, so its
    /// physical and virtual addresses are the same number.
    buffer: u64,
}

impl Entropy {
    pub fn open() -> Option<Entropy> {
        let device = crate::find(DEVICE_ENTROPY)?;
        device.enable();
        let transport = Transport::new(&device)?;
        transport.begin(0)?;
        let (queue, doorbell) = transport.setup_queue(0, 8)?;
        transport.ready();
        let buffer = mem::allocate_frames(1)? as u64;
        Some(Entropy { transport, queue, doorbell, buffer })
    }

    /// Fill `out` with entropy from the device. Returns how many bytes the
    /// device actually produced — it is allowed to give fewer than asked.
    pub fn read(&mut self, out: &mut [u8]) -> Option<usize> {
        let wanted = out.len().min(PAGE_SIZE) as u32;
        if wanted == 0 {
            return Some(0);
        }
        let head = self.queue.submit(&[(self.buffer, wanted, true)])?;
        self.transport.notify(self.doorbell, 0);
        let produced = self.queue.wait_for(head, 50_000_000)?;
        // The device chooses this number; clamp it against what we asked for
        // as well as the caller's slice, so an inflated length copies nothing
        // it should not.
        let produced = (produced as usize).min(wanted as usize).min(out.len());
        mmio::read_into(self.buffer, &mut out[..produced]);
        Some(produced)
    }
}
