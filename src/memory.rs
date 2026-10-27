use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicU64, Ordering},
};

pub struct TrackedAllocator {
    live: AtomicU64,
    peak: AtomicU64,
}

impl TrackedAllocator {
    const fn new() -> Self {
        Self {
            live: AtomicU64::new(0),
            peak: AtomicU64::new(0),
        }
    }

    fn add(&self, bytes: usize) {
        let live = self
            .live
            .fetch_add(bytes as u64, Ordering::Relaxed)
            .saturating_add(bytes as u64);
        self.peak.fetch_max(live, Ordering::Relaxed);
    }

    fn snapshot(&self) -> HeapUsage {
        HeapUsage {
            live: self.live.load(Ordering::Relaxed),
            peak: self.peak.load(Ordering::Relaxed),
        }
    }
}

// SAFETY: Every allocation operation is forwarded unchanged to System. The
// bookkeeping uses only non-allocating, non-panicking atomic operations and
// never reads or modifies the allocation's contents or alignment.
unsafe impl GlobalAlloc for TrackedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            self.add(layout.size());
        }
        ptr
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            self.add(layout.size());
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe {
            System.dealloc(ptr, layout);
        }
        self.live.fetch_sub(layout.size() as u64, Ordering::Relaxed);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(ptr, layout, size) };
        if !result.is_null() {
            if size >= layout.size() {
                self.add(size - layout.size());
            } else {
                self.live
                    .fetch_sub((layout.size() - size) as u64, Ordering::Relaxed);
            }
        }
        result
    }
}

#[global_allocator]
static ALLOCATOR: TrackedAllocator = TrackedAllocator::new();

#[derive(Debug, Clone, Copy, Default)]
pub struct HeapUsage {
    pub live: u64,
    pub peak: u64,
}

pub fn snapshot() -> HeapUsage {
    ALLOCATOR.snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocator_tracks_alignment_zeroing_growth_shrink_and_free() {
        // A private counter avoids interference from allocations by other tests.
        let allocator = TrackedAllocator::new();
        let layout = Layout::from_size_align(128, 64).unwrap();
        unsafe {
            let mut ptr = allocator.alloc_zeroed(layout);
            assert!(!ptr.is_null());
            assert_eq!(ptr as usize % 64, 0);
            assert!(
                std::slice::from_raw_parts(ptr, 128)
                    .iter()
                    .all(|byte| *byte == 0)
            );
            ptr.write_bytes(42, 128);
            ptr = allocator.realloc(ptr, layout, 512);
            assert!(!ptr.is_null());
            assert_eq!(allocator.snapshot().live, 512);
            assert_eq!(allocator.snapshot().peak, 512);
            assert!(
                std::slice::from_raw_parts(ptr, 128)
                    .iter()
                    .all(|byte| *byte == 42)
            );
            let large = Layout::from_size_align(512, 64).unwrap();
            ptr = allocator.realloc(ptr, large, 64);
            assert!(!ptr.is_null());
            assert_eq!(allocator.snapshot().live, 64);
            allocator.dealloc(ptr, Layout::from_size_align(64, 64).unwrap());
        }
        assert_eq!(allocator.snapshot().live, 0);
        assert_eq!(allocator.snapshot().peak, 512);
    }
}
