use ebr_aba_protection::{LockFreeQueue, LockFreeStack};

fn main() {
    println!("Running epoch-based reclamation examples...");

    let stack = LockFreeStack::new();
    stack.push(1).unwrap();
    stack.push(2).unwrap();
    println!("Stack size: {}", stack.len());
    println!("Popped: {:?}", stack.pop());

    let queue = LockFreeQueue::new();
    queue.enqueue(1);
    queue.enqueue(2);
    println!("Dequeued: {:?}", queue.dequeue());
}
