//! virtio-console — the kernel's channel to the world outside the machine.
//!
//! This is the device that makes i3mlOS an *AI* operating system rather than
//! an operating system with ideas about agents: a model lives on the other
//! end of it. What matters is where the channel sits. It belongs to the
//! kernel, not to a library inside a program, so reaching a model is an act
//! the gate mediates — a capability to hold, a budget to spend, a line in the
//! journal — exactly like touching a file or sending mail.
//!
//! Four queues, because the multiport protocol is the one the devices in the
//! world actually implement: 0 and 1 carry the port's data, 2 and 3 carry
//! control. A port stays shut until the guest says it is ready and open, and
//! bytes written to a shut port are dropped without a word — which is exactly
//! how this driver failed before the handshake was written.

use nawa_core::uefi::PAGE_SIZE;
use nawa_core::{mem, mmio};

use crate::queue::Queue;
use crate::transport::{Transport, DEVICE_CONSOLE};

/// **Port 1, not port 0.** Port 0 is the console the firmware writes to —
/// this driver's first version shared it, and the first thing the model relay
/// ever received was OVMF's boot chatter with our request glued to the end of
/// it. A channel anything else can write to is not a channel.
const PORT: u32 = 1;
/// Queues run: port 0 gets 0 and 1, control gets 2 and 3, and every further
/// port N gets 2(N+1) and 2(N+1)+1.
const RECEIVE_QUEUE: u16 = 2 * (PORT as u16 + 1);
const TRANSMIT_QUEUE: u16 = RECEIVE_QUEUE + 1;
const CONTROL_RECEIVE_QUEUE: u16 = 2;
const CONTROL_TRANSMIT_QUEUE: u16 = 3;

/// Feature bit 1: this device has more than one port and a control channel.
const FEATURE_MULTIPORT: u64 = 1 << 1;

// Control events (virtio 1.0, §5.3.6.1).
const EVENT_DEVICE_READY: u16 = 0;
/// Announced by the device after DEVICE_READY. We wait for it rather than
/// parse it: the port we use is known, and the message's arrival is the
/// signal that matters.
#[allow(dead_code)]
const EVENT_DEVICE_ADD: u16 = 1;
const EVENT_PORT_READY: u16 = 3;
const EVENT_PORT_OPEN: u16 = 6;

const CONTROL_BYTES: usize = 8;
#[allow(dead_code)]
/// Room for one exchange. A larger reply is truncated rather than buffered:
/// the kernel's job here is to broker, not to accumulate.
const BUFFER_BYTES: usize = PAGE_SIZE * 4;

pub struct Console {
    transport: Transport,
    receive: Queue,
    receive_doorbell: u64,
    transmit: Queue,
    transmit_doorbell: u64,
    control_receive: Queue,
    control_receive_doorbell: u64,
    control_transmit: Queue,
    control_transmit_doorbell: u64,
    /// Device-visible memory. Frames, not heap: the device reads these.
    receive_buffer: u64,
    transmit_buffer: u64,
    control_buffer: u64,
    control_reply: u64,
    /// Is a receive buffer currently posted and waiting to be filled?
    posted: Option<u16>,
}

impl Console {
    pub fn open() -> Option<Console> {
        let device = crate::find(DEVICE_CONSOLE)?;
        device.enable();
        let transport = Transport::new(&device)?;
        let negotiated = transport.begin(FEATURE_MULTIPORT)?;
        let multiport = negotiated & FEATURE_MULTIPORT != 0;

        let (receive, receive_doorbell) = transport.setup_queue(RECEIVE_QUEUE, 8)?;
        let (transmit, transmit_doorbell) = transport.setup_queue(TRANSMIT_QUEUE, 8)?;
        let (control_receive, control_receive_doorbell) =
            transport.setup_queue(CONTROL_RECEIVE_QUEUE, 8)?;
        let (control_transmit, control_transmit_doorbell) =
            transport.setup_queue(CONTROL_TRANSMIT_QUEUE, 8)?;
        transport.ready();

        let receive_buffer = mem::allocate_frames(BUFFER_BYTES / PAGE_SIZE)? as u64;
        let transmit_buffer = mem::allocate_frames(BUFFER_BYTES / PAGE_SIZE)? as u64;
        let control_buffer = mem::allocate_frames(1)? as u64;
        let control_reply = control_buffer + 512;

        let mut console = Console {
            transport,
            receive,
            receive_doorbell,
            transmit,
            transmit_doorbell,
            control_receive,
            control_receive_doorbell,
            control_transmit,
            control_transmit_doorbell,
            receive_buffer,
            transmit_buffer,
            control_buffer,
            control_reply,
            posted: None,
        };
        if multiport {
            console.handshake();
        }
        console.post_receive()?;
        Some(console)
    }

