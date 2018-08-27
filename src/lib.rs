//! # Atomic Queue
//!
//! This is a simple lock-free atomic queue for `#![no_std]` embedded systems
//! written in Rust. The queue can be any length because the storage is
//! supplied as a mutable slice at creation time. It can handle multiple
//! concurrent readers and writers, although because it uses a spin loop
//! you'll need an pre-emptible RTOS - producing from two interrupt routines
//! is going to lead to deadlocks.
//!
//! ```rust
//! extern crate atomic_queue;
//! #[macro_use]
//! extern crate lazy_static;
//!
//! use atomic_queue::AtomicQueue;
//!
//! /// This is the static storage we use to back our queue
//! static mut STORAGE: [u8; 16] = [0; 16];
//! lazy_static! {
//!     static ref QUEUE: AtomicQueue<'static, u8> = {
//!         let m = unsafe { AtomicQueue::new(&mut STORAGE) };
//!         m
//!     };
//! }
//!
//! fn main() -> Result<(), ()> {
//!     println!("Pushed 255");
//!     QUEUE.push(255)?;
//!     println!("Popped {:?}", QUEUE.pop());
//!     Ok(())
//! }
//! ```

#![cfg_attr(not(test), no_std)]

#[macro_use]
extern crate const_ft;

#[cfg(test)]
#[macro_use]
extern crate lazy_static;

#[cfg(test)]
use std as core;

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering, ATOMIC_USIZE_INIT};

/// A queue of fixed length based around a mutable slice provided to the
/// constructor. Holds a number of some type `T`. Safe for multiple consumers
/// and producers.
pub struct AtomicQueue<'a, T>
where
    T: 'a + Copy,
{
    /// This is where we store our data
    data: UnsafeCell<&'a mut [T]>,
    /// This is the counter for the first item in the queue (i.e. the one to
    /// be pop'd next). Counters increment forever. You convert it to an
    /// array index by taking them modulo data.len() (see `counter_to_idx`).
    read: AtomicUsize,
    /// This is the counter for the last item in the queue (i.e. the most
    /// recent one pushed), whether or not the write is complete. Counters
    /// increment forever. You convert it to an array index by taking them
    /// modulo data.len() (see `counter_to_idx`).
    write: AtomicUsize,
    /// This is the counter for the last item in the queue (i.e. the most
    /// recent one pushed) where the write is actually complete. Counters
    /// increment forever. You convert it to an array index by taking them
    /// modulo data.len() (see `counter_to_idx`).
    available: AtomicUsize,
}

/// Our use of CAS atomics means we can share `AtomicQueue` between threads
/// safely.
unsafe impl<'a, T> Sync for AtomicQueue<'a, T> where T: Send + Copy {}

