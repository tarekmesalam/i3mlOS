//! Local APIC in x2APIC mode + TSC-deadline timer. This is the kernel's
//! heartbeat: the first thing that can interrupt an agent, and the clock
//! every SIJIL entry and budget deadline is measured against.
//!
//! x2APIC over xAPIC deliberately: MSR access instead of MMIO means no page
//! mapping is needed while we still run on the firmware's tables, and it is
//! the mode a modern machine (and Firecracker) boots with anyway.

use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::serial;

const IA32_APIC_BASE: u32 = 0x1b;
const APIC_BASE_ENABLE: u64 = 1 << 11;
const APIC_BASE_X2APIC: u64 = 1 << 10;

// x2APIC MSRs (xAPIC MMIO offset >> 4 | 0x800).
const X2APIC_SIVR: u32 = 0x80f; // spurious interrupt vector
const X2APIC_EOI: u32 = 0x80b;
const X2APIC_LVT_TIMER: u32 = 0x832;
const X2APIC_INITIAL_COUNT: u32 = 0x838;
const X2APIC_CURRENT_COUNT: u32 = 0x839;
const X2APIC_DIVIDE_CONFIG: u32 = 0x83e;
const X2APIC_TSC_DEADLINE: u32 = 0x6e0;

const LVT_MASKED: u32 = 1 << 16;
const LVT_PERIODIC_MODE: u32 = 0b01 << 17;
/// Divide the APIC timer input clock by 16 (SDM encoding 0b0011).
const DIVIDE_BY_16: u64 = 0b0011;

/// Timer interrupt vector. Above 0x20 (legacy PIC range), below the range
/// AMAN will use for software interrupts later.
pub const TIMER_VECTOR: u8 = 0x40;

const LVT_TSC_DEADLINE_MODE: u32 = 0b10 << 17;

/// TSC ticks per microsecond, measured once at boot.
static TSC_PER_MICROSECOND: AtomicU64 = AtomicU64::new(0);
/// Monotonic tick count, incremented by the timer handler.
static TICKS: AtomicU64 = AtomicU64::new(0);
/// Timer period in TSC ticks (deadline mode only).
static PERIOD: AtomicU64 = AtomicU64::new(0);
/// 1 = TSC-deadline mode (rearmed per tick), 0 = APIC periodic mode (free-running).
static DEADLINE_MODE: AtomicU64 = AtomicU64::new(0);

fn read_msr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        asm!("rdmsr", in("ecx") msr, out("eax") low, out("edx") high, options(nomem, nostack, preserves_flags));
    }
    (high as u64) << 32 | low as u64
}

fn write_msr(msr: u32, value: u64) {
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") value as u32,
            in("edx") (value >> 32) as u32,
            options(nomem, nostack, preserves_flags),
        );
    }
}

/// Read the timestamp counter. Serializing (`lfence`) so measurements can't
/// float across the instructions they bracket.
pub fn timestamp() -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        asm!("lfence", "rdtsc", out("eax") low, out("edx") high, options(nomem, nostack, preserves_flags));
    }
    (high as u64) << 32 | low as u64
}

/// x2APIC is required; a TSC-deadline timer is a bonus. Returns
/// `(x2apic, tsc_deadline)`.
fn features() -> (bool, bool) {
    let (mut features, mut power_features) = (0u32, 0u32);
    unsafe {
        // CPUID.01H: ECX[21] = x2APIC, ECX[24] = TSC-Deadline
        asm!(
            "push rbx", "mov eax, 1", "cpuid", "mov {ecx:e}, ecx", "pop rbx",
            ecx = out(reg) features, out("eax") _, out("ecx") _, out("edx") _,
            options(nostack),
        );
        // CPUID.80000007H: EDX[8] = invariant TSC
        asm!(
            "push rbx", "mov eax, 0x80000007", "cpuid", "mov {edx:e}, edx", "pop rbx",
            edx = out(reg) power_features, out("eax") _, out("ecx") _, out("edx") _,
            options(nostack),
        );
    }
    let x2apic = features & (1 << 21) != 0;
    let deadline = features & (1 << 24) != 0;
    let invariant_tsc = power_features & (1 << 8) != 0;
    if !x2apic {
        serial::write_str("apic: no x2APIC\n");
    }
    if !deadline {
        serial::write_str("apic: no TSC-deadline timer; using APIC periodic mode\n");
    }
    if !invariant_tsc {
        serial::write_str("apic: TSC not invariant (timing may drift)\n");
    }
    (x2apic, deadline)
}

