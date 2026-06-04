#![cfg(feature = "inventory")]

//! Tests for volatile tracked functions.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use salsa::{Database as _, Durability};
use test_log::test;

#[salsa::input]
struct MyInput {
    field: u32,
}

thread_local! {
    static VOLATILE_EXECUTIONS: AtomicUsize = const { AtomicUsize::new(0) };
    static OUTER_EXECUTIONS: AtomicUsize = const { AtomicUsize::new(0) };
    static CYCLE_EXECUTIONS: AtomicUsize = const { AtomicUsize::new(0) };
    static LIVE_VALUES: AtomicUsize = const { AtomicUsize::new(0) };
}

fn reset_counts() {
    VOLATILE_EXECUTIONS.with(|n| n.store(0, Ordering::SeqCst));
    OUTER_EXECUTIONS.with(|n| n.store(0, Ordering::SeqCst));
    CYCLE_EXECUTIONS.with(|n| n.store(0, Ordering::SeqCst));
}

fn volatile_executions() -> usize {
    VOLATILE_EXECUTIONS.with(|n| n.load(Ordering::SeqCst))
}

fn outer_executions() -> usize {
    OUTER_EXECUTIONS.with(|n| n.load(Ordering::SeqCst))
}

fn cycle_executions() -> usize {
    CYCLE_EXECUTIONS.with(|n| n.load(Ordering::SeqCst))
}

#[derive(PartialEq, Eq)]
struct LiveValue;

impl LiveValue {
    fn new() -> Self {
        LIVE_VALUES.with(|n| n.fetch_add(1, Ordering::SeqCst));
        Self
    }
}

impl Drop for LiveValue {
    fn drop(&mut self) {
        LIVE_VALUES.with(|n| n.fetch_sub(1, Ordering::SeqCst));
    }
}

fn live_values() -> usize {
    LIVE_VALUES.with(|n| n.load(Ordering::SeqCst))
}

#[salsa::tracked(volatile = 2)]
fn volatile_value(db: &dyn salsa::Database, input: MyInput) -> u32 {
    VOLATILE_EXECUTIONS.with(|n| n.fetch_add(1, Ordering::SeqCst));
    input.field(db)
}

#[salsa::tracked(volatile = 2, returns(copy))]
fn volatile_copy(db: &dyn salsa::Database, input: MyInput) -> u32 {
    input.field(db)
}

#[salsa::tracked(volatile = 2)]
fn volatile_arc(_db: &dyn salsa::Database, _input: MyInput) -> Arc<LiveValue> {
    Arc::new(LiveValue::new())
}

#[salsa::tracked]
fn outer_value(db: &dyn salsa::Database, input: MyInput) -> u32 {
    OUTER_EXECUTIONS.with(|n| n.fetch_add(1, Ordering::SeqCst));
    volatile_value(db, input) + 1
}

#[salsa::tracked(volatile = 2, cycle_initial=cycle_initial)]
fn volatile_cycle_value(db: &dyn salsa::Database, input: MyInput) -> u32 {
    CYCLE_EXECUTIONS.with(|n| n.fetch_add(1, Ordering::SeqCst));

    if input.field(db) != 0 {
        volatile_cycle_value(db, input);
    }

    input.field(db)
}

fn cycle_initial(_db: &dyn salsa::Database, _id: salsa::Id, _input: MyInput) -> u32 {
    0
}

fn fill_volatile_cache(db: &salsa::DatabaseImpl) {
    for field in 100..104 {
        let input = MyInput::new(db, field);
        assert_eq!(volatile_value(db, input), field);
    }
}

fn fill_volatile_cycle_cache(db: &salsa::DatabaseImpl) {
    for field in 100..104 {
        let input = MyInput::new(db, field);
        assert_eq!(volatile_cycle_value(db, input), field);
    }
}

#[test]
fn volatile_evicts_automatically_without_new_revision() {
    reset_counts();
    let db = salsa::DatabaseImpl::new();
    let input = MyInput::new(&db, 22);

    assert_eq!(volatile_value(&db, input), 22);
    assert_eq!(volatile_value(&db, input), 22);
    assert_eq!(volatile_executions(), 1);

    fill_volatile_cache(&db);

    assert_eq!(volatile_value(&db, input), 22);
    assert!(volatile_executions() > 5);
}

#[test]
fn volatile_supports_copy_return_mode() {
    let db = salsa::DatabaseImpl::new();
    let input = MyInput::new(&db, 22);

    assert_eq!(volatile_copy(&db, input), 22);
}

#[test]
fn volatile_drops_values_without_new_revision() {
    assert_eq!(live_values(), 0);
    let db = salsa::DatabaseImpl::new();

    for field in 0..6 {
        let input = MyInput::new(&db, field);
        drop(volatile_arc(&db, input));
    }

    assert_eq!(live_values(), 2);
}

#[test]
fn volatile_eviction_is_safe_with_parallel_reads() {
    let db = salsa::DatabaseImpl::new();
    let inputs = (0..8)
        .map(|field| MyInput::new(&db, field))
        .collect::<Vec<_>>();

    let threads = (0..4)
        .map(|_| {
            let db = db.clone();
            let inputs = inputs.clone();
            std::thread::spawn(move || {
                for _ in 0..100 {
                    for input in &inputs {
                        assert!(*volatile_arc(&db, *input) == LiveValue);
                    }
                }
            })
        })
        .collect::<Vec<_>>();

    for thread in threads {
        thread.join().unwrap();
    }
}

#[test]
fn volatile_keeps_dependency_info() {
    reset_counts();
    let mut db = salsa::DatabaseImpl::new();
    let input = MyInput::new(&db, 22);

    assert_eq!(outer_value(&db, input), 23);
    assert_eq!(volatile_executions(), 1);
    assert_eq!(outer_executions(), 1);

    fill_volatile_cache(&db);
    let volatile_after_fill = volatile_executions();
    db.synthetic_write(Durability::HIGH);

    assert_eq!(outer_value(&db, input), 23);
    assert_eq!(volatile_executions(), volatile_after_fill);
    assert_eq!(outer_executions(), 1);
}

#[test]
fn volatile_evicts_cycle_participants() {
    reset_counts();
    let db = salsa::DatabaseImpl::new();
    let input = MyInput::new(&db, 22);

    assert_eq!(volatile_value(&db, input), 22);
    let cycle_value = volatile_cycle_value(&db, input);

    fill_volatile_cache(&db);
    fill_volatile_cycle_cache(&db);

    let volatile_before = volatile_executions();
    let cycle_before = cycle_executions();

    assert_eq!(volatile_value(&db, input), 22);
    assert_eq!(volatile_cycle_value(&db, input), cycle_value);

    assert!(volatile_executions() > volatile_before);
    assert!(cycle_executions() > cycle_before);
}
