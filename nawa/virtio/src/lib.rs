//! VirtIO — the only way this kernel talks to hardware.
//!
//! Deliberate: by targeting virtual devices first, the driver surface is a
//! dozen documented interfaces instead of the thousands a bare-metal OS must
//! carry. That single decision is what makes a from-scratch kernel shippable
//! this decade rather than next (master plan §1).

#![no_std]
#![deny(unsafe_code)]

pub mod block;
pub mod entropy;
pub mod queue;
pub mod transport;

use nawa_core::pci;

/// Find the first device of a given virtio kind on the bus, under either of
/// the PCI IDs it may present.
pub fn find(kind: transport::Kind) -> Option<pci::Device> {
    let mut found = None;
    pci::scan(|device| {
        if found.is_none()
            && device.vendor == transport::VENDOR_VIRTIO
            && (device.device == kind.modern || device.device == kind.transitional)
        {
            found = Some(device);
        }
    });
    found
}
