//! Boot handoff. The kernel binary stays `#![forbid(unsafe_code)]` by
//! delegating the raw firmware pointers here: it declares `efi_main`, then
//! immediately calls [`boot`], which builds a safe [`BootInfo`] and never
//! returns control to firmware.

use crate::{arch, serial, uefi};

/// Everything the safe kernel gets to touch at boot.
pub struct BootInfo {
    pub console: Option<uefi::Console>,
}

/// Trusted-core boundary: `system_table` must be the pointer UEFI passed to
/// `efi_main`, forwarded untouched. Initializes serial, wraps the firmware
/// console, and hands off to `kmain`, which must not return.
pub fn boot(
    _image_handle: uefi::EfiHandle,
    system_table: *mut uefi::EfiSystemTable,
    kmain: fn(BootInfo) -> !,
) -> ! {
    serial::init();
    let console = uefi::Console::from_system_table(system_table);
    kmain(BootInfo { console })
}

/// Terminal state for a kernel with no scheduler yet.
pub fn park() -> ! {
    arch::halt_forever()
}
