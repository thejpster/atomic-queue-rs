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

#![no_std]

#[macro_use]
extern crate const_ft;

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering, ATOMIC_USIZE_INIT};

/// A queue of fixed length based around a mutable slice provided to the
/// constructor. Holds a number of some type `T`. Safe for multiple consumers
/// and producers.
pub struct AtomicQueue<'a, T>
where
    T: 'a + Copy,
{
    data: UnsafeCell<&'a mut [T]>,
    read: AtomicUsize,
    write: AtomicUsize,
    available: AtomicUsize,
    length: usize,
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
            let length = buffer.len();
            AtomicQueue {
                data: UnsafeCell::new(buffer),
                read: ATOMIC_USIZE_INIT,
                write: ATOMIC_USIZE_INIT,
                available: ATOMIC_USIZE_INIT,
                length,
            }
        }
    }

    /// Check if the queue is full.
    pub fn is_full(&self) -> bool {
        let write_counter = self.write.load(Ordering::SeqCst);
        let read_counter = self.read.load(Ordering::SeqCst);
        (write_counter.wrapping_sub(read_counter)) >= self.length
    }

    /// Check if the queue is empty.
    pub fn is_empty(&self) -> bool {
        let available_counter = self.available.load(Ordering::SeqCst);
        let read_counter = self.read.load(Ordering::SeqCst);
        available_counter == read_counter
    }

    /// Add an item to the queue. An error is returned if the queue is full.
    pub fn push(&self, value: T) -> Result<(), ()> {
        // Loop until we've allocated ourselves some space without colliding
        // with another writer.
        let idx = loop {
            let write_counter = self.write.load(Ordering::SeqCst);
            let read_counter = self.read.load(Ordering::SeqCst);
            if (write_counter.wrapping_sub(read_counter)) >= self.length {
                // Queue is full - quit now
                return Err(());
            }
            if self.write.compare_and_swap(
                write_counter,
                write_counter.wrapping_add(1),
                Ordering::SeqCst,
            ) == write_counter
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
        loop {
            let available_counter = self.available.load(Ordering::SeqCst);
            if self.available.compare_and_swap(
                available_counter,
                available_counter.wrapping_add(1),
                Ordering::SeqCst,
            ) == available_counter
            {
                // We've managed increment `self.available` successfully without
                // anyone else updating it underneath us.
                break;
            }
        }

        Ok(())
    }

    /// Take the next item off the queue.
    pub fn pop(&self) -> Option<T> {
        // Loop until we've read an item without colliding with another
        // reader.
        loop {
            let available_counter = self.available.load(Ordering::SeqCst);
            let read_counter = self.read.load(Ordering::SeqCst);
            if available_counter == read_counter {
                // Queue is empty - quit now
                return None;
            }
            // This is safe because no-one can be writing to this slot right
            // now, and multiple-readers is OK.
            let p = unsafe { &*self.data.get() };
            // Cache the result
            let result = p[self.counter_to_idx(read_counter)];
            if self.read.compare_and_swap(
                read_counter,
                read_counter.wrapping_add(1),
                Ordering::SeqCst,
            ) == read_counter
            {
                // We've managed increment `self.read` successfully without
                // anyone else updating it underneath us. We can now return
                // the result and leave the loop.
                return Some(result);
            }
        }
    }

    fn counter_to_idx(&self, counter: usize) -> usize {
        counter % self.length
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
        assert!(!q.is_full());
        assert!(q.push(2).is_ok());
        assert!(!q.is_full());
        assert!(q.push(3).is_ok());
        assert!(!q.is_full());
        assert_eq!(q.pop(), Some(1));
        assert_eq!(q.pop(), Some(2));
        assert_eq!(q.pop(), Some(3));
        assert_eq!(q.pop(), None);
        assert!(q.is_empty());
        assert!(q.push(4).is_ok());
        assert!(!q.is_empty());
        assert_eq!(q.pop(), Some(4));
        assert!(q.is_empty());
        assert_eq!(q.pop(), None);
        assert!(q.is_empty());
    }

    #[test]
    fn overflow_queue() {
        let mut buffer = [0u32; 4];
        let q = AtomicQueue::new(&mut buffer);
        assert!(q.push(1).is_ok());
        assert!(!q.is_full());
        assert!(q.push(2).is_ok());
        assert!(!q.is_full());
        assert!(q.push(3).is_ok());
        assert!(!q.is_full());
        assert!(q.push(4).is_ok());
        assert!(q.is_full());
        assert!(q.push(5).is_err());
        assert!(q.is_full());
        assert_eq!(q.pop(), Some(1));
        assert_eq!(q.pop(), Some(2));
        assert_eq!(q.pop(), Some(3));
        assert_eq!(q.pop(), Some(4));
        assert_eq!(q.pop(), None);
        assert!(q.push(99).is_ok());
        assert_eq!(q.pop(), Some(99));
        assert_eq!(q.pop(), None);
    }
}
