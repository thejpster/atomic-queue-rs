# Atomic Queue

This is a simple lock-free atomic queue for `#![no_std]` embedded systems written in Rust.

It will compile on any target with Atomic Compare-and-Swap (e.g. `thumbv7m-none-eabi` is OK, but 
`thumbv6-none-eabi` is not).

```rust
use atomic_queue::AtomicQueue;

/// This is the static storage we use to back our queue
static QUEUE: AtomicQueue<u8, 16> = AtomicQueue::new([0u8; 16]);

fn main() -> Result<(), ()> {
	println!("Pushed 255");
	QUEUE.push(255)?;
	println!("Popped {:?}", QUEUE.pop());
	Ok(())
}
```