impl<'a, T> AtomicQueue<'a, T>
where
    T: Copy,
{
    /// Create a new queue.
    ///
    /// buffer is a mutable slice which this queue will use as storage. It
    /// needs to live at least as long as the queue does.
    const_ft! {
        pub fn new(buffer: &'a mut[T]) -> AtomicQueue<'a, T> {
            AtomicQueue {
                data: UnsafeCell::new(buffer),
                read: ATOMIC_USIZE_INIT,
                write: ATOMIC_USIZE_INIT,
                available: ATOMIC_USIZE_INIT,
            }
        }
    }

    /// Return the length of the queue. Note, we do not 'reserve' any
    /// elements, so you can actually put `N` items in a queue of length `N`.
    pub fn length(&self) -> usize {
        unsafe { (*self.data.get()).len() }
    }

    /// Add an item to the queue. An error is returned if the queue is full.
    /// You can call this from an ISR but don't call it from two different ISR
    /// concurrently. You can call it from two different threads just fine
    /// though, as long as they're pre-emptible
    pub fn push(&self, value: T) -> Result<(), ()> {
        // Loop until we've allocated ourselves some space without colliding
        // with another writer.
        let write = loop {
            let read = self.read.load(Ordering::SeqCst);
            let write = self.write.load(Ordering::SeqCst);
            if (write.wrapping_sub(read)) >= self.length() {
                // Queue is full - quit now
                return Err(());
            }
            if self
                .write
                .compare_exchange(
                    write,
                    write.wrapping_add(1),
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                )
                .is_ok()
            {
                // We've managed increment `self.write` successfully without
                // anyone else updating it underneath us. Someone could have
                // incremented read counter, but that gives us *more* space,
                // not less, so that's fine.
                break write;
            }
        };

        // This is safe because we're the only possible thread with this value
        // of idx (reading or writing).
        let p = unsafe { &mut *self.data.get() };
        p[self.counter_to_idx(write)] = value;

        // Now update `self.available` so that readers can read what we just wrote.
        loop {
            if self
                .available
                .compare_exchange(
                    write,
                    write.wrapping_add(1),
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                )
                .is_ok()
            {
                // We've managed increment `self.available` successfully without
                // anyone else updating it underneath us.
                return Ok(());
            } else {
                // `self.available` isn't as high as it should be. This means
                // there's another writer currently writing to a lower slot
                // than ours, and we need to try again until they've finished.
            }
        }
    }

    /// Take the next item off the queue. `None` is returned if the queue is
    /// empty. You can call this from multiple ISRs or threads concurrently
    /// and it should be fine.
    pub fn pop(&self) -> Option<T> {
        // Loop until we've read an item without colliding with another
        // reader.
        loop {
            let read = self.read.load(Ordering::SeqCst);
            let available = self.available.load(Ordering::SeqCst);
            if read >= available {
                // Queue is empty - quit now
                return None;
            }

            // This is safe because no-one else can be writing to this
            // location, and concurrent reads get resolved in the next block.
            let p = unsafe { &*self.data.get() };
            // Cache the result
            let result = p[self.counter_to_idx(read)];

            if self
                .read
                .compare_exchange(
                    read,
                    read.wrapping_add(1),
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                )
                .is_ok()
            {
                // We've managed increment `self.read` successfully without
                // anyone else updating it underneath us. We can now return
                // the result and leave the loop.
                return Some(result);
            }
        }
    }

    /// Counters are converted to slice indexes by taking them modulo the
    /// slice length.
    fn counter_to_idx(&self, counter: usize) -> usize {
        counter % self.length()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn create_queue() {
        let mut buffer = vec![0, 0, 0, 0];
        let q = AtomicQueue::new(&mut buffer);
        assert!(q.push(1).is_ok());
        assert!(q.push(2).is_ok());
        assert!(q.push(3).is_ok());
        assert_eq!(q.pop(), Some(1));
        assert_eq!(q.pop(), Some(2));
        assert_eq!(q.pop(), Some(3));
        assert_eq!(q.pop(), None);
        assert!(q.push(4).is_ok());
        assert_eq!(q.pop(), Some(4));
        assert_eq!(q.pop(), None);
    }

    #[test]
    fn overflow_queue() {
        let mut buffer = vec![0, 0, 0, 0];
        let q = AtomicQueue::new(&mut buffer);
        assert!(q.push(1).is_ok());
        assert!(q.push(2).is_ok());
        assert!(q.push(3).is_ok());
        assert!(q.push(4).is_ok());
        assert!(q.push(5).is_err());
        assert_eq!(q.pop(), Some(1));
        assert_eq!(q.pop(), Some(2));
        assert_eq!(q.pop(), Some(3));
        assert_eq!(q.pop(), Some(4));
        assert_eq!(q.pop(), None);
        assert!(q.push(99).is_ok());
        assert_eq!(q.pop(), Some(99));
        assert_eq!(q.pop(), None);
    }

    /// A test - start two consumers and two producers.
    #[test]
    fn threaded_test() {
        use std::thread;

        const TEST_ITEM_DATA_LEN: usize = 64 - 8;

        #[derive(Copy, Clone)]
        struct TestItem {
            data: [u8; TEST_ITEM_DATA_LEN],
            value: u64,
        }

        const COUNT: u64 = 100_000_000;
        static mut STORAGE: [TestItem; 256] = [TestItem {
            data: [0u8; TEST_ITEM_DATA_LEN],
            value: 0,
        }; 256];
        lazy_static! {
            static ref QUEUE: AtomicQueue<'static, TestItem> = {
                let m = unsafe { AtomicQueue::new(&mut STORAGE) };
                m
            };
        }

        let c1 = thread::spawn(|| {
            let mut total = 0;
            for _ in 0..COUNT {
                loop {
                    if let Some(n) = QUEUE.pop() {
                        assert!(n.data.iter().all(|x| *x == (n.value % 256) as u8));
                        total = total + n.value;
                        break;
                    }
                }
            }
            total
        });

        let c2 = thread::spawn(|| {
            let mut total = 0;
            for _ in 0..COUNT {
                loop {
                    if let Some(n) = QUEUE.pop() {
                        assert!(n.data.iter().all(|x| *x == (n.value % 256) as u8));
                        total = total + n.value;
                        break;
                    }
                }
            }
            total
        });

        thread::spawn(|| {
            for i in 0..COUNT {
                let p = TestItem {
                    data: [(i % 256) as u8; TEST_ITEM_DATA_LEN],
                    value: i,
                };
                while QUEUE.push(p).is_err() {
                    // spin
                }
            }
        });

        thread::spawn(|| {
            for i in 0..COUNT {
                let p = TestItem {
                    data: [(i % 256) as u8; TEST_ITEM_DATA_LEN],
                    value: i,
                };
                while QUEUE.push(p).is_err() {
                    // spin
                }
            }
        });

        let total1 = c1.join().unwrap();
        let total2 = c2.join().unwrap();

        let mut check = 0;
        for i in 0..COUNT {
            check += i;
        }

        assert_eq!(total1 + total2, check * 2);
    }

}
