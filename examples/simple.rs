use atomic_queue::AtomicQueue;

/// This is the static storage we use to back our queue
static QUEUE: AtomicQueue<u8, 16> = AtomicQueue::new([0u8; 16]);

fn main() -> Result<(), ()> {
	println!("Pushed 255");
	QUEUE.push(255)?;
	println!("Popped {:?}", QUEUE.pop());
	Ok(())
}
