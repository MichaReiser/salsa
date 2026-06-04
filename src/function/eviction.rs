//! Pluggable cache eviction strategies for memoized function values.
//!
//! This module provides the [`EvictionPolicy`] trait that allows different
//! eviction strategies to be used for salsa tracked functions.

mod lru;
mod noop;
mod volatile;

pub use lru::Lru;
pub use noop::NoopEviction;
pub use volatile::Volatile;

use crate::Id;

/// Trait for cache eviction strategies.
///
/// Implementations control when memoized values are evicted from the cache.
/// The eviction policy is selected at compile time via the `Configuration` trait.
pub trait EvictionPolicy: Send + Sync {
    /// Whether this policy can retire memo values within a revision.
    const RETIRES_VALUES: bool = false;

    /// Create a new eviction policy with the given capacity.
    fn new(capacity: usize) -> Self;

    /// Record that an item was accessed.
    fn record_use(&self, id: Id);

    /// Record that an item had a value inserted into its memo.
    ///
    /// Returns `true` if the policy crossed an eviction point and the
    /// ingredient should run an eviction pass.
    fn record_insert(&self, id: Id) -> bool {
        self.record_use(id);
        false
    }

    /// Set the maximum capacity.
    fn set_capacity(&mut self, capacity: usize);

    /// Iterate over items that should be evicted.
    ///
    /// Called at eviction points, including `reset_for_new_revision`.
    /// The callback `cb` should be invoked for each item to evict.
    fn for_each_evicted(&self, cb: impl FnMut(Id) -> bool);
}

/// Marker trait for eviction policies that have a configurable capacity.
///
/// This trait is used to conditionally generate the `set_lru_capacity` method
/// on tracked functions. Only policies that implement this trait will expose
/// runtime capacity configuration.
pub trait HasCapacity: EvictionPolicy {}
