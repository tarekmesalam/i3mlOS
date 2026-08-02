//! Minimal UEFI surface, written from the UEFI 2.10 spec (specs are Tier-3
//! data in the purity charter; this code is Tier-1 ours). Only what the boot
//! path needs is defined — the spec's field ORDER is load-bearing, so omitted
//! trailing fields are simply not declared.

use core::ffi::c_void;

pub type EfiStatus = usize;
pub type EfiHandle = *mut c_void;

pub const EFI_SUCCESS: EfiStatus = 0;

#[repr(C)]
pub struct EfiTableHeader {
    pub signature: u64,
    pub revision: u32,
    pub header_size: u32,
    pub crc32: u32,
    reserved: u32,
}

/// EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL — only `OutputString` is typed; the
/// preceding `Reset` slot is kept opaque to hold the layout.
#[repr(C)]
pub struct EfiSimpleTextOutputProtocol {
    reset: usize,
    output_string: extern "efiapi" fn(*mut EfiSimpleTextOutputProtocol, *const u16) -> EfiStatus,
}

/// EFI_SYSTEM_TABLE, truncated after `ConOut` (later fields unused so far).
#[repr(C)]
pub struct EfiSystemTable {
    pub hdr: EfiTableHeader,
    pub firmware_vendor: *const u16,
    pub firmware_revision: u32,
    pub console_in_handle: EfiHandle,
    con_in: *mut c_void,
    pub console_out_handle: EfiHandle,
    con_out: *mut EfiSimpleTextOutputProtocol,
}

/// Safe handle to the firmware text console (valid until ExitBootServices,
/// which nothing calls yet).
pub struct Console {
    con_out: *mut EfiSimpleTextOutputProtocol,
}

impl Console {
    /// Trusted-core boundary: `system_table` must be the pointer UEFI passed
    /// to `efi_main`. `entry::boot` is the only caller.
    pub(crate) fn from_system_table(system_table: *mut EfiSystemTable) -> Option<Console> {
        if system_table.is_null() {
            return None;
        }
        let con_out = unsafe { (*system_table).con_out };
        if con_out.is_null() {
            return None;
        }
        Some(Console { con_out })
    }

    /// Write UTF-8 text to the firmware console, converting to the UCS-2 +
    /// CRLF the protocol expects. Characters outside UCS-2 degrade to '?'.
    pub fn write_str(&mut self, s: &str) {
        const CAP: usize = 64;
        let mut buf = [0u16; CAP + 1];
        let mut len = 0;

        let push = |buf: &mut [u16; CAP + 1], len: &mut usize, unit: u16| {
            buf[*len] = unit;
            *len += 1;
            if *len == CAP {
                buf[*len] = 0;
                let output_string = unsafe { (*self.con_out).output_string };
                output_string(self.con_out, buf.as_ptr());
                *len = 0;
            }
        };

        for ch in s.chars() {
            let unit = if (ch as u32) < 0x1_0000 { ch as u32 as u16 } else { b'?' as u16 };
            if unit == b'\n' as u16 {
                push(&mut buf, &mut len, b'\r' as u16);
            }
            push(&mut buf, &mut len, unit);
        }
        if len > 0 {
            buf[len] = 0;
            let output_string = unsafe { (*self.con_out).output_string };
            output_string(self.con_out, buf.as_ptr());
        }
    }

    pub fn write_line(&mut self, s: &str) {
        self.write_str(s);
        self.write_str("\n");
    }
}
