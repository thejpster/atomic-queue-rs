//! # Atomic Queue
//!
//! This is a simple lock-free atomic queue for `#![no_std]` embedded systems
//! written in Rust. The queue can be any length because the storage is
//! supplied as a mutable slice at creation time.
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

    pub fn length(&self) -> usize {
        unsafe { (*self.data.get()).len() }
    }

    /// Add an item to the queue. An error is returned if the queue is full.
    pub fn push(&self, value: T) -> Result<(), ()> {
        // Loop until we've allocated ourselves some space without colliding
        // with another writer.
        let idx = loop {
            let write_counter = self.write.load(Ordering::Relaxed);
            let read_counter = self.read.load(Ordering::Acquire);
            if (write_counter.wrapping_sub(read_counter)) >= self.length() {
                // Queue is full - quit now
                return Err(());
            }
            if self
                .write
                .compare_exchange_weak(
                    write_counter,
                    write_counter.wrapping_add(1),
                    Ordering::Release,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                // We've managed increment `self.write` successfully without
                // anyone else updating it underneath us.
                break self.counter_to_idx(write_counter);
            }
        };
        // This is safe because we're the only possible thread with this value
        // of idx (reading or writing).
        let p = unsafe { &mut *self.data.get() };
        p[idx] = value;

        // Now update `self.available` so that readers can read what we just wrote.
        self.available.fetch_add(1, Ordering::Release);

        Ok(())
    }

    /// Take the next item off the queue.
    pub fn pop(&self) -> Option<T> {
        // Loop until we've read an item without colliding with another
        // reader.
        loop {
            let read_counter = self.read.load(Ordering::Acquire);
            let available_counter = self.available.load(Ordering::Acquire);
            if available_counter == read_counter {
                // Queue is empty - quit now
                return None;
            }

            // This is safe because if someone is writing to this exact
            // location, we'll detect that next and retry.
            let p = unsafe { &*self.data.get() };
            // Cache the result
            let result = p[self.counter_to_idx(read_counter)];

            let write_counter = self.write.load(Ordering::Relaxed);
            if write_counter != available_counter {
                // Queue is currently being written to - try again
                continue;
            }

            if self
                .read
                .compare_exchange_weak(
                    read_counter,
                    read_counter.wrapping_add(1),
                    Ordering::Release,
                    Ordering::Relaxed,
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

    fn counter_to_idx(&self, counter: usize) -> usize {
        counter % self.length()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn create_queue() {
        let mut buffer = [0u32; 4];
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
        let mut buffer = [0u32; 4];
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
    /// Producer A produces 1..100.map(|x| x*2).
    /// Producer B produces 1..100.map(|x| x*3).
    /// Consumer A and B pop from the queue and sums the values.
    /// Sum the result of A and B and it should be 1..100.map(|x| x*5)
    ///
    /// This currently fails - I think because it's possible to have one
    /// producer interrupt another and for the first item being added to be
    /// marked as 'available' when it in fact isn't. It's fine with a single
    /// producer.
    #[test]
    fn threaded_test() {
        use std::thread;
        const COUNT: u64 = 10_000_000;
        static mut STORAGE: [u64; 256] = [0u64; 256];
        lazy_static! {
            static ref QUEUE: AtomicQueue<'static, u64> = {
                let m = unsafe { AtomicQueue::new(&mut STORAGE) };
                m
            };
        }

        thread::spawn(|| {
            for i in 0..COUNT {
                loop {
                    if QUEUE.push(i * 3).is_ok() {
                        break;
                    }
                }
            }
        });
        thread::spawn(|| {
            for i in 0..COUNT {
                loop {
                    if QUEUE.push(i * 2).is_ok() {
                        break;
                    }
                }
            }
        });

        let c1 = thread::spawn(|| {
            let mut total = 0;
            for _ in 0..COUNT {
                loop {
                    if let Some(n) = QUEUE.pop() {
                        total = total + n;
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
                        total = total + n;
                        break;
                    }
                }
            }
            total
        });

        let total1 = c1.join().unwrap();
        let total2 = c2.join().unwrap();

        let mut check = 0;
        for i in 0..COUNT {
            check += i * 5;
        }

        assert_eq!(total1 + total2, check);
    }

}
