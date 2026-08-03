//! virtio-blk — persistence. Until this driver exists, SIJIL is a ring in RAM
//! that a reboot erases, and "what did my computer do while I slept?" has an
//! answer only until the machine is turned off.
//!
//! A request is three descriptors: a header we write, a payload one side
//! writes, and a status byte the device writes. That shape is the whole
//! protocol.

use nawa_core::{mem, mmio};

use crate::queue::Queue;
use crate::transport::{Transport, DEVICE_BLOCK};

pub const SECTOR_SIZE: usize = 512;

const TYPE_READ: u32 = 0;
const TYPE_WRITE: u32 = 1;
const TYPE_FLUSH: u32 = 4;

const STATUS_OK: u8 = 0;

/// Device configuration: capacity in 512-byte sectors, at offset 0.
const CONFIG_CAPACITY: u64 = 0;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RequestHeader {
    kind: u32,
    reserved: u32,
    sector: u64,
}

pub struct Block {
    transport: Transport,
    queue: Queue,
    doorbell: u64,
    /// One frame split into: header (16 bytes), status (1 byte), and a
    /// sector-sized payload area. Device-visible memory has to be memory the
    /// device can reach, so it comes from the frame allocator, not the heap.
    scratch: u64,
    pub sectors: u64,
}

const HEADER_OFFSET: u64 = 0;
const STATUS_OFFSET: u64 = 16;
const PAYLOAD_OFFSET: u64 = 512;

impl Block {
    pub fn open() -> Option<Block> {
        let device = crate::find(DEVICE_BLOCK)?;
        device.enable();
        let transport = Transport::new(&device)?;
        transport.begin(0)?;
        let (queue, doorbell) = transport.setup_queue(0, 8)?;
        transport.ready();
        let scratch = mem::allocate_frames(1)? as u64;
        let sectors = transport.config64(CONFIG_CAPACITY);
        Some(Block { transport, queue, doorbell, scratch, sectors })
    }

    fn submit(&mut self, kind: u32, sector: u64, payload_length: u32, device_writes: bool) -> bool {
        let header = RequestHeader { kind, reserved: 0, sector };
        mmio::write_struct(self.scratch + HEADER_OFFSET, header);
        // 0xff is not a valid status, so a device that never answers cannot
        // be mistaken for one that answered "ok".
        mmio::write8(self.scratch + STATUS_OFFSET, 0xff);
        let chain = [
            (self.scratch + HEADER_OFFSET, 16u32, false),
            (self.scratch + PAYLOAD_OFFSET, payload_length, device_writes),
            (self.scratch + STATUS_OFFSET, 1u32, true),
        ];
        let chain: &[(u64, u32, bool)] =
            if payload_length == 0 { &[chain[0], chain[2]] } else { &chain };
        if self.queue.submit(chain).is_none() {
            return false;
        }
        self.transport.notify(self.doorbell, 0);
        if self.queue.wait(50_000_000).is_none() {
            return false;
        }
        mmio::read8(self.scratch + STATUS_OFFSET) == STATUS_OK
    }

    pub fn read_sector(&mut self, sector: u64, out: &mut [u8]) -> bool {
        if out.len() < SECTOR_SIZE || sector >= self.sectors {
            return false;
        }
        if !self.submit(TYPE_READ, sector, SECTOR_SIZE as u32, true) {
            return false;
        }
        mmio::read_into(self.scratch + PAYLOAD_OFFSET, &mut out[..SECTOR_SIZE]);
        true
    }

    pub fn write_sector(&mut self, sector: u64, data: &[u8]) -> bool {
        if data.len() < SECTOR_SIZE || sector >= self.sectors {
            return false;
        }
        mmio::write_from(self.scratch + PAYLOAD_OFFSET, &data[..SECTOR_SIZE]);
        self.submit(TYPE_WRITE, sector, SECTOR_SIZE as u32, false)
    }

    /// Ask the device to commit. Without this, "written" means "accepted",
    /// which is not the same thing when the power goes out.
    pub fn flush(&mut self) -> bool {
        self.submit(TYPE_FLUSH, 0, 0, false)
    }
}
