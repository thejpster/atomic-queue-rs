extern crate atomic_queue;
#[macro_use]
extern crate lazy_static;

use atomic_queue::AtomicQueue;

/// This is the static storage we use to back our queue
static mut STORAGE: [u8; 16] = [0; 16];

/// This is our queue, backed by `STORAGE`. We have to use `lazy_static!` to
/// initialise this / because Rust won't let us have a `static` containing a
/// mutable reference to another `static`.
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
