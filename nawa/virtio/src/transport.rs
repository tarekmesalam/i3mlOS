//! virtio-pci, modern (1.0) — finding a device's registers and bringing it up.
//!
//! The device tells us where its registers are through PCI capabilities, each
//! naming a BAR and an offset. We follow that chain rather than assuming a
//! layout, because assuming layouts is how drivers work on exactly one
//! emulator.

use core::sync::atomic::{fence, Ordering};

use nawa_core::mmio::{read16, read32, read8, write16, write32, write8};
use nawa_core::pci;

use crate::queue::Queue;

pub const VENDOR_VIRTIO: u16 = 0x1af4;

/// A device kind, by both of the PCI IDs it may appear under.
///
/// Modern devices use 0x1040 + type. **Transitional** devices — what QEMU
/// gives you unless told otherwise, and what most VMMs still default to —
/// keep the older, ad-hoc IDs while still offering the virtio 1.0 capability
/// structures this driver uses. Recognising only the modern ID means finding
/// no hardware on the most common configuration there is.
#[derive(Clone, Copy)]
pub struct Kind {
    pub modern: u16,
    pub transitional: u16,
}

pub const DEVICE_NETWORK: Kind = Kind { modern: 0x1041, transitional: 0x1000 };
pub const DEVICE_BLOCK: Kind = Kind { modern: 0x1042, transitional: 0x1001 };
pub const DEVICE_CONSOLE: Kind = Kind { modern: 0x1043, transitional: 0x1003 };
pub const DEVICE_ENTROPY: Kind = Kind { modern: 0x1044, transitional: 0x1005 };

const CAPABILITY_VENDOR_SPECIFIC: u8 = 0x09;
const CONFIG_COMMON: u8 = 1;
const CONFIG_NOTIFY: u8 = 2;
const CONFIG_ISR: u8 = 3;
const CONFIG_DEVICE: u8 = 4;

pub const STATUS_ACKNOWLEDGE: u8 = 1;
pub const STATUS_DRIVER: u8 = 2;
pub const STATUS_DRIVER_OK: u8 = 4;
pub const STATUS_FEATURES_OK: u8 = 8;
pub const STATUS_FAILED: u8 = 128;

/// Feature bit 32: "I speak virtio 1.0, not the legacy layout."
const FEATURE_VERSION_1: u64 = 1 << 32;

/// Offsets within `struct virtio_pci_common_cfg` (virtio 1.0, §4.1.4.3).
mod common {
    pub const DEVICE_FEATURE_SELECT: usize = 0x00;
    pub const DEVICE_FEATURE: usize = 0x04;
    pub const DRIVER_FEATURE_SELECT: usize = 0x08;
    pub const DRIVER_FEATURE: usize = 0x0c;
    pub const NUM_QUEUES: usize = 0x12;
    pub const DEVICE_STATUS: usize = 0x14;
    pub const QUEUE_SELECT: usize = 0x16;
    pub const QUEUE_SIZE: usize = 0x18;
    pub const QUEUE_ENABLE: usize = 0x1c;
    pub const QUEUE_NOTIFY_OFF: usize = 0x1e;
    pub const QUEUE_DESC: usize = 0x20;
    pub const QUEUE_DRIVER: usize = 0x28;
    pub const QUEUE_DEVICE: usize = 0x30;
}

pub struct Transport {
    common: u64,
    notify: u64,
    notify_multiplier: u32,
    device_config: u64,
}

fn write64(address: u64, value: u64) {
    // Two 32-bit writes: the spec defines this register as a pair, and some
    // devices only implement it that way.
    write32(address, value as u32);
    write32(address + 4, (value >> 32) as u32);
}

impl Transport {
    /// Locate a device's register windows from its PCI capabilities.
    pub fn new(device: &pci::Device) -> Option<Transport> {
        let mut common = 0;
        let mut notify = 0;
        let mut notify_multiplier = 0;
        let mut device_config = 0;

        device.capabilities(|id, offset| {
            if id != CAPABILITY_VENDOR_SPECIFIC {
                return;
            }
            let kind = device.address.read8(offset + 3);
            let bar_index = device.address.read8(offset + 4);
            let bar_offset = device.address.read32(offset + 8) as u64;
            let Some(bar) = device.bar(bar_index) else {
                return;
            };
            let window = bar + bar_offset;
            let length = device.address.read32(offset + 12).max(0x1000) as usize;
            // The BAR may be nowhere near RAM — OVMF puts virtio registers at
            // 768 GiB — so the window has to be mapped before it is read.
            if !nawa_core::paging::map_device(window, length) {
                return;
            }
            match kind {
                CONFIG_COMMON => common = window,
                CONFIG_NOTIFY => {
                    notify = window;
                    notify_multiplier = device.address.read32(offset + 16);
                }
                CONFIG_ISR => {}
                CONFIG_DEVICE => device_config = window,
                _ => {}
            }
        });

        if common == 0 || notify == 0 {
            return None;
        }
        Some(Transport { common, notify, notify_multiplier, device_config })
    }

