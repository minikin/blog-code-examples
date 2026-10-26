//! Lock-free stack with a version-tagged head, the code for part 1 of the ABA
//! series: <https://minikin.me/blog/solving-the-aba-problem-in-rust-tagged-pointers/>

use portable_atomic::{AtomicU128, Ordering};
use std::mem::MaybeUninit;
use std::ptr;
use std::sync::atomic::AtomicPtr;

/// A pointer paired with a version counter. Packed into one `u128` so a single
/// CAS covers both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TaggedPtr {
    ptr: *mut Node,
    version: u64,
}

impl TaggedPtr {
    /// Lower 64 bits: the address. Upper 64 bits: the version.
    fn pack(self) -> u128 {
        // expose_provenance, not addr: the address is cast back to a pointer in
        // unpack, and addr deliberately strips the provenance that makes the
        // round trip legal.
        let addr = self.ptr.expose_provenance() as u64;
        (addr as u128) | ((self.version as u128) << 64)
    }

    fn unpack(value: u128) -> Self {
        let ptr = ptr::with_exposed_provenance_mut::<Node>(value as u64 as usize);
        TaggedPtr {
            ptr,
            version: (value >> 64) as u64,
        }
    }
}

struct Node {
    value: MaybeUninit<i32>,
    /// Atomic because a thread holding a stale `TaggedPtr` may read this while
    /// the node's current owner writes it.
    next: AtomicPtr<Node>,
    /// Links every node ever allocated, for `Drop`. Written once before the
    /// node is published, never again.
    all_next: *mut Node,
}

struct AtomicTaggedPtr {
    inner: AtomicU128,
}

impl AtomicTaggedPtr {
    fn new() -> Self {
        AtomicTaggedPtr {
            inner: AtomicU128::new(
                TaggedPtr {
                    ptr: ptr::null_mut(),
                    version: 0,
                }
                .pack(),
            ),
        }
    }

    fn load(&self, ordering: Ordering) -> TaggedPtr {
        TaggedPtr::unpack(self.inner.load(ordering))
    }

    /// Swaps in `new_ptr` and bumps the version, so the result never compares
    /// equal to `current` again.
    fn compare_and_swap(
        &self,
        current: TaggedPtr,
        new_ptr: *mut Node,
        success_order: Ordering,
        failure_order: Ordering,
    ) -> Result<u64, TaggedPtr> {
        let new = TaggedPtr {
            ptr: new_ptr,
            version: current.version.wrapping_add(1),
        };
        self.inner
            .compare_exchange(current.pack(), new.pack(), success_order, failure_order)
            .map(|_| new.version)
            .map_err(TaggedPtr::unpack)
    }
}

/// A lock-free stack with a version-tagged head.
pub struct LockFreeStack {
    head: AtomicTaggedPtr,
    /// Nodes that have been popped and are ready for reuse.
    free: AtomicTaggedPtr,
    /// Every node this stack has allocated, freed only in `Drop`.
    all: AtomicPtr<Node>,
}

impl LockFreeStack {
    pub fn new() -> Self {
        LockFreeStack {
            head: AtomicTaggedPtr::new(),
            free: AtomicTaggedPtr::new(),
            all: AtomicPtr::new(ptr::null_mut()),
        }
    }

    /// Takes a node off the free list, or allocates one.
    fn alloc(&self, value: i32) -> *mut Node {
        loop {
            let current = self.free.load(Ordering::Acquire);
            if current.ptr.is_null() {
                break;
            }
            // SAFETY: nodes on the free list are live. Nothing is deallocated
            // until the stack is dropped, so a stale `current.ptr` still points
            // at a valid node and `next` is read atomically.
            let next = unsafe { (*current.ptr).next.load(Ordering::Acquire) };
            if self
                .free
                .compare_and_swap(current, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                // SAFETY: the CAS removed this node from the free list, so this
                // thread owns it exclusively.
                unsafe { (*current.ptr).value = MaybeUninit::new(value) };
                return current.ptr;
            }
        }

        let node = Box::into_raw(Box::new(Node {
            value: MaybeUninit::new(value),
            next: AtomicPtr::new(ptr::null_mut()),
            all_next: ptr::null_mut(),
        }));
        loop {
            let head = self.all.load(Ordering::Relaxed);
            // SAFETY: `node` is not published yet, so this write races with
            // nothing.
            unsafe { (*node).all_next = head };
            if self
                .all
                .compare_exchange_weak(head, node, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                return node;
            }
        }
    }

