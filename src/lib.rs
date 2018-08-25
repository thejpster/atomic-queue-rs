#![no_std]

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering, ATOMIC_USIZE_INIT};

/// A queue of fixed length 4. Holds any type `T`. Safe to `push()` from one
/// ISR and `pop()` from the main thread.
///
/// It is unsafe to `push()` from multiple ISRs that can pre-empt each other.
pub struct FixedQueue4<T>
where
    T: Copy,
{
    data: UnsafeCell<[T; 4]>,
    read: AtomicUsize,
    write: AtomicUsize,
    written: AtomicUsize,
}

impl<T> FixedQueue4<T>
where
    T: Copy,
{
    const QUEUE_LEN: usize = 4;

    /// Create a new fixed-length queue.
    pub fn new(default: T) -> FixedQueue4<T> {
        FixedQueue4 {
            data: UnsafeCell::new([default; 4]),
            read: ATOMIC_USIZE_INIT,
            write: ATOMIC_USIZE_INIT,
            written: ATOMIC_USIZE_INIT,
        }
    }

    fn counter_to_idx(&self, counter: usize) -> usize {
        counter % Self::QUEUE_LEN
    }

    pub fn is_full(&self) -> bool {
        let write_counter = self.write.load(Ordering::SeqCst);
        let read_counter = self.read.load(Ordering::SeqCst);
        (write_counter.wrapping_sub(read_counter)) >= Self::QUEUE_LEN
    }

    pub fn is_empty(&self) -> bool {
        let written_counter = self.written.load(Ordering::SeqCst);
        let read_counter = self.read.load(Ordering::SeqCst);
        written_counter == read_counter
    }

    /// Add an item to the queue. An error is returned if the queue is full.
    pub fn push(&self, value: T) -> Result<(), ()> {
        // Loop until we've allocated ourselves some space without colliding
        // with another writer.
        let idx = loop {
            let write_counter = self.write.load(Ordering::SeqCst);
            let read_counter = self.read.load(Ordering::SeqCst);
            if (write_counter.wrapping_sub(read_counter)) >= Self::QUEUE_LEN {
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

        // Now update `self.written` so that readers can read what we just wrote.
        loop {
            let written_counter = self.written.load(Ordering::SeqCst);
            if self.written.compare_and_swap(
                written_counter,
                written_counter.wrapping_add(1),
                Ordering::SeqCst,
            ) == written_counter
            {
                // We've managed increment `self.written` successfully without
                // anyone else updating it underneath us.
                break;
            }
        }

        Ok(())
    }

    /// Look at the next item in the queue, but do not remove it.
    pub fn peek(&self) -> Option<T> {
        let wp = self.write.load(Ordering::SeqCst);
        let rp = self.read.load(Ordering::SeqCst);
        if wp > rp {
            // We have data
            let read_idx = rp % Self::QUEUE_LEN;
            let p = unsafe { &*self.data.get() };
            Some(p[read_idx])
        } else {
            None
        }
    }

    /// Take the next item off the queue.
    pub fn pop(&self) -> Option<T> {
        let wp = self.write.load(Ordering::SeqCst);
        let rp = self.read.load(Ordering::SeqCst);
        if wp > rp {
            // We have data
            let read_idx = rp % Self::QUEUE_LEN;
            let p = unsafe { &*self.data.get() };
            let result = Some(p[read_idx]);
            self.read.fetch_add(1, Ordering::SeqCst);
            result
        } else {
            None
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn create_queue() {
        let q = FixedQueue4::new(0u32);
        assert!(q.push(1).is_ok());
        assert!(!q.is_full());
        assert!(q.push(2).is_ok());
        assert!(!q.is_full());
        assert!(q.push(3).is_ok());
        assert!(!q.is_full());
        assert_eq!(q.peek(), Some(1));
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
        let q = FixedQueue4::new(0u32);
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
