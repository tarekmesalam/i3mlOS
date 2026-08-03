//! Single-core boot-time statics. NAWA is single-threaded until SMP (Phase 3);
//! these cells make that assumption explicit and keep it in one place.

use core::cell::UnsafeCell;

/// A `Sync` cell for statics mutated only during single-threaded bring-up
/// (GDT, IDT, TSS, memory-map buffer) or behind [`SpinLock`].
pub struct StaticCell<T>(UnsafeCell<T>);

// Single core, interrupts masked during bring-up: no data races exist yet.
unsafe impl<T> Sync for StaticCell<T> {}

impl<T> StaticCell<T> {
    pub const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    pub fn get(&self) -> *mut T {
        self.0.get()
    }
}

/// Minimal test-and-set spinlock — the allocator's guard until a real
/// scheduler exists. Written here rather than vendored (purity Tier 1).
pub struct SpinLock<T> {
    locked: core::sync::atomic::AtomicBool,
    value: UnsafeCell<T>,
}

unsafe impl<T> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self {
        Self { locked: core::sync::atomic::AtomicBool::new(false), value: UnsafeCell::new(value) }
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        use core::sync::atomic::Ordering;
        while self.locked.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
        let result = f(unsafe { &mut *self.value.get() });
        self.locked.store(false, Ordering::Release);
        result
    }
}