    pub fn push(&self, value: i32) {
        let new_node = self.alloc(value);
        loop {
            let current = self.head.load(Ordering::Relaxed);
            // SAFETY: this thread owns `new_node` until the CAS publishes it.
            unsafe { (*new_node).next.store(current.ptr, Ordering::Relaxed) };
            if self
                .head
                .compare_and_swap(current, new_node, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                return;
            }
        }
    }

    pub fn pop(&self) -> Option<i32> {
        loop {
            let current = self.head.load(Ordering::Acquire);
            if current.ptr.is_null() {
                return None;
            }
            // SAFETY: the node is live even if another thread has already popped
            // it, because nodes are pooled rather than freed. A stale `next`
            // only costs a retry, since the tag will have moved on.
            let next = unsafe { (*current.ptr).next.load(Ordering::Acquire) };
            if self
                .head
                .compare_and_swap(current, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                // SAFETY: the CAS unlinked this node, so this thread is the only
                // one that can read its value.
                let value = unsafe { (*current.ptr).value.assume_init() };
                self.recycle(current.ptr);
                return Some(value);
            }
        }
    }

    /// Returns a popped node to the free list.
    fn recycle(&self, node: *mut Node) {
        loop {
            let current = self.free.load(Ordering::Relaxed);
            // SAFETY: this thread owns `node` until the CAS publishes it.
            unsafe { (*node).next.store(current.ptr, Ordering::Relaxed) };
            if self
                .free
                .compare_and_swap(current, node, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                return;
            }
        }
    }

    /// The head's current version. Useful for watching the tag move.
    pub fn version(&self) -> u64 {
        self.head.load(Ordering::Acquire).version
    }
}