    /// Tell the device we exist, then tell it the port is ready and open.
    ///
    /// The order is the protocol's, not ours: a device announces its ports
    /// only after the guest says it is ready, and forwards data only after
    /// the guest says the port is open. Each step **waits for the device's
    /// answer** rather than assuming it has been processed — an earlier
    /// version merely polled once, worked whenever an unrelated log line
    /// happened to slow the driver down, and dropped every message when it
    /// did not. A handshake that depends on timing is not a handshake.
    fn handshake(&mut self) {
        let posted = self.post_control_receive();
        self.send_control(PORT, EVENT_DEVICE_READY, 1);
        // The device answers DEVICE_READY by announcing its ports.
        if let Some(head) = posted {
            let _ = self.control_receive.wait_for(head, 20_000_000);
        }

        let posted = self.post_control_receive();
        self.send_control(PORT, EVENT_PORT_READY, 1);
        self.send_control(PORT, EVENT_PORT_OPEN, 1);
        // And answers those with the port's own state — proof it processed
        // them, which is what we actually need before writing data.
        if let Some(head) = posted {
            let _ = self.control_receive.wait_for(head, 20_000_000);
        }
        self.post_control_receive();
    }

    fn send_control(&mut self, port: u32, event: u16, value: u16) {
        mmio::write32(self.control_reply, port);
        mmio::write16(self.control_reply + 4, event);
        mmio::write16(self.control_reply + 6, value);
        if let Some(head) =
            self.control_transmit.submit(&[(self.control_reply, CONTROL_BYTES as u32, false)])
        {
            self.transport.notify(self.control_transmit_doorbell, CONTROL_TRANSMIT_QUEUE);
            self.control_transmit.wait_for(head, 20_000_000);
        }
    }

    fn post_control_receive(&mut self) -> Option<u16> {
        let head = self.control_receive.submit(&[(self.control_buffer, 512, true)])?;
        self.transport.notify(self.control_receive_doorbell, CONTROL_RECEIVE_QUEUE);
        Some(head)
    }

    /// Hand the device somewhere to put what arrives next.
    fn post_receive(&mut self) -> Option<()> {
        let head = self.receive.submit(&[(self.receive_buffer, BUFFER_BYTES as u32, true)])?;
        self.posted = Some(head);
        self.transport.notify(self.receive_doorbell, RECEIVE_QUEUE);
        Some(())
    }

    pub fn send(&mut self, data: &[u8]) -> bool {
        if data.len() > BUFFER_BYTES {
            return false;
        }
        mmio::write_from(self.transmit_buffer, data);
        let Some(head) = self.transmit.submit(&[(self.transmit_buffer, data.len() as u32, false)])
        else {
            return false;
        };
        self.transport.notify(self.transmit_doorbell, TRANSMIT_QUEUE);
        self.transmit.wait_for(head, 50_000_000).is_some()
    }

    /// Wait for the other end to answer. Returns how many bytes landed in
    /// `out`; a silent peer times out rather than hanging the machine.
    pub fn receive(&mut self, out: &mut [u8], attempts: u64) -> Option<usize> {
        let head = self.posted?;
        let length = self.receive.wait_for(head, attempts)?;
        let length = (length as usize).min(BUFFER_BYTES).min(out.len());
        mmio::read_into(self.receive_buffer, &mut out[..length]);
        // Immediately give the device somewhere to write the next message.
        self.post_receive();
        Some(length)
    }
}
