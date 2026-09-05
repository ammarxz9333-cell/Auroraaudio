//! Allocation audit used only by tests. Measurement setup is outside the callback.
use std::alloc::{GlobalAlloc, Layout, System};

pub struct CountingAllocator;
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        counter::record();
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        counter::record();
        unsafe { System.realloc(pointer, layout, size) }
    }
}

pub fn count_allocations(action: impl FnOnce()) -> usize {
    counter::measure(action)
}

#[cfg(windows)]
mod counter {
    use std::sync::{
        atomic::{AtomicU32, AtomicUsize, Ordering},
        Mutex,
    };
    // Rust 1.78 Windows GNU TLS initialization can allocate. Never access Rust
    // TLS inside the allocator: it would recursively enter the allocator.
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentThreadId() -> u32;
    }
    static SERIAL: Mutex<()> = Mutex::new(());
    static OWNER: AtomicU32 = AtomicU32::new(0);
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    fn thread_id() -> u32 {
        unsafe { GetCurrentThreadId() }
    }
    pub fn record() {
        if OWNER.load(Ordering::Relaxed) == thread_id() {
            COUNT.fetch_add(1, Ordering::Relaxed);
        }
    }
    pub fn measure(action: impl FnOnce()) -> usize {
        let id = thread_id();
        assert_ne!(OWNER.load(Ordering::Relaxed), id, "nested allocation audit");
        let _serial = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                OWNER.store(0, Ordering::Relaxed);
            }
        }
        COUNT.store(0, Ordering::Relaxed);
        OWNER.store(id, Ordering::Relaxed);
        let reset = Reset;
        action();
        drop(reset);
        COUNT.load(Ordering::Relaxed)
    }
}

#[cfg(not(windows))]
mod counter {
    use std::cell::Cell;
    thread_local! {
        static ACTIVE: Cell<bool> = const { Cell::new(false) };
        static COUNT: Cell<usize> = const { Cell::new(0) };
    }
    pub fn record() {
        if ACTIVE.try_with(Cell::get).unwrap_or(false) {
            COUNT.with(|count| count.set(count.get().saturating_add(1)));
        }
    }
    pub fn measure(action: impl FnOnce()) -> usize {
        assert!(!ACTIVE.with(Cell::get), "nested allocation audit");
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                ACTIVE.with(|active| active.set(false));
            }
        }
        COUNT.with(|count| count.set(0));
        ACTIVE.with(|active| active.set(true));
        let reset = Reset;
        action();
        drop(reset);
        COUNT.with(Cell::get)
    }
}
