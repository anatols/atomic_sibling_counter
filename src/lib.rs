#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

use std::{
    ptr::NonNull,
    sync::atomic::{AtomicU64, Ordering},
};

/// A counter that can mark new siblings and return their total count.
///
/// This type can be seen conceptually as a weak pointer to the underlying counter
/// (with [`SiblingToken`] being a strong pointer).
///
/// You can have as many clones of `SiblingCounter` as you want and pass them freely between
/// threads. Instances of `SiblingCounter` are *not* counted as siblings.
///
/// You do *not* need to keep any instances of `SiblingCounter` alive for the underlying counter to work.
/// As long as any token is alive, you can create new instances of `SiblingCounter` from it (and new tokens too).
/// # Panics
///
/// You can have a maximum of `u32::MAX - 1_000_000` `SiblingCounter` instances for each underlying counter.
/// Adding more will result in a panic.
pub struct SiblingCounter {
    counters: NonNull<AtomicU64>,
}

impl SiblingCounter {
    /// Creates a new counter with sibling count of 0.
    pub fn new() -> Self {
        Self {
            counters: new_reference_counters(CounterPart::Counter),
        }
    }

    /// Safety: counters must point to a valid Box-allocated counter.
    unsafe fn with_counters(counters: NonNull<AtomicU64>) -> Self {
        add_reference(counters, CounterPart::Counter);
        Self { counters }
    }

    /// Creates a new token that refers to the same underlying counter, thus incrementing the sibling count by 1.
    pub fn add_sibling(&self) -> SiblingToken {
        unsafe {
            // Safety: counters pointer is valid since self exists
            SiblingToken::with_counters(self.counters)
        }
    }

    /// Returns the total number of siblings (i.e. the total number of existing tokens).
    pub fn sibling_count(&self) -> usize {
        unsafe {
            // Safety: counters pointer is valid since self exists
            sibling_count(self.counters)
        }
    }
}

impl Default for SiblingCounter {
    /// Creates a new counter with sibling count of 0.
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SiblingCounter {
    /// Creates a new instance of `SiblingCounter` that refers to the same underlying counter.
    /// Sibling count is not affected.
    fn clone(&self) -> Self {
        unsafe {
            // Safety: counters pointer is valid since self exists
            Self::with_counters(self.counters)
        }
    }
}

impl Drop for SiblingCounter {
    /// Drops the underlying counter if the dropped instance is the last thing that refers to it.
    fn drop(&mut self) {
        unsafe {
            // Safety: drop is called before self.counters is dropped, and the fact that
            // self exists means that counters pointer is valid.
            remove_reference(self.counters, CounterPart::Counter);
        }
    }
}

unsafe impl Send for SiblingCounter {}
unsafe impl Sync for SiblingCounter {}

/// A token that marks a sibling.
///
/// A sibling is marked by making it retain an instance of [`SiblingToken`]. Dropping the token decrements the sibling count.
/// Tokens can be cloned, each clone would refer to the same underlying counter (thus, incrementing the sibling count).
///
/// This type can be seen conceptually as a strong pointer to the underlying counter
/// (with [`SiblingCounter`] being a weak pointer).
///
/// # Panics
///
/// You can have a maximum of `u32::MAX - 1_000_000` siblings for each underlying counter. Adding more will result in a panic.
pub struct SiblingToken {
    counters: NonNull<AtomicU64>,
}

impl SiblingToken {
    /// Creates a new token with sibling count of 1.
    pub fn new() -> Self {
        Self {
            counters: new_reference_counters(CounterPart::Token),
        }
    }

    /// Safety: counters pointer must point to a valid Box-allocated counter
    unsafe fn with_counters(counters: NonNull<AtomicU64>) -> Self {
        add_reference(counters, CounterPart::Token);
        Self { counters }
    }

    /// Creates a new instance of [`SiblingCounter`] that refers to the same underlying counter.
    pub fn counter(&self) -> SiblingCounter {
        unsafe {
            // Safety: counters pointer is valid since self exists
            SiblingCounter::with_counters(self.counters)
        }
    }

