//! Generation-based volatile eviction policy.
//!
//! This policy is for values that are useful during a short burst of queries but
//! can be aggressively discarded once the cache crosses a high-watermark.

use std::cell::RefCell;
use std::num::NonZeroUsize;
use std::sync::atomic::AtomicUsize as StdAtomicUsize;

use crossbeam_queue::SegQueue;

use crate::Id;
use crate::hash::{FxHashMap, FxHashSet, FxIndexSet};
use crate::sync::Mutex;
use crate::sync::atomic::{AtomicUsize, Ordering};

use super::EvictionPolicy;

const GENERATIONS_TO_KEEP: usize = 1;

static NEXT_POLICY_ID: StdAtomicUsize = StdAtomicUsize::new(0);

crate::sync::thread_local! {
    static READS: RefCell<ThreadReads> = RefCell::default();
}

#[derive(Default)]
struct ThreadReads {
    policies: FxHashMap<usize, ThreadPolicyReads>,
}

#[derive(Default)]
struct ThreadPolicyReads {
    generation: usize,
    ids: FxHashSet<Id>,
}

#[derive(Clone, Copy)]
struct Read {
    id: Id,
    generation: usize,
}

/// Evicts older values once the number of resident values crosses the configured capacity.
///
/// This is not LRU: volatile reads are only published once per id, per thread,
/// per generation. Hot reads therefore avoid the global write on every access
/// that makes LRU expensive.
pub struct Volatile {
    capacity: Option<NonZeroUsize>,
    policy_id: usize,
    generation: AtomicUsize,
    reads: SegQueue<Read>,
    state: Mutex<VolatileState>,
}

#[derive(Default)]
struct VolatileState {
    resident: FxIndexSet<Id>,
    last_read: FxHashMap<Id, usize>,
}

impl Volatile {
    #[inline]
    fn current_generation(&self) -> usize {
        self.generation.load(Ordering::Relaxed)
    }
}

impl EvictionPolicy for Volatile {
    const RETIRES_VALUES: bool = true;

    fn new(capacity: usize) -> Self {
        Self {
            capacity: NonZeroUsize::new(capacity),
            policy_id: NEXT_POLICY_ID.fetch_add(1, Ordering::Relaxed),
            generation: AtomicUsize::new(1),
            reads: SegQueue::new(),
            state: Mutex::default(),
        }
    }

    #[inline(always)]
    fn record_use(&self, id: Id) {
        if self.capacity.is_none() {
            return;
        }

        let generation = self.current_generation();
        READS.with(|reads| {
            let mut reads = reads.borrow_mut();
            let policy_reads = reads.policies.entry(self.policy_id).or_default();

            if policy_reads.generation != generation {
                policy_reads.generation = generation;
                policy_reads.ids.clear();
            }

            if policy_reads.ids.insert(id) {
                self.reads.push(Read { id, generation });
            }
        });
    }

    #[inline(always)]
    fn record_insert(&self, id: Id) -> bool {
        let Some(capacity) = self.capacity else {
            return false;
        };

        let generation = self.current_generation();
        let mut state = self.state.lock();
        state.resident.insert(id);
        state.last_read.insert(id, generation);

        if state.resident.len() > capacity.get() {
            self.generation.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    #[inline(always)]
    fn set_capacity(&mut self, capacity: usize) {
        self.capacity = NonZeroUsize::new(capacity);
        if self.capacity.is_none() {
            let state = self.state.get_mut();
            state.resident.clear();
            state.last_read.clear();
        }
    }

    fn for_each_evicted(&self, mut cb: impl FnMut(Id) -> bool) {
        let Some(capacity) = self.capacity else {
            return;
        };

        let generation = self.current_generation();
        let mut state = self.state.lock();

        while let Some(read) = self.reads.pop() {
            if let Some(last_read) = state.last_read.get_mut(&read.id) {
                *last_read = (*last_read).max(read.generation);
            }
        }

        let mut len = state.resident.len();
        if len <= capacity.get() {
            return;
        }

        let evict_before = generation.saturating_sub(GENERATIONS_TO_KEEP);
        let resident = std::mem::take(&mut state.resident);
        let mut retained = FxIndexSet::default();

        for id in resident {
            if len <= capacity.get() {
                retained.insert(id);
                continue;
            }

            let last_read = state.last_read.get(&id).copied().unwrap_or_default();
            if last_read < evict_before && cb(id) {
                state.last_read.remove(&id);
                len -= 1;
            } else {
                retained.insert(id);
            }
        }

        state.resident = retained;
    }
}
