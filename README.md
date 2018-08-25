# Atomic Queue

This is a simple lock-free atomic queue for `#![no_std]` embedded systems written in Rust. The queue can be any length because the storage is supplied as a mutable slice at creation time.

```rust
extern crate atomic_queue;
#[macro_use]
extern crate lazy_static;

use atomic_queue::AtomicQueue;

/// This is the static storage we use to back our queue
static mut STORAGE: [u8; 16] = [0; 16];
lazy_static! {
    static ref QUEUE: AtomicQueue<'static, u8> = {
        let m = unsafe { AtomicQueue::new(&mut STORAGE) };
        m
    };
}

fn main() -> Result<(), ()> {
    println!("Pushed 255");
    QUEUE.push(255)?;
    println!("Popped {:?}", QUEUE.pop());
    Ok(())
}
```
