//! Minimal UEFI surface, written from the UEFI 2.10 spec (specs are Tier-3
//! data in the purity charter; this code is Tier-1 ours). Field ORDER in the
//! firmware tables is load-bearing: every slot up to the last one we call is
//! declared, by spec name, even when unused.

use core::ffi::c_void;

use crate::cell::StaticCell;
use crate::fb::{Framebuffer, PixelLayout};

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

#[repr(C)]
pub struct EfiGuid(u32, u16, u16, [u8; 8]);

/// EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL — only `OutputString` is typed; the
/// preceding `Reset` slot is kept opaque to hold the layout.
#[repr(C)]
pub struct EfiSimpleTextOutputProtocol {
    reset: usize,
    output_string: extern "efiapi" fn(*mut EfiSimpleTextOutputProtocol, *const u16) -> EfiStatus,
}

/// EFI_BOOT_SERVICES. Slots we call are typed; the rest are opaque `usize`
/// placeholders named after the spec so the offsets cannot drift silently.
#[repr(C)]
pub struct EfiBootServices {
    pub hdr: EfiTableHeader,
    raise_tpl: usize,
    restore_tpl: usize,
    allocate_pages: usize,
    free_pages: usize,
    get_memory_map: extern "efiapi" fn(*mut usize, *mut u8, *mut usize, *mut usize, *mut u32) -> EfiStatus,
    allocate_pool: usize,
    free_pool: usize,
    create_event: usize,
    set_timer: usize,
    wait_for_event: usize,
    signal_event: usize,
    close_event: usize,
    check_event: usize,
    install_protocol_interface: usize,
    reinstall_protocol_interface: usize,
    uninstall_protocol_interface: usize,
    handle_protocol: usize,
    reserved: usize,
    register_protocol_notify: usize,
    locate_handle: usize,
    locate_device_path: usize,
    install_configuration_table: usize,
    load_image: usize,
    start_image: usize,
    exit: usize,
    unload_image: usize,
    exit_boot_services: extern "efiapi" fn(EfiHandle, usize) -> EfiStatus,
    get_next_monotonic_count: usize,
    stall: usize,
    set_watchdog_timer: usize,
    connect_controller: usize,
    disconnect_controller: usize,
    open_protocol: usize,
    close_protocol: usize,
    open_protocol_information: usize,
    protocols_per_handle: usize,
    locate_handle_buffer: usize,
    locate_protocol: extern "efiapi" fn(*const EfiGuid, *mut c_void, *mut *mut c_void) -> EfiStatus,
}

/// EFI_SYSTEM_TABLE, truncated after `BootServices` (later fields unused).
#[repr(C)]
pub struct EfiSystemTable {
    pub hdr: EfiTableHeader,
    pub firmware_vendor: *const u16,
    pub firmware_revision: u32,
    pub console_in_handle: EfiHandle,
    con_in: *mut c_void,
    pub console_out_handle: EfiHandle,
    con_out: *mut EfiSimpleTextOutputProtocol,
    standard_error_handle: EfiHandle,
    std_err: *mut c_void,
    runtime_services: *mut c_void,
    boot_services: *mut EfiBootServices,
}

// ---------------------------------------------------------------------------
// Graphics Output Protocol
// ---------------------------------------------------------------------------

const GOP_GUID: EfiGuid =
    EfiGuid(0x9042_a9de, 0x23dc, 0x4a38, [0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a]);

#[repr(C)]
struct EfiGraphicsOutputProtocol {
    query_mode: usize,
    set_mode: usize,
    blt: usize,
    mode: *mut EfiGraphicsOutputProtocolMode,
}

#[repr(C)]
struct EfiGraphicsOutputProtocolMode {
    max_mode: u32,
    mode: u32,
    info: *mut EfiGraphicsOutputModeInformation,
    size_of_info: usize,
    frame_buffer_base: u64,
    frame_buffer_size: usize,
}

#[repr(C)]
struct EfiGraphicsOutputModeInformation {
    version: u32,
    horizontal_resolution: u32,
    vertical_resolution: u32,
    pixel_format: u32,
    pixel_information: [u32; 4],
    pixels_per_scan_line: u32,
}

const PIXEL_RGBX: u32 = 0;
const PIXEL_BGRX: u32 = 1;