impl Default for LockFreeStack {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for LockFreeStack {
    fn drop(&mut self) {
        let mut node = *self.all.get_mut();
        while !node.is_null() {
            // SAFETY: `&mut self` means no other thread holds a pointer into the
            // pool, and every node on this list came from `Box::into_raw`.
            let boxed = unsafe { Box::from_raw(node) };
            node = boxed.all_next;
        }
    }
}

fn main() {
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::thread;

    let stack = Arc::new(LockFreeStack::new());
    stack.push(1);
    stack.push(2);
    stack.push(3);

    let (read_tx, read_rx) = mpsc::channel();
    let (aba_tx, aba_rx) = mpsc::channel();

    let t1 = {
        let stack = Arc::clone(&stack);
        thread::spawn(move || {
            let seen = stack.head.load(Ordering::Acquire);
            println!("T1 read the head at version {}", seen.version);
            read_tx.send(()).unwrap();
            aba_rx.recv().unwrap();

            // SAFETY: pooled nodes stay live, so this read is valid even though
            // T2 has popped this node in the meantime.
            let next = unsafe { (*seen.ptr).next.load(Ordering::Acquire) };
            match stack
                .head
                .compare_and_swap(seen, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(v) => println!("T1 CAS succeeded, head is now version {v}"),
                Err(actual) => println!(
                    "T1 CAS failed: it expected version {}, found {}",
                    seen.version, actual.version
                ),
            }
        })
    };

    let t2 = {
        let stack = Arc::clone(&stack);
        thread::spawn(move || {
            read_rx.recv().unwrap();
            let top = stack.pop().unwrap();
            println!("T2 popped {top}");
            println!("T2 popped {}", stack.pop().unwrap());
            stack.push(top);
            println!(
                "T2 pushed {top} back, head is at version {}",
                stack.version()
            );
            aba_tx.send(()).unwrap();
        })
    };

    t1.join().unwrap();
    t2.join().unwrap();

    print!("stack from the top:");
    while let Some(value) = stack.pop() {
        print!(" {value}");
    }
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn pops_in_lifo_order() {
        let stack = LockFreeStack::new();
        stack.push(1);
        stack.push(2);
        stack.push(3);

        assert_eq!(stack.pop(), Some(3));
        assert_eq!(stack.pop(), Some(2));
        assert_eq!(stack.pop(), Some(1));
        assert_eq!(stack.pop(), None);
    }

    #[test]
    fn empty_stack_pops_none() {
        assert_eq!(LockFreeStack::new().pop(), None);
    }

    #[test]
    fn extreme_values_round_trip() {
        let stack = LockFreeStack::new();
        for value in [i32::MAX, i32::MIN, 0] {
            stack.push(value);
            assert_eq!(stack.pop(), Some(value));
        }
    }

    #[test]
    fn the_tag_advances_on_every_successful_cas() {
        let stack = LockFreeStack::new();
        let start = stack.version();
        stack.push(1);
        stack.push(2);
        stack.pop();
        assert_eq!(stack.version(), start + 3);
    }

    /// The point of the tag: a CAS holding a stale version fails even when the
    /// head points at the same address again.
    #[test]
    fn a_stale_tag_loses_the_cas() {
        let stack = LockFreeStack::new();
        stack.push(1);
        stack.push(2);

        let stale = stack.head.load(Ordering::Acquire);
        let top = stack.pop().unwrap();
        stack.push(top);

        let fresh = stack.head.load(Ordering::Acquire);
        assert_eq!(
            fresh.ptr, stale.ptr,
            "the node was recycled, so A came back"
        );
        assert_ne!(fresh.version, stale.version);

        let next = unsafe { (*stale.ptr).next.load(Ordering::Acquire) };
        assert!(stack
            .head
            .compare_and_swap(stale, next, Ordering::AcqRel, Ordering::Acquire)
            .is_err());
    }

    #[test]
    fn concurrent_pushes_all_arrive() {
        let stack = Arc::new(LockFreeStack::new());
        let threads = 4;
        let per_thread = 100;

        let handles: Vec<_> = (0..threads)
            .map(|id| {
                let stack = Arc::clone(&stack);
                thread::spawn(move || {
                    for i in 0..per_thread {
                        stack.push(id * per_thread + i);
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }

        let mut seen = HashSet::new();
        while let Some(value) = stack.pop() {
            assert!(seen.insert(value), "{value} was popped twice");
        }
        assert_eq!(seen.len(), (threads * per_thread) as usize);
    }

    #[test]
    fn every_value_is_popped_exactly_once() {
        let stack = Arc::new(LockFreeStack::new());
        let pushers = 3;
        let poppers = 2;
        let per_thread = 100;
        let total = pushers * per_thread;

        let push_handles: Vec<_> = (0..pushers)
            .map(|id| {
                let stack = Arc::clone(&stack);
                thread::spawn(move || {
                    for i in 0..per_thread {
                        stack.push(id * per_thread + i);
                    }
                })
            })
            .collect();

        let pop_handles: Vec<_> = (0..poppers)
            .map(|_| {
                let stack = Arc::clone(&stack);
                thread::spawn(move || {
                    let mut mine = Vec::new();
                    for _ in 0..total {
                        if let Some(value) = stack.pop() {
                            mine.push(value);
                        }
                        thread::yield_now();
                    }
                    mine
                })
            })
            .collect();

        for handle in push_handles {
            handle.join().unwrap();
        }
        let mut seen = HashSet::new();
        for handle in pop_handles {
            for value in handle.join().unwrap() {
                assert!(seen.insert(value), "{value} was popped twice");
            }
        }
        while let Some(value) = stack.pop() {
            assert!(seen.insert(value), "{value} was popped twice");
        }
        assert_eq!(seen.len(), total as usize);
    }
}
