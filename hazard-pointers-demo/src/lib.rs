//! Hazard pointers protecting a Treiber stack, the code for part 3 of the ABA
//! series: <https://minikin.me/blog/solving-the-aba-problem-in-rust-hazard-pointers/>

use std::collections::HashSet;
use std::fmt;
use std::mem::ManuallyDrop;
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, ThreadId};

mod buffer_pool;
pub mod slot;

pub use slot::HazardSlot;

/// A registry of pointers that threads are currently reading.
///
/// One slot per thread, which is all a Treiber stack needs. A list or queue
/// walk needs several slots per thread.
pub struct HazardPointers<T: Send> {
    thread_hazards: Mutex<Vec<(ThreadId, *mut T)>>,
    retire_list: Mutex<Vec<*mut T>>,
}

// Safety: the registry only moves raw pointers around and deallocates them.
// `T: Send` is required because the thread that deallocates a node is not the
// thread that allocated it.
unsafe impl<T: Send> Send for HazardPointers<T> {}
unsafe impl<T: Send> Sync for HazardPointers<T> {}

/// Clears the calling thread's hazard slot when it goes out of scope, on every
/// path out of the function, including an early return or a panic.
pub struct HazardGuard<'a, T: Send> {
    registry: &'a HazardPointers<T>,
    ptr: *mut T,
}

impl<T: Send> HazardGuard<'_, T> {
    pub fn as_ptr(&self) -> *mut T {
        self.ptr
    }
}

impl<T: Send> Drop for HazardGuard<'_, T> {
    fn drop(&mut self) {
        self.registry.clear_hazards();
    }
}

impl<T: Send> Default for HazardPointers<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Send> HazardPointers<T> {
    /// Reclamation is attempted once the retire list reaches this length.
    const RECLAIM_THRESHOLD: usize = 16;

    pub fn new() -> Self {
        HazardPointers {
            thread_hazards: Mutex::new(Vec::new()),
            retire_list: Mutex::new(Vec::new()),
        }
    }

    /// Publishes `ptr` as in use by the calling thread.
    ///
    /// The caller must still re-read the source location and compare it with
    /// `ptr` before dereferencing.
    pub fn protect(&self, ptr: *mut T) -> HazardGuard<'_, T> {
        if !ptr.is_null() {
            let thread_id = thread::current().id();
            let mut hazards = self
                .thread_hazards
                .lock()
                .expect("hazard list mutex poisoned");

            match hazards.iter_mut().find(|entry| entry.0 == thread_id) {
                Some(entry) => entry.1 = ptr,
                None => hazards.push((thread_id, ptr)),
            }
        }
        HazardGuard {
            registry: self,
            ptr,
        }
    }

    /// Clears the calling thread's hazard slot.
    pub fn clear_hazards(&self) {
        let thread_id = thread::current().id();
        let mut hazards = self
            .thread_hazards
            .lock()
            .expect("hazard list mutex poisoned");
        hazards.retain(|entry| entry.0 != thread_id);
    }
}

impl<T: Send> HazardPointers<T> {
    /// Hands a pointer over for deallocation once no thread is reading it.
    ///
    /// # Safety
    ///
    /// `ptr` must come from `Box::into_raw`, must already be unlinked from the
    /// shared structure, and must not be retired twice.
    pub unsafe fn retire(&self, ptr: *mut T) {
        if ptr.is_null() {
            return;
        }
        let len = {
            let mut retire = self.retire_list.lock().expect("retire list mutex poisoned");
            retire.push(ptr);
            retire.len()
        };
        // The guard above is released before try_reclaim takes the same lock:
        // std::sync::Mutex is not reentrant.
        if len >= Self::RECLAIM_THRESHOLD {
            self.try_reclaim(false);
        }
    }

