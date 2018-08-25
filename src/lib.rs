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
        }
    }

    /// Add an item to the queue. An error is returned if the queue is full.
    pub fn push(&self, value: T) -> Result<(), ()> {
        let wp = self.write.load(Ordering::SeqCst);
        let rp = self.read.load(Ordering::SeqCst);
        if (wp - rp) < Self::QUEUE_LEN {
            let write_idx = self.write.load(Ordering::SeqCst) % Self::QUEUE_LEN;
            unsafe {
                let p = &mut *self.data.get();
                p[write_idx] = value;
            }
            self.write.fetch_add(1, Ordering::SeqCst);
            Ok(())
        } else {
            Err(())
        }
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
        assert!(q.push(2).is_ok());
        assert!(q.push(3).is_ok());
        assert_eq!(q.peek(), Some(1));
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
        let q = FixedQueue4::new(0u32);
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
}
