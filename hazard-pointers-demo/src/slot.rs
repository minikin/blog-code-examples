use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};

/// One entry in a fixed array of hazard slots, claimed by one thread at a time.
#[derive(Default)]
pub struct HazardSlot<T> {
    ptr: AtomicPtr<T>,
}

impl<T> HazardSlot<T> {
    /// Publishes whatever `source` holds and returns it, retrying until the
    /// publication and `source` agree.
    pub fn protect(&self, source: &AtomicPtr<T>) -> Option<*mut T> {
        loop {
            let candidate = source.load(Ordering::Acquire);
            if candidate.is_null() {
                self.clear();
                return None;
            }
            // SeqCst, not Release: a Release store followed by an Acquire load
            // of a different location can be reordered, and a scan that runs
            // between the two would free the node.
            self.ptr.store(candidate, Ordering::SeqCst);
            if source.load(Ordering::SeqCst) == candidate {
                return Some(candidate);
            }
        }
    }

    pub fn clear(&self) {
        self.ptr.store(ptr::null_mut(), Ordering::Release);
    }

    /// Read by the thread that scans before freeing.
    pub fn published(&self) -> *mut T {
        self.ptr.load(Ordering::SeqCst)
    }
}
