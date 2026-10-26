#![allow(dead_code)]
use crate::{HazardGuard, HazardPointers};
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, Ordering};

pub struct Page {
    pub id: u64,
}

pub struct BufferPool {
    slots: Vec<AtomicPtr<Page>>,
    registry: Arc<HazardPointers<Page>>,
}

impl BufferPool {
    /// Returns a guard over the page in `slot`. The page stays mapped for as
    /// long as the caller holds the guard. Returning the pointer alone would
    /// drop the guard here and hand back an unprotected page.
    pub fn read(&self, slot: usize) -> Option<HazardGuard<'_, Page>> {
        loop {
            let candidate = self.slots[slot].load(Ordering::Acquire);
            if candidate.is_null() {
                return None;
            }
            let guard = self.registry.protect(candidate);
            if self.slots[slot].load(Ordering::Acquire) != candidate {
                // Evicted and replaced between the load and the publication.
                // Dropping the guard clears the slot, so nothing is held back.
                continue;
            }
            return Some(guard);
        }
    }

    /// Unlinks a page and hands it to the registry.
    pub fn evict(&self, slot: usize) {
        let old = self.slots[slot].swap(std::ptr::null_mut(), Ordering::AcqRel);
        if !old.is_null() {
            // Safety: the page came from Box::into_raw, is now unlinked, and
            // this swap is the only place that retires it.
            unsafe { self.registry.retire(old) };
        }
    }
}