    /// Frees every retired pointer that no thread has published, and returns
    /// how many it freed.
    pub fn try_reclaim(&self, force: bool) -> usize {
        let hazardous: HashSet<*mut T> = {
            let hazards = self
                .thread_hazards
                .lock()
                .expect("hazard list mutex poisoned");
            hazards.iter().map(|entry| entry.1).collect()
        };

        let to_free = {
            let mut retire = self.retire_list.lock().expect("retire list mutex poisoned");
            if retire.is_empty() || (!force && retire.len() < Self::RECLAIM_THRESHOLD) {
                return 0;
            }
            let (to_free, still_hazardous): (Vec<*mut T>, Vec<*mut T>) =
                retire.drain(..).partition(|ptr| !hazardous.contains(ptr));
            *retire = still_hazardous;
            to_free
        };

        for ptr in &to_free {
            // Safety: the pointer was retired, so it is unlinked, and it is
            // absent from every hazard slot read above.
            unsafe {
                drop(Box::from_raw(*ptr));
            }
        }
        to_free.len()
    }

    /// How many pointers are waiting for reclamation. Test helper.
    pub fn retired_len(&self) -> usize {
        self.retire_list
            .lock()
            .expect("retire list mutex poisoned")
            .len()
    }
}

impl<T: Send> Drop for HazardPointers<T> {
    fn drop(&mut self) {
        self.try_reclaim(true);
        let retire = self.retire_list.lock().expect("retire list mutex poisoned");
        assert!(
            retire.is_empty(),
            "{} retired pointers were still published when the registry was dropped",
            retire.len()
        );
    }
}

/// A stack node. `value` is moved out on pop, so the node must be deallocated
/// without dropping it a second time.
pub struct Node<T> {
    pub value: ManuallyDrop<T>,
    pub next: AtomicPtr<Node<T>>,
}

impl<T: fmt::Debug> fmt::Debug for Node<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Node")
            .field("value", &*self.value)
            .field("next", &self.next)
            .finish()
    }
}

pub struct LockFreeStack<T: Send> {
    pub head: AtomicPtr<Node<T>>,
    pub hazard_pointers: Arc<HazardPointers<Node<T>>>,
    size: AtomicUsize,
}

impl<T: Send> Default for LockFreeStack<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Send> LockFreeStack<T> {
    pub fn new() -> Self {
        LockFreeStack {
            head: AtomicPtr::new(ptr::null_mut()),
            hazard_pointers: Arc::new(HazardPointers::new()),
            size: AtomicUsize::new(0),
        }
    }

    pub fn push(&self, value: T) {
        let new_node = Box::into_raw(Box::new(Node {
            value: ManuallyDrop::new(value),
            next: AtomicPtr::new(ptr::null_mut()),
        }));

        loop {
            let current_head = self.head.load(Ordering::Acquire);
            // Safety: new_node is ours until the CAS below publishes it.
            unsafe { (*new_node).next.store(current_head, Ordering::Relaxed) };

            if self
                .head
                .compare_exchange(current_head, new_node, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                self.size.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
    }
}

impl<T: Send> LockFreeStack<T> {
    pub fn pop(&self) -> Option<T> {
        loop {
            let current_head = self.head.load(Ordering::Acquire);
            if current_head.is_null() {
                return None;
            }

            // Publish the pointer, then re-read head. The guard clears the
            // slot on every path out of this function.
            let guard = self.hazard_pointers.protect(current_head);
            if self.head.load(Ordering::Acquire) != current_head {
                continue;
            }

            // Safety: current_head is published and was still head after
            // publication, so no thread can have freed it.
            let next = unsafe { (*guard.as_ptr()).next.load(Ordering::Acquire) };

            if self
                .head
                .compare_exchange(current_head, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                // Safety: this thread won the CAS, so it alone unlinked the
                // node and alone moves the value out.
                let value = unsafe { ManuallyDrop::take(&mut (*guard.as_ptr()).value) };
                self.size.fetch_sub(1, Ordering::Relaxed);
                drop(guard);
                // Safety: the node is unlinked, retired once, and its value
                // has been moved out.
                unsafe { self.hazard_pointers.retire(current_head) };
                return Some(value);
            }
        }
    }
}

impl<T: Send> LockFreeStack<T> {
    pub fn len(&self) -> usize {
        self.size.load(Ordering::Relaxed)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T: Send> Drop for LockFreeStack<T> {
    fn drop(&mut self) {
        while self.pop().is_some() {}
        self.hazard_pointers.try_reclaim(true);
    }
}
