//! Boot-time self-tests, exposed as safe calls so the kernel binary can
//! exercise trusted-core machinery. CI asserts their serial markers.

use core::arch::asm;

/// Fire a breakpoint exception. If the IDT is wired correctly, the handler
/// logs `int3: breakpoint handled at …` and execution RESUMES here.
pub fn breakpoint() {
    unsafe {
        asm!("int3", options(nomem, nostack));
    }
}