/// Ask the firmware where the linear framebuffer lives. Must run while boot
/// services are alive; the mapping stays valid after we take over.
pub(crate) fn locate_framebuffer(system_table: *mut EfiSystemTable) -> Option<Framebuffer> {
    if system_table.is_null() {
        return None;
    }
    let bs = unsafe { (*system_table).boot_services };
    if bs.is_null() {
        return None;
    }
    let mut interface: *mut c_void = core::ptr::null_mut();
    let status =
        unsafe { ((*bs).locate_protocol)(&GOP_GUID, core::ptr::null_mut(), &mut interface) };
    if status != EFI_SUCCESS || interface.is_null() {
        return None;
    }
    let gop = interface as *mut EfiGraphicsOutputProtocol;
    let mode = unsafe { (*gop).mode };
    if mode.is_null() {
        return None;
    }
    let info = unsafe { (*mode).info };
    if info.is_null() {
        return None;
    }
    let layout = match unsafe { (*info).pixel_format } {
        PIXEL_RGBX => PixelLayout::Rgbx,
        PIXEL_BGRX => PixelLayout::Bgrx,
        _ => return None, // bitmask/blt-only modes: no direct framebuffer
    };
    Some(Framebuffer::new(
        unsafe { (*mode).frame_buffer_base } as usize,
        unsafe { (*info).horizontal_resolution } as usize,
        unsafe { (*info).vertical_resolution } as usize,
        unsafe { (*info).pixels_per_scan_line } as usize,
        layout,
    ))
}

// ---------------------------------------------------------------------------
// Memory map + ExitBootServices
// ---------------------------------------------------------------------------

/// EFI_MEMORY_DESCRIPTOR (entries are `descriptor_size` apart — larger than
/// this struct; always step by the reported size, never by `size_of`).
#[repr(C)]
struct EfiMemoryDescriptor {
    r#type: u32,
    physical_start: u64,
    virtual_start: u64,
    number_of_pages: u64,
    attribute: u64,
}

pub const EFI_CONVENTIONAL_MEMORY: u32 = 7;
pub const PAGE_SIZE: usize = 4096;

const MAP_BUFFER_SIZE: usize = 32 * 1024;
/// Backing store is u64 so the buffer is 8-aligned: `regions()` materializes
/// `&EfiMemoryDescriptor` (align 8) into it, and an align-1 byte array would
/// be undefined behavior the moment the linker places it oddly (review
/// finding, M0).
static MAP_BUFFER: StaticCell<[u64; MAP_BUFFER_SIZE / 8]> =
    StaticCell::new([0; MAP_BUFFER_SIZE / 8]);

/// A parsed view of the final UEFI memory map (the one handed to
/// `ExitBootServices`). Lives in a kernel static; valid forever after.
#[derive(Clone, Copy)]
pub struct MemoryMap {
    buffer: *const u8,
    map_size: usize,
    descriptor_size: usize,
}

pub struct MemoryRegion {
    pub r#type: u32,
    pub physical_start: u64,
    pub pages: u64,
}

impl MemoryMap {
    pub fn regions(&self) -> impl Iterator<Item = MemoryRegion> + '_ {
        let count = self.map_size / self.descriptor_size;
        (0..count).map(move |index| {
            let descriptor =
                unsafe { &*(self.buffer.add(index * self.descriptor_size) as *const EfiMemoryDescriptor) };
            MemoryRegion {
                r#type: descriptor.r#type,
                physical_start: descriptor.physical_start,
                pages: descriptor.number_of_pages,
            }
        })
    }
}

/// The ExitBootServices dance: fetch the map, call exit with its key, and if
/// the firmware mutated the map in between (EFI_INVALID_PARAMETER), refetch
/// and retry. After success the firmware is gone and the machine is ours.
pub(crate) fn exit_boot_services(
    image_handle: EfiHandle,
    system_table: *mut EfiSystemTable,
) -> Option<MemoryMap> {
    if system_table.is_null() {
        return None;
    }
    let bs = unsafe { (*system_table).boot_services };
    if bs.is_null() {
        return None;
    }
    let buffer = MAP_BUFFER.get() as *mut u8;

    for _ in 0..8 {
        let mut map_size = MAP_BUFFER_SIZE;
        let mut map_key = 0usize;
        let mut descriptor_size = 0usize;
        let mut descriptor_version = 0u32;
        let status = unsafe {
            ((*bs).get_memory_map)(
                &mut map_size,
                buffer,
                &mut map_key,
                &mut descriptor_size,
                &mut descriptor_version,
            )
        };
        if status != EFI_SUCCESS {
            return None; // buffer too small would need a bigger static
        }
        let status = unsafe { ((*bs).exit_boot_services)(image_handle, map_key) };
        if status == EFI_SUCCESS {
            return Some(MemoryMap { buffer, map_size, descriptor_size });
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Firmware text console (valid only BEFORE ExitBootServices)
// ---------------------------------------------------------------------------

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