/// Spin until the 8254's channel-2 one-shot expires. The classic
/// no-dependencies delay: everything else calibrates against it.
fn pit_wait(milliseconds: u64) -> bool {
    const PIT_FREQUENCY: u64 = 1_193_182; // Hz
    let count = (PIT_FREQUENCY * milliseconds / 1000) as u16;

    // Speaker off, channel-2 gate on; mode 0 (interrupt on terminal count).
    crate::arch::outb(0x61, (crate::arch::inb(0x61) & !0x02) | 0x01);
    crate::arch::outb(0x43, 0xb0);
    crate::arch::outb(0x42, count as u8);
    crate::arch::outb(0x42, (count >> 8) as u8);

    // Restart the counter by toggling the gate.
    let gate = crate::arch::inb(0x61) & !0x01;
    crate::arch::outb(0x61, gate);
    crate::arch::outb(0x61, gate | 0x01);

    // Port 0x61 bit 5 mirrors channel 2's OUT pin: 1 once the count expires.
    let mut guard = 0u64;
    while crate::arch::inb(0x61) & 0x20 == 0 {
        guard += 1;
        if guard > 500_000_000 {
            serial::write_str("apic: PIT wait timed out\n");
            return false;
        }
    }
    true
}

/// Bring up the local APIC and start the heartbeat.
///
/// Two timer modes, one calibration pass: the TSC (for timestamps) and the
/// APIC timer (for ticks) are both measured against a single PIT interval.
/// TSC-deadline mode is used when the CPU offers it; otherwise the APIC's own
/// periodic mode, which every x86 machine — and every emulator — supports.
///
/// Returns TSC ticks per microsecond (0 if no timer could be brought up).
pub(crate) fn init(period_microseconds: u64) -> u64 {
    const CALIBRATION_MILLISECONDS: u64 = 10;

    let (x2apic, tsc_deadline) = features();
    if !x2apic {
        return 0;
    }

    // Enable x2APIC mode (order matters: the xAPIC enable bit goes first).
    let base = read_msr(IA32_APIC_BASE);
    write_msr(IA32_APIC_BASE, base | APIC_BASE_ENABLE | APIC_BASE_X2APIC);
    // Spurious vector register: bit 8 = APIC software enable.
    write_msr(X2APIC_SIVR, 0x100 | 0xff);

    // Calibrate both clocks in one PIT interval, with the APIC timer masked
    // and free-running from its maximum count.
    write_msr(X2APIC_DIVIDE_CONFIG, DIVIDE_BY_16);
    write_msr(X2APIC_LVT_TIMER, (LVT_MASKED | TIMER_VECTOR as u32) as u64);
    write_msr(X2APIC_INITIAL_COUNT, u32::MAX as u64);
    let tsc_start = timestamp();
    if !pit_wait(CALIBRATION_MILLISECONDS) {
        return 0;
    }
    let tsc_elapsed = timestamp() - tsc_start;
    let apic_elapsed = u32::MAX as u64 - read_msr(X2APIC_CURRENT_COUNT);
    write_msr(X2APIC_INITIAL_COUNT, 0); // stop the calibration run

    let microseconds = CALIBRATION_MILLISECONDS * 1000;
    let tsc_per_microsecond = tsc_elapsed / microseconds;
    let apic_per_microsecond = apic_elapsed / microseconds;
    if tsc_per_microsecond == 0 {
        return 0;
    }
    TSC_PER_MICROSECOND.store(tsc_per_microsecond, Ordering::Relaxed);

    if tsc_deadline {
        DEADLINE_MODE.store(1, Ordering::Relaxed);
        PERIOD.store(tsc_per_microsecond * period_microseconds, Ordering::Relaxed);
        write_msr(X2APIC_LVT_TIMER, (LVT_TSC_DEADLINE_MODE | TIMER_VECTOR as u32) as u64);
        arm_next_deadline();
    } else {
        if apic_per_microsecond == 0 {
            return 0;
        }
        write_msr(X2APIC_LVT_TIMER, (LVT_PERIODIC_MODE | TIMER_VECTOR as u32) as u64);
        write_msr(X2APIC_INITIAL_COUNT, apic_per_microsecond * period_microseconds);
    }
    tsc_per_microsecond
}

/// Program the next TSC deadline. Called from the timer handler; a no-op in
/// periodic mode, where the APIC rearms itself.
pub(crate) fn arm_next_deadline() {
    if DEADLINE_MODE.load(Ordering::Relaxed) == 0 {
        return;
    }
    let period = PERIOD.load(Ordering::Relaxed);
    if period != 0 {
        write_msr(X2APIC_TSC_DEADLINE, timestamp() + period);
    }
}

pub(crate) fn end_of_interrupt() {
    write_msr(X2APIC_EOI, 0);
}

pub(crate) fn record_tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

/// Timer ticks since boot.
pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

/// Microseconds since the TSC was calibrated (i.e. since early boot).
/// This is the timestamp SIJIL entries and budget deadlines use.
pub fn now_microseconds() -> u64 {
    let per_microsecond = TSC_PER_MICROSECOND.load(Ordering::Relaxed);
    if per_microsecond == 0 {
        return 0;
    }
    timestamp() / per_microsecond
}

pub fn calibrated() -> bool {
    TSC_PER_MICROSECOND.load(Ordering::Relaxed) != 0
}