    pub fn status(&self) -> u8 {
        read8(self.common + common::DEVICE_STATUS as u64)
    }

    pub fn set_status(&self, status: u8) {
        write8(self.common + common::DEVICE_STATUS as u64, status);
    }

    pub fn add_status(&self, bits: u8) {
        self.set_status(self.status() | bits);
    }

    pub fn queue_count(&self) -> u16 {
        read16(self.common + common::NUM_QUEUES as u64)
    }

    fn device_features(&self) -> u64 {
        write32(self.common + common::DEVICE_FEATURE_SELECT as u64, 0);
        let low = read32(self.common + common::DEVICE_FEATURE as u64) as u64;
        write32(self.common + common::DEVICE_FEATURE_SELECT as u64, 1);
        let high = read32(self.common + common::DEVICE_FEATURE as u64) as u64;
        low | high << 32
    }

    fn set_driver_features(&self, features: u64) {
        write32(self.common + common::DRIVER_FEATURE_SELECT as u64, 0);
        write32(self.common + common::DRIVER_FEATURE as u64, features as u32);
        write32(self.common + common::DRIVER_FEATURE_SELECT as u64, 1);
        write32(self.common + common::DRIVER_FEATURE as u64, (features >> 32) as u32);
    }

    /// The handshake, in the order the spec requires: reset, acknowledge,
    /// negotiate, confirm. A device that will not confirm our feature set is
    /// marked failed rather than driven on hope.
    pub fn begin(&self, wanted: u64) -> Option<u64> {
        self.set_status(0);
        // Reset is complete when the device reads back zero.
        for _ in 0..100_000 {
            if self.status() == 0 {
                break;
            }
            core::hint::spin_loop();
        }
        self.add_status(STATUS_ACKNOWLEDGE);
        self.add_status(STATUS_DRIVER);

        let offered = self.device_features();
        if offered & FEATURE_VERSION_1 == 0 {
            self.add_status(STATUS_FAILED);
            return None;
        }
        let negotiated = (offered & wanted) | FEATURE_VERSION_1;
        self.set_driver_features(negotiated);
        self.add_status(STATUS_FEATURES_OK);
        if self.status() & STATUS_FEATURES_OK == 0 {
            self.add_status(STATUS_FAILED);
            return None;
        }
        Some(negotiated)
    }

    /// Attach a queue to slot `index`, sized to whatever the device allows
    /// (capped by our own ceiling).
    pub fn setup_queue(&self, index: u16, preferred: u16) -> Option<(Queue, u64)> {
        write16(self.common + common::QUEUE_SELECT as u64, index);
        let maximum = read16(self.common + common::QUEUE_SIZE as u64);
        if maximum == 0 {
            return None;
        }
        let size = preferred.min(maximum);
        let queue = Queue::new(size)?;

        write16(self.common + common::QUEUE_SIZE as u64, size);
        write64(self.common + common::QUEUE_DESC as u64, queue.descriptor_address());
        write64(self.common + common::QUEUE_DRIVER as u64, queue.available_address());
        write64(self.common + common::QUEUE_DEVICE as u64, queue.used_address());
        let notify_offset = read16(self.common + common::QUEUE_NOTIFY_OFF as u64) as u64;
        write16(self.common + common::QUEUE_ENABLE as u64, 1);

        let doorbell = self.notify + notify_offset * self.notify_multiplier as u64;
        Some((queue, doorbell))
    }

    pub fn ready(&self) {
        self.add_status(STATUS_DRIVER_OK);
    }

    /// Ring the doorbell for a queue. The fence matters: the descriptors and
    /// the available index must be visible before the device is told to look.
    pub fn notify(&self, doorbell: u64, queue_index: u16) {
        fence(Ordering::SeqCst);
        write16(doorbell, queue_index);
    }

    /// Read a byte of device-specific configuration.
    pub fn config8(&self, offset: u64) -> u8 {
        if self.device_config == 0 {
            return 0;
        }
        read8(self.device_config + offset)
    }

    pub fn config64(&self, offset: u64) -> u64 {
        if self.device_config == 0 {
            return 0;
        }
        let low = read32(self.device_config + offset) as u64;
        let high = read32(self.device_config + offset + 4) as u64;
        low | high << 32
    }
}
