mod queue;
mod stack;

pub use queue::LockFreeQueue;
pub use stack::LockFreeStack;

fn main() {
    println!("Running epoch-based reclamation examples...");

    // Basic stack demo
    let stack = LockFreeStack::new();
    stack.push(1).unwrap();
    stack.push(2).unwrap();
    println!("Stack size: {}", stack.len());
    println!("Popped: {:?}", stack.pop());

    // Basic queue demo
    let queue = LockFreeQueue::new();
    queue.enqueue(1);
    queue.enqueue(2);
    println!("Dequeued: {:?}", queue.dequeue());
}