    /// Creates a new token that refers to the same underlying counter, thus incrementing the sibling count by 1.
    ///
    /// Cloning the token has the same effect.
    pub fn add_sibling(&self) -> SiblingToken {
        self.clone()
    }

    /// Returns the total number of siblings (i.e. the total number of existing tokens, including `self`).
    pub fn sibling_count(&self) -> usize {
        unsafe {
            // Safety: counters pointer is valid since self exists
            sibling_count(self.counters)
        }
    }
}

impl Default for SiblingToken {
    /// Creates a new token with sibling count of 1.
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SiblingToken {
    /// Creates a new token that refers to the same underlying counter, thus incrementing the sibling count by 1.
    ///
    /// Calling [`add_sibling()`](SiblingToken::add_sibling()) has the same effect.
    fn clone(&self) -> Self {
        unsafe {
            // Safety: counters pointer is valid since self exists
            Self::with_counters(self.counters)
        }
    }
}

impl Drop for SiblingToken {
    /// Reduces the sibling count by 1.
    ///
    /// If the dropped instance is the last thing that refers to the underlying counter, the
    /// underlying counter is dropped.
    fn drop(&mut self) {
        unsafe {
            // Safety: drop is called before self.counters is dropped, and the fact that
            // self exists means that counters pointer is valid.
            remove_reference(self.counters, CounterPart::Token);
        }
    }
}

unsafe impl Send for SiblingToken {}
unsafe impl Sync for SiblingToken {}

#[derive(Copy, Clone)]
enum CounterPart {
    Token,
    Counter,
}

#[inline]
fn new_reference_counters(initiator: CounterPart) -> NonNull<AtomicU64> {
    let one = match initiator {
        CounterPart::Token => 1,
        CounterPart::Counter => 0x1_00_00_00_00,
    };

    unsafe {
        // Safety: Box::into_raw is guaranteed to produce a non-null pointer
        NonNull::new_unchecked(Box::into_raw(Box::new(AtomicU64::new(one))))
    }
}

#[inline]
/// Safety: counters pointer must point to a valid Box-allocated counter
unsafe fn add_reference(counters: NonNull<AtomicU64>, part: CounterPart) {
    let one = match part {
        CounterPart::Token => 1,
        CounterPart::Counter => 0x1_00_00_00_00,
    };

    let old_counters = counters.as_ref().fetch_add(one, Ordering::Relaxed);
    let (sibling_count, issuer_count) = split_counters(old_counters);
    assert!(sibling_count < u32::MAX - 1_000_000, "too many siblings");
    assert!(issuer_count < u32::MAX - 1_000_000, "too many counter");
}

#[inline]
/// Safety: counters pointer must point to a valid Box-allocated counter.
/// If this was the last overall reference, upon return the Box will have been deallocated
/// and any pointers to it are dangling.
unsafe fn remove_reference(counters: NonNull<AtomicU64>, part: CounterPart) {
    let one = match part {
        CounterPart::Token => 1,
        CounterPart::Counter => 0x1_00_00_00_00,
    };

    let old_counters = counters.as_ref().fetch_sub(one, Ordering::Relaxed);

    // If we were the last referring instance, drop the box
    if old_counters == one {
        // Safety: we know that counters pointer came from a properly allocated box.
        // After dropping the box the pointer will dangle, not using it is a
        // responsibility of the caller of this function.
        drop(Box::from_raw(counters.as_ptr()));
    }
}

#[inline]
/// Returns (sibling_count, issuer_count)
fn split_counters(counters: u64) -> (u32, u32) {
    ((counters & 0xFF_FF_FF_FF) as u32, (counters >> 32) as u32)
}

#[inline]
/// Safety: counters pointer must point to a valid Box-allocated counter.
unsafe fn sibling_count(counters: NonNull<AtomicU64>) -> usize {
    split_counters(counters.as_ref().load(Ordering::Relaxed)).0 as usize
}
